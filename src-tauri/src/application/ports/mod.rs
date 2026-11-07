//! Ports: the interfaces use cases depend on. Infrastructure implements them and
//! `bootstrap.rs` injects the concrete adapters.
pub mod answers;
pub mod index;
pub mod models;
pub mod pdf;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::application::error::AppResult;
use crate::application::views::{CollectionView, DocumentListPage, DocumentQuery, DocumentView, JobView};
use crate::domain::collection::Collection;
use crate::domain::document::{ExtractionState, FailureKind, MetadataOrigin};
use crate::domain::ids::{AttemptId, CollectionId, DocumentId, JobId};
use crate::domain::job::{JobKind, JobState};
use crate::domain::onboarding::{StepKey, StepState};

pub use answers::{AnswerRepository, CellUpdate, CitationRecord, NewAttempt};
pub use index::{ChunkRow, IndexRepository, NewChunk, SearchableDoc};
pub use models::{ChatOutcome, ChatRequest, ModelInfo, ModelProvider, ProviderKind};
pub use pdf::{ExtractEvent, ExtractFailure, ExtractOutcome, ExtractedMetadata, ExtractedPage, Flow, PdfExtractor, SpanBox};

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;
}

pub trait CollectionRepository: Send + Sync {
    /// Fails with `Conflict` when another collection already has this name.
    fn create(&self, collection: &Collection) -> AppResult<()>;
    fn rename(&self, id: &CollectionId, name: &str, now: i64) -> AppResult<()>;
    fn delete(&self, id: &CollectionId) -> AppResult<()>;
    fn exists(&self, id: &CollectionId) -> AppResult<bool>;
    fn list(&self) -> AppResult<Vec<CollectionView>>;
    fn get(&self, id: &CollectionId) -> AppResult<Option<CollectionView>>;
}

#[derive(Debug, Clone)]
pub struct NewDocument {
    pub id: DocumentId,
    pub content_hash: String,
    pub managed_file: String,
    pub original_file_name: String,
    pub file_size: u64,
    pub title: String,
    pub now: i64,
}

