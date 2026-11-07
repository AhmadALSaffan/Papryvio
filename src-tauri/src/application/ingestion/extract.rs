//! Extraction pipeline stage: PDF → pages with text, geometry and quality flags.
//!
//! Each page is persisted as it arrives and the checkpoint advances with it, so a
//! pause, an app shutdown or a crash resumes at the next unprocessed page.
use std::time::{Duration, Instant};

use crate::application::error::{AppError, AppResult};
use crate::application::jobs::{JobControl, Signal};
use crate::application::ports::{AppEvent, ExtractEvent, ExtractOutcome, Flow, JobRecord};
use crate::application::Services;
use crate::domain::document::{page_quality_flags, state_after_extraction, ExtractionState, FailureKind};
use crate::domain::ids::DocumentId;
use crate::domain::job::{ExtractCheckpoint, JobState};

const EMIT_INTERVAL: Duration = Duration::from_millis(200);

fn fail(svc: &Services, job: &mut JobRecord, doc: &DocumentId, kind: FailureKind, detail: &str) -> AppResult<()> {
    tracing::warn!(failure = kind.as_str(), "extraction failed: {detail}");
    job.state = job.state.transition(JobState::Failed)?;
    job.stage = "failed".into();
    job.error_kind = Some(kind.as_str().into());
    job.error_message = Some(detail.chars().take(500).collect());
    job.updated_at = svc.now();
    svc.jobs.save(job)?;
    svc.events.emit(AppEvent::JobProgress(job.view()));
    if let Some(rec) = svc.documents.get(doc)? {
        if rec.deleted_at.is_none() {
            svc.documents.set_state(doc, ExtractionState::Failed, Some(kind), svc.now())?;
            svc.events.emit(AppEvent::DocumentChanged { document_id: doc.clone(), state: ExtractionState::Failed });
        }
    }
    Ok(())
}

