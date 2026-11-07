//! Event adapter: implements the `EventSink` port with Tauri events.
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use ts_rs::TS;

use crate::application::ports::{AppEvent, EventSink};
use crate::application::views::{ImportSelection, JobView};
use crate::domain::document::ExtractionState;
use crate::domain::ids::DocumentId;

pub const SCHEMA_VERSION: u32 = 1;

pub mod names {
    pub const JOB_PROGRESS: &str = "job_progress";
    pub const DOCUMENT_CHANGED: &str = "document_changed";
    pub const LIBRARY_CHANGED: &str = "library_changed";
    pub const FILES_DROPPED: &str = "files_dropped";
    pub const DRAG_STATE: &str = "drag_state";
    pub const GENERATION_PROGRESS: &str = "generation_progress";
    pub const GENERATION_FINISHED: &str = "generation_finished";
    pub const COMPARISON_UPDATED: &str = "comparison_updated";
    pub const MODELS_CHANGED: &str = "models_changed";
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GenerationPayload {
    pub attempt_id: String,
    pub conversation_id: String,
    pub state: String,
    pub chars: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ComparisonPayload {
    pub comparison_id: String,
}

/// Envelope for every event. `sequence` is monotonic per app run so the UI can drop stale updates.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EventEnvelope<T: TS> {
    pub schema_version: u32,
    pub event_id: String,
    #[ts(type = "number")]
    pub sequence: u64,
    pub attempt_id: Option<String>,
    pub payload: T,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DocumentChangedPayload {
    pub document_id: DocumentId,
    pub state: ExtractionState,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DragStatePayload {
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct EmptyPayload {}

pub struct TauriEventSink {
    app: AppHandle,
    seq: AtomicU64,
}

impl TauriEventSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app, seq: AtomicU64::new(0) }
    }

    pub fn send<T: Serialize + TS + Clone>(&self, name: &str, attempt_id: Option<String>, payload: T) {
        let envelope = EventEnvelope {
            schema_version: SCHEMA_VERSION,
            event_id: uuid::Uuid::new_v4().to_string(),
            sequence: self.seq.fetch_add(1, Ordering::SeqCst) + 1,
            attempt_id,
            payload,
        };
        if let Err(e) = self.app.emit(name, envelope) {
            tracing::warn!("emit {name} failed: {e}");
        }
    }

    pub fn files_dropped(&self, selection: ImportSelection) {
        self.send(names::FILES_DROPPED, None, selection);
    }

    pub fn drag_state(&self, active: bool) {
        self.send(names::DRAG_STATE, None, DragStatePayload { active });
    }
}

impl EventSink for TauriEventSink {
    fn emit(&self, event: AppEvent) {
        match event {
            AppEvent::JobProgress(job) => {
                let attempt = Some(job.attempt_id.to_string());
                self.send::<JobView>(names::JOB_PROGRESS, attempt, job)
            }
            AppEvent::DocumentChanged { document_id, state } => {
                self.send(names::DOCUMENT_CHANGED, None, DocumentChangedPayload { document_id, state })
            }
            AppEvent::LibraryChanged => self.send(names::LIBRARY_CHANGED, None, EmptyPayload {}),
            AppEvent::GenerationProgress { attempt_id, conversation_id, state, chars } => self.send(
                names::GENERATION_PROGRESS,
                Some(attempt_id.clone()),
                GenerationPayload { attempt_id, conversation_id, state, chars },
            ),
            AppEvent::GenerationFinished { attempt_id, conversation_id, state } => self.send(
                names::GENERATION_FINISHED,
                Some(attempt_id.clone()),
                GenerationPayload { attempt_id, conversation_id, state, chars: 0 },
            ),
            AppEvent::ComparisonUpdated { comparison_id } => self.send(names::COMPARISON_UPDATED, None, ComparisonPayload { comparison_id }),
            AppEvent::ModelsChanged => self.send(names::MODELS_CHANGED, None, EmptyPayload {}),
        }
    }
}