#[derive(Debug, Clone)]
pub struct DocumentRecord {
    pub id: DocumentId,
    pub managed_file: Option<String>,
    pub original_file_name: String,
    pub title_origin: MetadataOrigin,
    pub authors_origin: Option<MetadataOrigin>,
    pub year_origin: Option<MetadataOrigin>,
    pub page_count: Option<u32>,
    pub state: ExtractionState,
    pub extraction_revision: i64,
    pub deleted_at: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct UserMetadata {
    pub title: Option<String>,
    /// `Some(None)` clears the field.
    pub authors: Option<Option<String>>,
    pub year: Option<Option<i32>>,
}

pub trait DocumentRepository: Send + Sync {
    fn find_live_by_hash(&self, content_hash: &str) -> AppResult<Option<DocumentId>>;
    /// Inserts the document, links it to the collection and queues its extraction job atomically.
    fn register_import(&self, doc: &NewDocument, collection: &CollectionId, job: &NewJob) -> AppResult<()>;
    /// Returns true when a new link was created.
    fn link(&self, collection: &CollectionId, doc: &DocumentId, now: i64) -> AppResult<bool>;
    fn unlink(&self, collection: &CollectionId, doc: &DocumentId) -> AppResult<bool>;
    fn get(&self, id: &DocumentId) -> AppResult<Option<DocumentRecord>>;
    fn view(&self, id: &DocumentId) -> AppResult<Option<DocumentView>>;
    fn list_views(&self, query: &DocumentQuery) -> AppResult<DocumentListPage>;
    fn list_by_states(&self, states: &[ExtractionState]) -> AppResult<Vec<DocumentRecord>>;
    /// Ids of every document row, deleted or not; used to clean orphaned staging files.
    fn all_ids(&self) -> AppResult<HashSet<String>>;
    fn count_live(&self) -> AppResult<u32>;
    fn set_state(&self, id: &DocumentId, state: ExtractionState, failure: Option<FailureKind>, now: i64) -> AppResult<()>;
    /// Applies PDF metadata without overriding fields the user corrected.
    fn apply_extracted_metadata(&self, id: &DocumentId, meta: &ExtractedMetadata, now: i64) -> AppResult<()>;
    fn update_user_metadata(&self, id: &DocumentId, update: &UserMetadata, now: i64) -> AppResult<()>;
    /// Publishes a finished extraction revision and drops pages from older revisions in one transaction.
    fn finish_extraction(&self, id: &DocumentId, revision: i64, text_pages: u32, low_text_pages: u32, state: ExtractionState, now: i64) -> AppResult<()>;
    fn begin_extraction_revision(&self, id: &DocumentId, now: i64) -> AppResult<i64>;
    /// Tombstones the document so it disappears from lists and searches immediately.
    fn mark_deleting(&self, id: &DocumentId, now: i64) -> AppResult<()>;
    /// Removes pages, spans and collection links; keeps a tombstone row for history.
    fn purge_derived(&self, id: &DocumentId, now: i64) -> AppResult<()>;
}

pub trait PageRepository: Send + Sync {
    /// Stores one extracted page (idempotent per document/revision/page).
    fn save_page(&self, doc: &DocumentId, revision: i64, page: &ExtractedPage, quality_flags: &str) -> AppResult<()>;
    fn clear_revision(&self, doc: &DocumentId, revision: i64) -> AppResult<()>;
    /// (pages with text, pages with weak text)
    fn text_page_counts(&self, doc: &DocumentId, revision: i64) -> AppResult<(u32, u32)>;
}

#[derive(Debug, Clone)]
pub struct NewJob {
    pub id: JobId,
    pub kind: JobKind,
    pub entity_id: String,
    pub attempt_id: AttemptId,
    pub stage: String,
    pub now: i64,
}

#[derive(Debug, Clone)]
pub struct JobRecord {
    pub id: JobId,
    pub kind: JobKind,
    pub entity_id: String,
    pub attempt_id: AttemptId,
    pub state: JobState,
    pub stage: String,
    pub checkpoint: Option<String>,
    pub completed_units: u32,
    pub total_units: Option<u32>,
    pub error_kind: Option<String>,
    pub error_message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl JobRecord {
    pub fn view(&self) -> JobView {
        JobView {
            id: self.id.clone(),
            kind: self.kind,
            entity_id: self.entity_id.clone(),
            attempt_id: self.attempt_id.clone(),
            state: self.state,
            stage: self.stage.clone(),
            completed_units: self.completed_units,
            total_units: self.total_units,
            error_kind: self.error_kind.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

pub trait JobRepository: Send + Sync {
    fn insert(&self, job: &NewJob) -> AppResult<()>;
    fn get(&self, id: &JobId) -> AppResult<Option<JobRecord>>;
    fn latest_for_entity(&self, entity_id: &str, kind: JobKind) -> AppResult<Option<JobRecord>>;
    /// Persists state, stage, checkpoint, progress and error fields.
    fn save(&self, job: &JobRecord) -> AppResult<()>;
    fn list_active(&self) -> AppResult<Vec<JobRecord>>;
}

pub trait SettingsRepository: Send + Sync {
    fn get(&self, key: &str) -> AppResult<Option<String>>;
    fn set(&self, key: &str, value: &str, now: i64) -> AppResult<()>;
}

#[derive(Debug, Clone)]
pub struct StepRecord {
    pub key: StepKey,
    pub state: StepState,
    pub related_entity_id: Option<String>,
    pub updated_at: i64,
}

pub trait OnboardingRepository: Send + Sync {
    fn list(&self, flow_version: i64) -> AppResult<Vec<StepRecord>>;
    fn set(&self, flow_version: i64, step: StepKey, state: StepState, related: Option<&str>, now: i64) -> AppResult<()>;
    fn reset(&self, flow_version: i64) -> AppResult<()>;
}

/// Request-id based idempotency for write commands.
pub trait IdempotencyStore: Send + Sync {
    fn get(&self, request_id: &str) -> AppResult<Option<(String, String, String)>>;
    fn put(&self, request_id: &str, command: &str, payload_hash: &str, response: &str, now: i64) -> AppResult<()>;
    fn prune_older_than(&self, cutoff: i64) -> AppResult<()>;
}

#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub file_name: String,
    pub size: u64,
    pub sha256: String,
    pub looks_like_pdf: bool,
}

/// Application-managed files inside AppData (library, staging, cache, exports, logs).
pub trait FileStore: Send + Sync {
    /// Reads the user's chosen file without modifying it: size, PDF signature, SHA-256.
    fn inspect_source(&self, source: &Path, max_size: u64) -> AppResult<SourceInfo>;
    fn stage_copy(&self, source: &Path, id: &DocumentId) -> AppResult<()>;
    /// Moves the staged copy into the library; returns its library-relative path.
    fn promote(&self, id: &DocumentId) -> AppResult<String>;
    fn managed_rel_path(&self, id: &DocumentId) -> String;
    /// Resolves a library-relative path and verifies it stays inside the library.
    fn resolve_managed(&self, rel: &str) -> AppResult<PathBuf>;
    fn managed_exists(&self, id: &DocumentId) -> bool;
    fn staged_exists(&self, id: &DocumentId) -> bool;
    fn remove_managed(&self, id: &DocumentId) -> AppResult<()>;
    fn remove_staged(&self, id: &DocumentId);
    /// Deletes staging files that no document row refers to. Returns how many were removed.
    fn cleanup_staging(&self, known_ids: &HashSet<String>) -> AppResult<u32>;
    fn data_dir(&self) -> PathBuf;
    fn library_bytes(&self) -> u64;
    fn database_bytes(&self) -> u64;
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    JobProgress(JobView),
    DocumentChanged { document_id: DocumentId, state: ExtractionState },
    LibraryChanged,
    /// Draft progress of an answer (characters received so far).
    GenerationProgress { attempt_id: String, conversation_id: String, state: String, chars: u32 },
    GenerationFinished { attempt_id: String, conversation_id: String, state: String },
    ComparisonUpdated { comparison_id: String },
    ModelsChanged,
}

/// Observer port: pushes live updates to the UI. The database stays the source of truth.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: AppEvent);
}

// ── Model providers and accounts ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ProviderRecord {
    pub id: String,
    pub kind: ProviderKind,
    pub label: String,
    pub base_url: String,
    /// Key of the credential in the OS store; never the secret itself.
    pub secret_ref: Option<String>,
    pub external_id: Option<String>,
    pub config: serde_json::Value,
    pub created_at: i64,
}

pub trait ProviderRepository: Send + Sync {
    fn save(&self, p: &ProviderRecord, now: i64) -> AppResult<()>;
    fn get(&self, id: &str) -> AppResult<Option<ProviderRecord>>;
    fn list(&self) -> AppResult<Vec<ProviderRecord>>;
    fn delete(&self, id: &str) -> AppResult<()>;
}

/// OS credential store (Windows Credential Manager).
pub trait SecretStore: Send + Sync {
    fn set(&self, key: &str, value: &str) -> AppResult<()>;
    fn get(&self, key: &str) -> AppResult<Option<String>>;
    fn delete(&self, key: &str) -> AppResult<()>;
}

/// Builds a live provider client from a stored record (resolving and refreshing its secret).
pub trait ProviderFactory: Send + Sync {
    fn open(&self, record: &ProviderRecord) -> AppResult<std::sync::Arc<dyn ModelProvider>>;
}

#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SignInStart {
    pub flow_id: String,
    /// `device`: show `user_code`, the user enters it at `verification_uri`.
    /// `browser`: open `verification_uri`; the browser returns to a loopback listener.
    pub kind: String,
    pub user_code: Option<String>,
    pub verification_uri: String,
    pub expires_in: u32,
    pub interval: u32,
}

#[derive(Debug, Clone)]
pub enum SignInPoll {
    Pending,
    Complete { label: String, external_id: String, secret_json: String },
    Denied(String),
    Expired,
    Failed(String),
}

/// Account sign-in flows (GitHub device flow, Sign in with ChatGPT).
pub trait SignInService: Send + Sync {
    fn start(&self, kind: ProviderKind) -> AppResult<SignInStart>;
    fn poll(&self, flow_id: &str) -> AppResult<SignInPoll>;
    fn cancel(&self, flow_id: &str);
}