pub fn run_extract_job(svc: &Services, job: &mut JobRecord, control: &JobControl) -> AppResult<()> {
    let doc_id = DocumentId::from_trusted(job.entity_id.clone());
    let Some(doc) = svc.documents.get(&doc_id)? else {
        return fail(svc, job, &doc_id, FailureKind::FileMissing, "document row missing");
    };
    if doc.deleted_at.is_some() {
        job.state = job.state.transition(JobState::Cancelled)?;
        job.stage = "cancelled".into();
        job.updated_at = svc.now();
        svc.jobs.save(job)?;
        return Ok(());
    }
    if !svc.pdf.is_available() {
        return fail(svc, job, &doc_id, FailureKind::PdfEngineMissing, "pdfium library not found");
    }
    let Some(rel) = doc.managed_file.as_deref() else {
        return fail(svc, job, &doc_id, FailureKind::FileMissing, "no managed file");
    };
    let path = svc.files.resolve_managed(rel)?;
    if !path.is_file() {
        return fail(svc, job, &doc_id, FailureKind::FileMissing, "managed file missing");
    }

    let checkpoint: Option<ExtractCheckpoint> = job.checkpoint.as_deref().and_then(|c| serde_json::from_str(c).ok());
    let (revision, start_page) = match checkpoint {
        Some(cp) => (cp.revision, cp.next_page),
        None => (svc.documents.begin_extraction_revision(&doc_id, svc.now())?, 0),
    };

    job.stage = if start_page == 0 { "opening".into() } else { "reading_pages".into() };
    job.completed_units = start_page;
    job.checkpoint = Some(serde_json::to_string(&ExtractCheckpoint { revision, next_page: start_page }).unwrap());
    job.updated_at = svc.now();
    svc.jobs.save(job)?;
    svc.events.emit(AppEvent::JobProgress(job.view()));
    svc.documents.set_state(&doc_id, ExtractionState::Extracting, None, svc.now())?;
    svc.events.emit(AppEvent::DocumentChanged { document_id: doc_id.clone(), state: ExtractionState::Extracting });

    let mut storage_error: Option<AppError> = None;
    let mut last_emit = Instant::now() - EMIT_INTERVAL;

    let outcome = {
        let job_ref = &mut *job;
        let mut on_event = |event: ExtractEvent| -> Flow {
            let step: AppResult<()> = (|| {
                match event {
                    ExtractEvent::Meta(meta) => {
                        if start_page == 0 {
                            svc.documents.apply_extracted_metadata(&doc_id, &meta, svc.now())?;
                        }
                        job_ref.total_units = Some(meta.page_count);
                        job_ref.stage = "reading_pages".into();
                        job_ref.updated_at = svc.now();
                        svc.jobs.save(job_ref)?;
                        svc.events.emit(AppEvent::JobProgress(job_ref.view()));
                        svc.events.emit(AppEvent::DocumentChanged { document_id: doc_id.clone(), state: ExtractionState::Extracting });
                    }
                    ExtractEvent::Page(page) => {
                        let flags = page_quality_flags(&page.text, page.image_objects).join(",");
                        svc.pages.save_page(&doc_id, revision, &page, &flags)?;
                        let next = page.index + 1;
                        job_ref.completed_units = next;
                        job_ref.checkpoint = Some(serde_json::to_string(&ExtractCheckpoint { revision, next_page: next }).unwrap());
                        job_ref.updated_at = svc.now();
                        svc.jobs.save(job_ref)?;
                        let is_last = job_ref.total_units.is_some_and(|t| next >= t);
                        if is_last || last_emit.elapsed() >= EMIT_INTERVAL {
                            last_emit = Instant::now();
                            svc.events.emit(AppEvent::JobProgress(job_ref.view()));
                        }
                    }
                }
                Ok(())
            })();
            if let Err(e) = step {
                storage_error = Some(e);
                return Flow::Cancel;
            }
            match control.signal() {
                Signal::None => Flow::Continue,
                Signal::Pause | Signal::Interrupt => Flow::Pause,
                Signal::Cancel => Flow::Cancel,
            }
        };
        svc.pdf.extract(&path, start_page, control, &mut on_event)
    };

    if let Some(e) = storage_error {
        // Keep the checkpoint so a retry can continue; surface the storage problem.
        return Err(e);
    }

    match outcome {
        Ok(ExtractOutcome::Completed) => {
            let total = job.total_units.unwrap_or(job.completed_units);
            let (text_pages, low_pages) = svc.pages.text_page_counts(&doc_id, revision)?;
            let final_state = state_after_extraction(text_pages, total);
            svc.documents.finish_extraction(&doc_id, revision, text_pages, low_pages, final_state, svc.now())?;
            job.state = job.state.transition(JobState::Completed)?;
            job.stage = "done".into();
            job.completed_units = total;
            job.checkpoint = None;
            job.updated_at = svc.now();
            svc.jobs.save(job)?;
            svc.events.emit(AppEvent::JobProgress(job.view()));
            svc.events.emit(AppEvent::DocumentChanged { document_id: doc_id, state: final_state });
            svc.events.emit(AppEvent::LibraryChanged);
            Ok(())
        }
        Ok(ExtractOutcome::Paused) => {
            let next = if control.signal() == Signal::Interrupt { JobState::Queued } else { JobState::Paused };
            job.state = job.state.transition(next)?;
            job.stage = if next == JobState::Paused { "paused".into() } else { "queued".into() };
            job.updated_at = svc.now();
            svc.jobs.save(job)?;
            svc.events.emit(AppEvent::JobProgress(job.view()));
            Ok(())
        }
        Ok(ExtractOutcome::Cancelled) => {
            job.state = job.state.transition(JobState::Cancelled)?;
            job.stage = "cancelled".into();
            job.updated_at = svc.now();
            svc.jobs.save(job)?;
            svc.events.emit(AppEvent::JobProgress(job.view()));
            if let Some(rec) = svc.documents.get(&doc_id)? {
                if rec.deleted_at.is_none() {
                    svc.documents.set_state(&doc_id, ExtractionState::Failed, Some(FailureKind::Cancelled), svc.now())?;
                    svc.events.emit(AppEvent::DocumentChanged { document_id: doc_id, state: ExtractionState::Failed });
                }
            }
            Ok(())
        }
        Err(failure) => fail(svc, job, &doc_id, failure.kind, &failure.detail),
    }
}
