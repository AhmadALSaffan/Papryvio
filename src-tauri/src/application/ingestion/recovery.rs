//! Startup recovery: repair interrupted imports and resume persisted jobs.
use crate::application::error::AppResult;
use crate::application::jobs::JobScheduler;
use crate::application::ports::NewJob;
use crate::application::Services;
use crate::domain::document::{ExtractionState, FailureKind};
use crate::domain::ids::{AttemptId, JobId};
use crate::domain::job::{JobKind, JobState};

const IDEMPOTENCY_RETENTION_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Default)]
pub struct RecoveryReport {
    pub promoted_files: u32,
    pub missing_files: u32,
    pub orphan_staging_removed: u32,
    pub resumed_jobs: u32,
    pub recreated_jobs: u32,
}

pub fn recover_on_startup(svc: &Services, scheduler: &JobScheduler) -> AppResult<RecoveryReport> {
    let mut report = RecoveryReport::default();
    let now = svc.now();

    // 1) Imports interrupted between the DB commit and moving the file into the library.
    for doc in svc.documents.list_by_states(&[ExtractionState::Imported, ExtractionState::Extracting])? {
        if !svc.files.managed_exists(&doc.id) {
            if svc.files.staged_exists(&doc.id) {
                svc.files.promote(&doc.id)?;
                report.promoted_files += 1;
            } else {
                svc.documents.set_state(&doc.id, ExtractionState::Failed, Some(FailureKind::FileMissing), now)?;
                report.missing_files += 1;
            }
        }
    }
    // 2) Staging copies without a document row (crash before the DB commit).
    report.orphan_staging_removed = svc.files.cleanup_staging(&svc.documents.all_ids()?)?;

    // 3) Persisted jobs: running ones were interrupted and resume from their checkpoint.
    let mut has_active_extract = std::collections::HashSet::new();
    let mut has_active_delete = std::collections::HashSet::new();
    let mut has_active_index = std::collections::HashSet::new();
    for mut job in svc.jobs.list_active()? {
        match job.kind {
            JobKind::Extract => has_active_extract.insert(job.entity_id.clone()),
            JobKind::DeleteDocument => has_active_delete.insert(job.entity_id.clone()),
            JobKind::Index | JobKind::Embed => has_active_index.insert(job.entity_id.clone()),
        };
        match job.state {
            JobState::Running => {
                job.state = job.state.transition(JobState::Queued)?;
                job.stage = "queued".into();
                job.updated_at = now;
                svc.jobs.save(&job)?;
                scheduler.enqueue(job.id.clone());
                report.resumed_jobs += 1;
            }
            JobState::Queued => {
                scheduler.enqueue(job.id.clone());
                report.resumed_jobs += 1;
            }
            _ => {}
        }
    }

    // 4) Documents whose job row is missing (should not happen, but keep state honest).
    for doc in svc.documents.list_by_states(&[ExtractionState::Imported, ExtractionState::Extracting])? {
        if !has_active_extract.contains(doc.id.as_str()) && svc.files.managed_exists(&doc.id) {
            let job = new_job(JobKind::Extract, doc.id.as_str(), now);
            svc.jobs.insert(&job)?;
            scheduler.enqueue(job.id);
            report.recreated_jobs += 1;
        }
    }
    // Text extracted but never indexed (or indexing interrupted without a job row).
    for doc in svc.documents.list_by_states(&[ExtractionState::TextReady, ExtractionState::Indexing])? {
        if !has_active_index.contains(doc.id.as_str()) {
            let job = new_job(JobKind::Index, doc.id.as_str(), now);
            svc.jobs.insert(&job)?;
            scheduler.enqueue(job.id);
            report.recreated_jobs += 1;
        }
    }
    for doc in svc.documents.list_by_states(&[ExtractionState::Deleting])? {
        if !has_active_delete.contains(doc.id.as_str()) {
            let job = new_job(JobKind::DeleteDocument, doc.id.as_str(), now);
            svc.jobs.insert(&job)?;
            scheduler.enqueue(job.id);
            report.recreated_jobs += 1;
        }
    }

    svc.idempotency.prune_older_than(now - IDEMPOTENCY_RETENTION_MS)?;
    tracing::info!(?report, "startup recovery finished");
    Ok(report)
}

fn new_job(kind: JobKind, entity: &str, now: i64) -> NewJob {
    NewJob {
        id: JobId::new(),
        kind,
        entity_id: entity.to_string(),
        attempt_id: AttemptId::new(),
        stage: "queued".into(),
        now,
    }
}

/// Bumped when extraction or the keyword index format changes in a way that old papers need
/// processing again. 3: letter-reversed Arabic is repaired and Arabic prefixes are stripped.
pub const TEXT_PIPELINE_VERSION: u32 = 3;
const TEXT_PIPELINE_KEY: &str = "text.pipeline_version";

/// Re-extracts papers processed by an older pipeline, once, so fixes reach the existing library.
pub fn upgrade_text_pipeline(svc: &Services, scheduler: &JobScheduler) -> AppResult<u32> {
    let current = svc.settings.get(TEXT_PIPELINE_KEY)?.and_then(|v| v.parse::<u32>().ok()).unwrap_or(1);
    if current >= TEXT_PIPELINE_VERSION {
        return Ok(0);
    }
    let mut queued = 0;
    for doc in svc.documents.list_by_states(&[ExtractionState::TextReady, ExtractionState::Ready])? {
        match super::retry::retry_extraction(svc, scheduler, &doc.id) {
            Ok(_) => queued += 1,
            Err(e) => tracing::warn!("re-extraction not queued: {e}"),
        }
    }
    svc.settings.set(TEXT_PIPELINE_KEY, &TEXT_PIPELINE_VERSION.to_string(), svc.now())?;
    tracing::info!(queued, "text pipeline upgraded");
    Ok(queued)
}
