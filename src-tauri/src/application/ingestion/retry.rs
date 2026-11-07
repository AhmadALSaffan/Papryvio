//! RetryIngestion: a new extraction attempt for a failed or cancelled document.
use crate::application::error::{AppError, AppResult};
use crate::application::jobs::JobScheduler;
use crate::application::ports::{AppEvent, NewJob};
use crate::application::views::JobView;
use crate::application::Services;
use crate::domain::document::ExtractionState;
use crate::domain::ids::{AttemptId, DocumentId, JobId};
use crate::domain::job::{JobKind, JobState};

pub fn retry_extraction(svc: &Services, scheduler: &JobScheduler, id: &DocumentId) -> AppResult<JobView> {
    let doc = svc.documents.get(id)?.ok_or_else(|| AppError::not_found("document"))?;
    if doc.deleted_at.is_some() {
        return Err(AppError::not_found("document"));
    }
    if !matches!(doc.state, ExtractionState::Failed | ExtractionState::NeedsOcr | ExtractionState::TextReady | ExtractionState::Ready) {
        return Err(AppError::not_allowed("extraction is already in progress"));
    }
    svc.documents.set_state(id, ExtractionState::Imported, None, svc.now())?;
    svc.events.emit(AppEvent::DocumentChanged { document_id: id.clone(), state: ExtractionState::Imported });
    match svc.jobs.latest_for_entity(id.as_str(), JobKind::Extract)? {
        Some(job) if matches!(job.state, JobState::Failed | JobState::Cancelled) => scheduler.retry(&job.id),
        Some(job) if job.state.is_active() => Ok(job.view()),
        _ => {
            let job = NewJob {
                id: JobId::new(),
                kind: JobKind::Extract,
                entity_id: id.to_string(),
                attempt_id: AttemptId::new(),
                stage: "queued".into(),
                now: svc.now(),
            };
            svc.jobs.insert(&job)?;
            scheduler.enqueue(job.id.clone());
            let rec = svc.jobs.get(&job.id)?.ok_or_else(|| AppError::internal("job vanished"))?;
            svc.events.emit(AppEvent::JobProgress(rec.view()));
            Ok(rec.view())
        }
    }
}
