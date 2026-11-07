//! Index storage port: generations, chunks, FTS5, embeddings and the queries retrieval needs.
use crate::application::error::AppResult;
use crate::application::ports::pdf::SpanBox;
use crate::domain::ids::DocumentId;

#[derive(Debug, Clone)]
pub struct NewChunk {
    pub page_index: u32,
    pub chunk_index: u32,
    pub text: String,
    pub search_text: String,
    pub start: u32,
    pub end: u32,
    pub token_estimate: u32,
}

#[derive(Debug, Clone)]
pub struct ChunkRow {
    pub id: i64,
    pub document_id: DocumentId,
    pub generation_id: String,
    pub page_index: u32,
    pub text: String,
    pub start: u32,
    pub end: u32,
}

/// What search may use for one document right now.
#[derive(Debug, Clone)]
pub struct SearchableDoc {
    pub id: DocumentId,
    pub title: String,
    pub authors: Option<String>,
    pub year: Option<i32>,
    pub page_count: Option<u32>,
    pub generation_id: Option<String>,
    pub lexical_ready: bool,
    pub semantic_fingerprint: Option<String>,
    pub state: String,
}

pub trait IndexRepository: Send + Sync {
    /// (page_index, text) of the published extraction revision.
    fn pages(&self, doc: &DocumentId) -> AppResult<Vec<(u32, String)>>;
    fn page_text(&self, doc: &DocumentId, page_index: u32) -> AppResult<Option<String>>;
    fn page_spans(&self, doc: &DocumentId, page_index: u32) -> AppResult<Vec<SpanBox>>;
    /// Inserts a building generation with its chunks (FTS updated by triggers) in one transaction.
    fn create_generation(&self, doc: &DocumentId, chunker_version: i64, chunks: &[NewChunk], now: i64) -> AppResult<String>;
    /// Atomically makes `generation` the active one for keyword search and drops older ones.
    fn publish_generation(&self, doc: &DocumentId, generation: &str, now: i64) -> AppResult<()>;
    fn active_generation(&self, doc: &DocumentId) -> AppResult<Option<String>>;
    /// (total chunks, chunks with a vector for `fingerprint`)
    fn embedding_progress(&self, generation: &str, fingerprint: &str) -> AppResult<(u32, u32)>;
    fn chunks_missing_embedding(&self, generation: &str, fingerprint: &str, limit: u32) -> AppResult<Vec<(i64, String)>>;
    fn store_embeddings(&self, fingerprint: &str, dims: u32, items: &[(i64, Vec<f32>)]) -> AppResult<()>;
    fn mark_semantic_ready(&self, doc: &DocumentId, generation: &str, fingerprint: &str, now: i64) -> AppResult<bool>;
    /// Embedding model changed: no document is semantically searchable until re-embedded.
    fn reset_semantic(&self, keep_fingerprint: Option<&str>) -> AppResult<()>;
    fn searchable_docs(&self, ids: &[DocumentId]) -> AppResult<Vec<SearchableDoc>>;
    /// BM25-ranked chunks from the active generations of `docs` (best first).
    fn lexical_search(&self, fts_query: &str, docs: &[DocumentId], limit: u32) -> AppResult<Vec<(i64, f64)>>;
    /// Vectors of active-generation chunks of `docs` with `fingerprint`.
    fn vectors(&self, docs: &[DocumentId], fingerprint: &str) -> AppResult<Vec<(i64, Vec<f32>)>>;
    fn chunks_by_ids(&self, ids: &[i64]) -> AppResult<Vec<ChunkRow>>;
    /// Chunks of one page in a generation, in reading order.
    fn chunks_for_page(&self, generation_id: &str, page_index: u32) -> AppResult<Vec<ChunkRow>>;
    /// Ids of live documents with an active generation (for re-embedding).
    fn indexed_documents(&self) -> AppResult<Vec<DocumentId>>;
    /// Integrity check used by tests: FTS rows match chunk rows.
    fn fts_consistent(&self) -> AppResult<bool>;
}
