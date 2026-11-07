//! Persisted background jobs executed by one bounded worker thread.
//!
//! The database is the source of truth for job state; the in-memory queue only
//! carries job ids. State transitions go through `JobState::transition` and are
//! serialized by `transition_lock` so pause/cancel cannot race with the worker
//! claiming a job.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::application::error::{AppError, AppResult};
use crate::application::ingestion;
use crate::application::ports::{AppEvent, JobRecord};
use crate::application::views::JobView;
use crate::application::Services;
use crate::domain::document::{ExtractionState, FailureKind};
use crate::domain::ids::{AttemptId, DocumentId, JobId};
use crate::domain::job::{JobKind, JobState};

use super::control::JobControl;

const QUEUE_CAPACITY: usize = 1024;

pub struct JobScheduler {
    svc: Arc<Services>,
    tx: Mutex<Option<SyncSender<JobId>>>,
    controls: Mutex<HashMap<JobId, Arc<JobControl>>>,
    transition_lock: Mutex<()>,
    shutting_down: AtomicBool,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl JobScheduler {
    pub fn start(svc: Arc<Services>) -> Arc<Self> {
        let (tx, rx) = mpsc::sync_channel::<JobId>(QUEUE_CAPACITY);
        let scheduler = Arc::new(Self {
            svc,
            tx: Mutex::new(Some(tx)),
            controls: Mutex::new(HashMap::new()),
            transition_lock: Mutex::new(()),
            shutting_down: AtomicBool::new(false),
            worker: Mutex::new(None),
        });
        let worker_ref = Arc::clone(&scheduler);
        let handle = std::thread::Builder::new()
            .name("papryvio-jobs".into())
            .spawn(move || worker_ref.run(rx))
            .expect("spawn job worker");
        *scheduler.worker.lock().unwrap() = Some(handle);
        scheduler
    }

    pub fn enqueue(&self, id: JobId) {
        if self.shutting_down.load(Ordering::SeqCst) {
            return;
        }
        if let Some(tx) = self.tx.lock().unwrap().as_ref() {
            // A full queue only delays work: queued jobs are re-enqueued at next start.
            if let Err(e) = tx.try_send(id) {
                tracing::warn!("job queue full or closed: {e}");
            }
        }
    }

    fn save_and_emit(&self, job: &JobRecord) -> AppResult<()> {
        self.svc.jobs.save(job)?;
        self.svc.events.emit(AppEvent::JobProgress(job.view()));
        Ok(())
    }

    fn load(&self, id: &JobId) -> AppResult<JobRecord> {
        self.svc.jobs.get(id)?.ok_or_else(|| AppError::not_found("job"))
    }

    pub fn pause(&self, id: &JobId) -> AppResult<JobView> {
        let _g = self.transition_lock.lock().unwrap();
        let mut job = self.load(id)?;
        match job.state {
            JobState::Running => {
                if let Some(c) = self.controls.lock().unwrap().get(id) {
                    c.request_pause();
                }
            }
            JobState::Queued => {
                job.state = job.state.transition(JobState::Paused)?;
                job.stage = "paused".into();
                job.updated_at = self.svc.now();
                self.save_and_emit(&job)?;
            }
            _ => return Err(AppError::not_allowed("job cannot be paused in its current state")),
        }
        Ok(job.view())
    }

    pub fn resume(&self, id: &JobId) -> AppResult<JobView> {
        let _g = self.transition_lock.lock().unwrap();
        let mut job = self.load(id)?;
        if job.state != JobState::Paused {
            return Err(AppError::not_allowed("only paused jobs can be resumed"));
        }
        job.state = job.state.transition(JobState::Queued)?;
        job.stage = "queued".into();
        job.updated_at = self.svc.now();
        self.save_and_emit(&job)?;
        drop(_g);
        self.enqueue(job.id.clone());
        Ok(job.view())
    }

    pub fn cancel(&self, id: &JobId) -> AppResult<JobView> {
        let _g = self.transition_lock.lock().unwrap();
        let mut job = self.load(id)?;
        match job.state {
            JobState::Running => {
                if let Some(c) = self.controls.lock().unwrap().get(id) {
                    c.request_cancel();
                }
            }
            JobState::Queued | JobState::Paused => {
                job.state = job.state.transition(JobState::Cancelled)?;
                job.stage = "cancelled".into();
                job.updated_at = self.svc.now();
                self.save_and_emit(&job)?;
                if job.kind == JobKind::Extract {
                    let doc = DocumentId::from_trusted(job.entity_id.clone());
                    if let Some(rec) = self.svc.documents.get(&doc)? {
                        if rec.deleted_at.is_none() {
                            self.svc.documents.set_state(&doc, ExtractionState::Failed, Some(FailureKind::Cancelled), self.svc.now())?;
                            self.svc.events.emit(AppEvent::DocumentChanged { document_id: doc, state: ExtractionState::Failed });
                        }
                    }
                }
            }
            _ => return Err(AppError::not_allowed("job already finished")),
        }
        Ok(job.view())
    }

    /// Starts a new attempt of a failed or cancelled job. The checkpoint is discarded.
    pub fn retry(&self, id: &JobId) -> AppResult<JobView> {
        let _g = self.transition_lock.lock().unwrap();
        let mut job = self.load(id)?;
        if !matches!(job.state, JobState::Failed | JobState::Cancelled) {
            return Err(AppError::not_allowed("only failed or cancelled jobs can be retried"));
        }
        job.state = job.state.transition(JobState::Queued)?;
        job.attempt_id = AttemptId::new();
        job.checkpoint = None;
        job.completed_units = 0;
        job.total_units = None;
        job.error_kind = None;
        job.error_message = None;
        job.stage = "queued".into();
        job.updated_at = self.svc.now();
        self.save_and_emit(&job)?;
        drop(_g);
        self.enqueue(job.id.clone());
        Ok(job.view())
    }

    /// Asks the running attempt (if any) for this entity to stop, e.g. before deletion.
    pub fn cancel_for_entity(&self, entity_id: &str, kind: JobKind) -> AppResult<()> {
        if let Some(job) = self.svc.jobs.latest_for_entity(entity_id, kind)? {
            if job.state.is_active() {
                let _ = self.cancel(&job.id);
            }
        }
        Ok(())
    }

    /// Stops accepting work and asks the running job to checkpoint. Waits briefly for it.
    pub fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        for c in self.controls.lock().unwrap().values() {
            c.request_interrupt();
        }
        self.tx.lock().unwrap().take();
        if let Some(handle) = self.worker.lock().unwrap().take() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !handle.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }

