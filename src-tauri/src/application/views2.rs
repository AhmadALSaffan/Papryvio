//! Read models for models, search, answers and comparisons (IPC DTOs, exported to TS).
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::application::ports::ProviderKind;
use crate::domain::evidence::ValidatedAnswer;
use crate::domain::ids::DocumentId;

// ── Models ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProviderView {
    pub id: String,
    pub kind: ProviderKind,
    pub label: String,
    pub base_url: String,
    pub is_local: bool,
    pub has_secret: bool,
    pub account: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelChoice {
    pub provider_id: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EmbeddingChoice {
    pub provider_id: String,
    pub model: String,
    pub dimensions: u32,
    pub digest: Option<String>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelSettingsView {
    pub providers: Vec<ProviderView>,
    pub generation: Option<ModelChoice>,
    pub embedding: Option<EmbeddingChoice>,
    /// Provider ids the user agreed may receive paper text.
    pub cloud_consent: Vec<String>,
    pub indexed_docs: u32,
    pub semantic_docs: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelTestResult {
    pub ok: bool,
    pub dimensions: Option<u32>,
    pub sample: Option<String>,
    #[ts(type = "number")]
    pub elapsed_ms: u64,
    pub error: Option<String>,
}

// ── Search ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct ScopeInput {
    pub collection_id: Option<String>,
    pub document_ids: Option<Vec<String>>,
    /// The page open in the reader when the question was asked, so "this page" questions work.
    pub viewing: Option<ViewingPage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ViewingPage {
    pub document_id: String,
    pub page_index: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RetrievalMode {
    Lexical,
    Semantic,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NotSearched {
    pub document_id: DocumentId,
    pub title: String,
    /// `processing`, `needs_ocr`, `failed`, `no_vectors`
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScopeSummary {
    pub label: Option<String>,
    pub requested: u32,
    pub searched: u32,
    pub not_searched: Vec<NotSearched>,
    pub mode: RetrievalMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SearchHitView {
    #[ts(type = "number")]
    pub chunk_id: i64,
    pub document_id: DocumentId,
    pub title: String,
    pub authors: Option<String>,
    pub year: Option<i32>,
    pub page_index: u32,
    pub page_offset: u32,
    pub excerpt: String,
    pub score: f64,
    pub lexical_rank: Option<u32>,
    pub semantic_rank: Option<u32>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SearchResults {
    pub scope: ScopeSummary,
    pub hits: Vec<SearchHitView>,
}

// ── Conversations ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub collection_id: Option<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SourceView {
    pub evidence_id: String,
    pub document_id: DocumentId,
    pub title: String,
    pub page_index: u32,
    pub excerpt: String,
    /// The paper was deleted from the library after this answer.
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AttemptView {
    pub id: String,
    pub state: String,
    pub answer: Option<ValidatedAnswer>,
    pub sources: Vec<SourceView>,
    pub scope: Option<ScopeSummary>,
    pub error_kind: Option<String>,
    pub model: String,
    #[ts(type = "number")]
    pub started_at: i64,
    #[ts(type = "number | null")]
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TurnView {
    pub message_id: String,
    pub question: String,
    #[ts(type = "number")]
    pub asked_at: i64,
    /// Latest attempt; earlier attempts stay stored for diagnostics.
    pub attempt: Option<AttemptView>,
    pub attempt_count: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConversationView {
    pub id: String,
    pub title: String,
    pub collection_id: Option<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    pub turns: Vec<TurnView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AskStarted {
    pub conversation_id: String,
    pub message_id: String,
    pub attempt_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Rect {
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EvidenceLocation {
    pub document_id: DocumentId,
    pub title: String,
    pub page_index: u32,
    pub quote: Option<String>,
    pub excerpt: String,
    /// PDF user-space boxes to highlight; empty when the position was not located reliably.
    pub boxes: Vec<Rect>,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", tag = "type")]
#[ts(export)]
pub enum EvidenceTarget {
    #[serde(rename_all = "camelCase")]
    Citation { citation_id: String },
    #[serde(rename_all = "camelCase")]
    Cell { cell_id: String, number: u32 },
    #[serde(rename_all = "camelCase")]
    Hit {
        #[ts(type = "number")]
        chunk_id: i64,
    },
}

// ── Comparison ────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ComparisonRow {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CellEvidence {
    pub number: u32,
    pub document_id: DocumentId,
    pub page_index: u32,
    pub span: Option<(u32, u32)>,
    pub quote: Option<String>,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ComparisonCellView {
    pub id: String,
    pub document_id: DocumentId,
    pub row_key: String,
    pub status: String,
    pub text: Option<String>,
    pub evidence: Vec<CellEvidence>,
    pub searched: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ComparisonDocView {
    pub id: DocumentId,
    pub title: String,
    pub authors: Option<String>,
    pub year: Option<i32>,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ComparisonView {
    pub id: String,
    pub state: String,
    pub documents: Vec<ComparisonDocView>,
    pub rows: Vec<ComparisonRow>,
    pub cells: Vec<ComparisonCellView>,
    /// Completed cells linked to at least one valid source ("cells linked to sources").
    pub linked_cells: u32,
    pub completed_cells: u32,
    #[ts(type = "number")]
    pub created_at: i64,
}
