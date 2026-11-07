//! Persistence for conversations, generation attempts, evidence, citations and comparisons.
use crate::application::error::AppResult;
use crate::application::views::{ComparisonView, ConversationSummary, ConversationView, ScopeInput, ScopeSummary};
use crate::domain::evidence::{CitationRef, Evidence, ValidatedAnswer};
use crate::domain::ids::DocumentId;

#[derive(Debug, Clone)]
pub struct NewAttempt {
    pub id: String,
    pub message_id: String,
    pub request_id: String,
    pub model_fingerprint: String,
    pub prompt_version: i64,
    pub scope_snapshot: String,
    pub retrieval_config: String,
    pub now: i64,
}

#[derive(Debug, Clone)]
pub struct CitationRecord {
    pub id: String,
    pub attempt_id: String,
    pub document_id: DocumentId,
    pub page_index: u32,
    pub span: Option<(u32, u32)>,
    pub quote: Option<String>,
    pub excerpt: String,
}

#[derive(Debug, Clone)]
pub struct CellUpdate {
    pub id: String,
    pub status: String,
    pub text: Option<String>,
    /// JSON array of `CellEvidence`.
    pub evidence_json: String,
    pub searched: Option<String>,
    pub now: i64,
}

pub trait AnswerRepository: Send + Sync {
    fn create_conversation(&self, id: &str, title: &str, collection: Option<&str>, now: i64) -> AppResult<()>;
    fn conversation_exists(&self, id: &str) -> AppResult<bool>;
    fn list_conversations(&self, limit: u32) -> AppResult<Vec<ConversationSummary>>;
    fn conversation(&self, id: &str) -> AppResult<Option<ConversationView>>;
    fn delete_conversation(&self, id: &str) -> AppResult<()>;
    fn add_message(&self, id: &str, conversation: &str, role: &str, text: &str, state: &str, now: i64) -> AppResult<()>;
    fn message_question(&self, message_id: &str) -> AppResult<Option<(String, String)>>; // (conversation id, text)
    fn find_attempt_by_request(&self, request_id: &str) -> AppResult<Option<(String, String)>>; // (attempt id, conversation id)
    fn create_attempt(&self, a: &NewAttempt) -> AppResult<()>;
    fn set_attempt_state(&self, id: &str, state: &str) -> AppResult<()>;
    fn set_attempt_scope(&self, id: &str, summary: &ScopeSummary) -> AppResult<()>;
    /// (message id, requested scope) of an attempt.
    fn attempt_inputs(&self, id: &str) -> AppResult<Option<(String, ScopeInput)>>;
    fn attempt_scope(&self, id: &str) -> AppResult<Option<ScopeInput>>;
    fn finish_attempt(&self, id: &str, state: &str, result: Option<&ValidatedAnswer>, error_kind: Option<&str>, timing: &str, now: i64) -> AppResult<()>;
    fn save_evidence(&self, attempt: &str, bundle: &[Evidence]) -> AppResult<()>;
    /// Persists citations and returns them with their ids filled in.
    fn save_citations(&self, attempt: &str, claims: &mut ValidatedAnswer) -> AppResult<()>;
    fn citation(&self, id: &str) -> AppResult<Option<CitationRecord>>;
    fn attempts_running(&self) -> AppResult<Vec<String>>;

    fn create_comparison(&self, id: &str, request_id: &str, docs: &[DocumentId], rows_json: &str, model_fp: &str, cells: &[(String, DocumentId, String, String)], now: i64) -> AppResult<()>;
    fn find_comparison_by_request(&self, request_id: &str) -> AppResult<Option<String>>;
    fn update_cell(&self, u: &CellUpdate) -> AppResult<()>;
    fn set_comparison_state(&self, id: &str, state: &str, now: i64) -> AppResult<()>;
    fn comparison(&self, id: &str) -> AppResult<Option<ComparisonView>>;
    fn latest_comparisons(&self, limit: u32) -> AppResult<Vec<ComparisonView>>;
    fn cell_citation(&self, cell_id: &str, number: u32) -> AppResult<Option<(CitationRef, String)>>; // (citation, excerpt)
}
