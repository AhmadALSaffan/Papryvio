//! Deleting a document from the library: tombstone first, then purge derivatives and file.
use crate::application::error::{AppError, AppResult};
use crate::application::jobs::JobScheduler;
use crate::application::ports::{AppEvent, JobRecord, NewJob};
use crate::application::Services;
use crate::domain::document::ExtractionState;
use crate::domain::ids::{AttemptId, DocumentId, JobId};
use crate::domain::job::{JobKind, JobState};

/// Starts deletion. The document disappears from lists immediately; the job removes the rest.
pub fn delete_document(svc: &Services, scheduler: &JobScheduler, id: &DocumentId) -> AppResult<JobId> {
    let doc = svc.documents.get(id)?.ok_or_else(|| AppError::not_found("document"))?;
    if doc.deleted_at.is_none() {
        svc.documents.mark_deleting(id, svc.now())?;
    }
    scheduler.cancel_for_entity(id.as_str(), JobKind::Extract)?;

    if let Some(existing) = svc.jobs.latest_for_entity(id.as_str(), JobKind::DeleteDocument)? {
        if existing.state.is_active() {
            return Ok(existing.id);
        }
    }
    let job = NewJob {
        id: JobId::new(),
        kind: JobKind::DeleteDocument,
        entity_id: id.to_string(),
        attempt_id: AttemptId::new(),
        stage: "queued".into(),
        now: svc.now(),
    };
    svc.jobs.insert(&job)?;
    svc.events.emit(AppEvent::DocumentChanged { document_id: id.clone(), state: ExtractionState::Deleting });
    svc.events.emit(AppEvent::LibraryChanged);
    scheduler.enqueue(job.id.clone());
    Ok(job.id)
}

pub fn run_delete_job(svc: &Services, job: &mut JobRecord) -> AppResult<()> {
    let doc_id = DocumentId::from_trusted(job.entity_id.clone());
    job.stage = "removing_files".into();
    svc.jobs.save(job)?;
    if svc.documents.get(&doc_id)?.is_some() {
        svc.documents.purge_derived(&doc_id, svc.now())?;
    }
    svc.files.remove_managed(&doc_id)?;
    svc.files.remove_staged(&doc_id);
    job.state = job.state.transition(JobState::Completed)?;
    job.stage = "done".into();
    job.updated_at = svc.now();
    svc.jobs.save(job)?;
    svc.events.emit(AppEvent::JobProgress(job.view()));
    svc.events.emit(AppEvent::DocumentChanged { document_id: doc_id, state: ExtractionState::Deleted });
    svc.events.emit(AppEvent::LibraryChanged);
    Ok(())
}
