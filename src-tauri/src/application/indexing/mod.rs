//! Indexing pipeline: pages → chunks → FTS5 (published atomically) → embeddings.
//! Keyword search becomes available as soon as a generation is published; meaning-based
//! search per document once all its chunks have vectors for the current embedding model.
use crate::application::error::{AppError, AppResult};
use crate::application::jobs::{JobControl, JobScheduler, Signal};
use crate::application::models;
use crate::application::ports::{AppEvent, JobRecord, NewChunk};
use crate::application::Services;
use crate::domain::chunking::{chunk_page, CHUNKER_VERSION};
use crate::domain::document::ExtractionState;
use crate::domain::ids::DocumentId;
use crate::domain::job::{JobKind, JobState};
use crate::domain::retrieval::normalize_vector;
use crate::domain::text::search_text;

const EMBED_BATCH: u32 = 16;

fn finish(svc: &Services, job: &mut JobRecord, state: JobState, stage: &str, error: Option<(&str, String)>) -> AppResult<()> {
    job.state = job.state.transition(state)?;
    job.stage = stage.into();
    if let Some((kind, msg)) = error {
        job.error_kind = Some(kind.into());
        job.error_message = Some(msg.chars().take(500).collect());
    }
    job.updated_at = svc.now();
    svc.jobs.save(job)?;
    svc.events.emit(AppEvent::JobProgress(job.view()));
    Ok(())
}