    /// Moves a queued job to running and registers its control token.
    fn claim(&self, id: &JobId) -> AppResult<Option<(JobRecord, Arc<JobControl>)>> {
        let _g = self.transition_lock.lock().unwrap();
        let Some(mut job) = self.svc.jobs.get(id)? else { return Ok(None) };
        if job.state != JobState::Queued {
            return Ok(None);
        }
        job.state = job.state.transition(JobState::Running)?;
        job.updated_at = self.svc.now();
        self.save_and_emit(&job)?;
        let control = Arc::new(JobControl::new());
        self.controls.lock().unwrap().insert(job.id.clone(), Arc::clone(&control));
        Ok(Some((job, control)))
    }

    fn run(self: Arc<Self>, rx: Receiver<JobId>) {
        while let Ok(id) = rx.recv() {
            if self.shutting_down.load(Ordering::SeqCst) {
                break;
            }
            let claimed = match self.claim(&id) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(job = %id, "claim failed: {e}");
                    continue;
                }
            };
            let Some((mut job, control)) = claimed else { continue };
            let span = tracing::info_span!("job", id = %job.id, kind = job.kind.as_str(), attempt = %job.attempt_id);
            let _enter = span.enter();
            let started = std::time::Instant::now();
            let result = match job.kind {
                JobKind::Extract => ingestion::extract::run_extract_job(&self.svc, &mut job, &control),
                JobKind::Index => crate::application::indexing::run_index_job(&self.svc, &mut job, &control),
                JobKind::Embed => crate::application::indexing::run_embed_job(&self.svc, &mut job, &control),
                JobKind::DeleteDocument => ingestion::delete::run_delete_job(&self.svc, &mut job),
            };
            if let Err(e) = result {
                tracing::error!("job error: {e}");
                job.state = JobState::Failed;
                job.error_kind = Some(format!("{:?}", e.code).to_lowercase());
                job.error_message = Some(e.detail.chars().take(500).collect());
                job.updated_at = self.svc.now();
                let _ = self.save_and_emit(&job);
            }
            if let Err(e) = crate::application::indexing::follow_up(&self.svc, &self, &job) {
                tracing::warn!("follow-up scheduling failed: {e}");
            }
            tracing::info!(state = job.state.as_str(), elapsed_ms = started.elapsed().as_millis() as u64, "job finished");
            self.controls.lock().unwrap().remove(&job.id);
        }
    }
}