/// Called after any job finishes: schedules the next pipeline stage.
pub fn follow_up(svc: &Services, scheduler: &JobScheduler, job: &JobRecord) -> AppResult<()> {
    if job.state != JobState::Completed {
        return Ok(());
    }
    let doc = DocumentId::from_trusted(job.entity_id.clone());
    match job.kind {
        JobKind::Extract => {
            if svc.documents.get(&doc)?.is_some_and(|d| d.state == ExtractionState::TextReady && d.deleted_at.is_none()) {
                models::enqueue(svc, scheduler, JobKind::Index, doc.as_str())?;
            }
        }
        JobKind::Index => {
            if models::embedding_choice(svc)?.is_some() {
                models::enqueue(svc, scheduler, JobKind::Embed, doc.as_str())?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn run_index_job(svc: &Services, job: &mut JobRecord, control: &JobControl) -> AppResult<()> {
    let doc_id = DocumentId::from_trusted(job.entity_id.clone());
    let Some(doc) = svc.documents.get(&doc_id)? else { return finish(svc, job, JobState::Cancelled, "cancelled", None) };
    if doc.deleted_at.is_some() || !matches!(doc.state, ExtractionState::TextReady | ExtractionState::Ready | ExtractionState::Indexing) {
        return finish(svc, job, JobState::Cancelled, "cancelled", None);
    }
    svc.documents.set_state(&doc_id, ExtractionState::Indexing, None, svc.now())?;
    svc.events.emit(AppEvent::DocumentChanged { document_id: doc_id.clone(), state: ExtractionState::Indexing });

    let pages = svc.index.pages(&doc_id)?;
    job.stage = "chunking".into();
    job.total_units = Some(pages.len() as u32);
    job.completed_units = 0;
    svc.jobs.save(job)?;
    svc.events.emit(AppEvent::JobProgress(job.view()));

    let mut chunks = Vec::new();
    for (i, (page_index, text)) in pages.iter().enumerate() {
        for (n, c) in chunk_page(text).into_iter().enumerate() {
            chunks.push(NewChunk {
                page_index: *page_index,
                chunk_index: n as u32,
                search_text: search_text(&c.text),
                text: c.text,
                start: c.start as u32,
                end: c.end as u32,
                token_estimate: c.token_estimate as u32,
            });
        }
        job.completed_units = i as u32 + 1;
        if control.signal() != Signal::None {
            // Chunking is cheap and restartable: stop without publishing anything.
            let next = if control.signal() == Signal::Cancel { JobState::Cancelled } else if control.signal() == Signal::Interrupt { JobState::Queued } else { JobState::Paused };
            svc.documents.set_state(&doc_id, ExtractionState::TextReady, None, svc.now())?;
            return finish(svc, job, next, if next == JobState::Paused { "paused" } else { "queued" }, None);
        }
    }
    let generation = svc.index.create_generation(&doc_id, CHUNKER_VERSION, &chunks, svc.now())?;
    // Re-check: the paper may have been deleted while chunking.
    if svc.documents.get(&doc_id)?.is_none_or(|d| d.deleted_at.is_some()) {
        return finish(svc, job, JobState::Cancelled, "cancelled", None);
    }
    svc.index.publish_generation(&doc_id, &generation, svc.now())?;
    finish(svc, job, JobState::Completed, "done", None)?;
    svc.events.emit(AppEvent::DocumentChanged { document_id: doc_id, state: ExtractionState::Ready });
    svc.events.emit(AppEvent::LibraryChanged);
    Ok(())
}

pub fn run_embed_job(svc: &Services, job: &mut JobRecord, control: &JobControl) -> AppResult<()> {
    let doc_id = DocumentId::from_trusted(job.entity_id.clone());
    let Some(choice) = models::embedding_choice(svc)? else { return finish(svc, job, JobState::Completed, "no_model", None) };
    let Some(generation) = svc.index.active_generation(&doc_id)? else { return finish(svc, job, JobState::Cancelled, "cancelled", None) };
    let (record, client) = match models::open(svc, &choice.provider_id) {
        Ok(v) => v,
        Err(e) => return finish(svc, job, JobState::Failed, "failed", Some(("provider_unavailable", e.detail))),
    };
    if let Err(e) = models::ensure_allowed(svc, &record) {
        return finish(svc, job, JobState::Failed, "failed", Some(("not_allowed", e.detail)));
    }
    let fp = choice.fingerprint.clone();
    let (total, done) = svc.index.embedding_progress(&generation, &fp)?;
    job.stage = "embedding".into();
    job.total_units = Some(total);
    job.completed_units = done;
    svc.jobs.save(job)?;
    svc.events.emit(AppEvent::JobProgress(job.view()));

    loop {
        match control.signal() {
            Signal::None => {}
            Signal::Cancel => return finish(svc, job, JobState::Cancelled, "cancelled", None),
            Signal::Pause => return finish(svc, job, JobState::Paused, "paused", None),
            Signal::Interrupt => return finish(svc, job, JobState::Queued, "queued", None),
        }
        let batch = svc.index.chunks_missing_embedding(&generation, &fp, EMBED_BATCH)?;
        if batch.is_empty() {
            break;
        }
        let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
        let vectors = match client.embed(&choice.model, &texts) {
            Ok(v) => v,
            Err(e) => return finish(svc, job, JobState::Failed, "failed", Some(("model_error", e.detail))),
        };
        if vectors.len() != batch.len() {
            return finish(svc, job, JobState::Failed, "failed", Some(("model_error", "vector count mismatch".into())));
        }
        let mut items = Vec::with_capacity(batch.len());
        for ((id, _), v) in batch.iter().zip(vectors) {
            match normalize_vector(&v, Some(choice.dimensions as usize)) {
                Ok(n) => items.push((*id, n)),
                // Different dimensions mean the model changed under the same name: rebuild needed.
                Err(reason) => return finish(svc, job, JobState::Failed, "failed", Some(("dimension_mismatch", reason.into()))),
            }
        }
        svc.index.store_embeddings(&fp, choice.dimensions, &items)?;
        job.completed_units += items.len() as u32;
        job.updated_at = svc.now();
        svc.jobs.save(job)?;
        svc.events.emit(AppEvent::JobProgress(job.view()));
    }
    let published = svc.index.mark_semantic_ready(&doc_id, &generation, &fp, svc.now())?;
    if !published {
        return Err(AppError::conflict("generation changed while embedding"));
    }
    finish(svc, job, JobState::Completed, "done", None)?;
    svc.events.emit(AppEvent::DocumentChanged { document_id: doc_id, state: ExtractionState::Ready });
    svc.events.emit(AppEvent::LibraryChanged);
    Ok(())
}
