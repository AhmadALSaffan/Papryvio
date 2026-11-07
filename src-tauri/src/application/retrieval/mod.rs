//! Search engine: explicit scope, keyword (FTS5/BM25), meaning (vectors) and hybrid (RRF).
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;

use crate::application::error::{AppError, AppResult};
use crate::application::models;
use crate::application::ports::{ChunkRow, ModelProvider, SearchableDoc};
use crate::application::views::{
    DocumentQuery, EmbeddingChoice, NotSearched, RetrievalMode, ScopeInput, ScopeSummary, SearchHitView, SearchResults,
};
use crate::application::Services;
use crate::domain::ids::{CollectionId, DocumentId};
use crate::domain::retrieval::{cap_per_document, dot, fts_query, normalize_vector, reciprocal_rank_fusion, CANDIDATES_PER_PATH, MAX_PER_DOCUMENT, RRF_K};

pub const MAX_QUERY_CHARS: usize = 2000;

pub struct ResolvedScope {
    pub summary: ScopeSummary,
    /// Documents searchable by keyword.
    pub lexical: Vec<SearchableDoc>,
    /// Documents with vectors for the current embedding model.
    pub semantic: Vec<SearchableDoc>,
    pub embedder: Option<(EmbeddingChoice, Arc<dyn ModelProvider>)>,
}

impl ResolvedScope {
    pub fn doc(&self, id: &DocumentId) -> Option<&SearchableDoc> {
        self.lexical.iter().find(|d| &d.id == id)
    }
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub chunk: ChunkRow,
    pub score: f64,
    pub lexical_rank: Option<u32>,
    pub semantic_rank: Option<u32>,
}

pub fn resolve_scope(svc: &Services, input: &ScopeInput, mode: RetrievalMode) -> AppResult<ResolvedScope> {
    let (requested, label): (Vec<DocumentId>, Option<String>) = if let Some(ids) = input.document_ids.as_ref().filter(|v| !v.is_empty()) {
        if ids.len() > 500 {
            return Err(AppError::validation("too many documents in scope"));
        }
        let parsed = ids.iter().map(|i| DocumentId::parse(i).ok_or_else(|| AppError::validation("bad document id"))).collect::<AppResult<Vec<_>>>()?;
        (parsed, None)
    } else {
        let collection = match input.collection_id.as_deref().filter(|c| !c.is_empty()) {
            Some(c) => Some(CollectionId::parse(c).ok_or_else(|| AppError::validation("bad collection id"))?),
            None => None,
        };
        let label = match &collection {
            Some(c) => Some(svc.collections.get(c)?.ok_or_else(|| AppError::not_found("collection"))?.name),
            None => None,
        };
        let page = svc.documents.list_views(&DocumentQuery { collection_id: collection, limit: Some(500), ..Default::default() })?;
        (page.items.into_iter().map(|d| d.id).collect(), label)
    };
    let docs = svc.index.searchable_docs(&requested)?;
    let embedder = if mode == RetrievalMode::Lexical {
        None
    } else {
        match models::embedding_choice(svc)? {
            Some(choice) => match models::open(svc, &choice.provider_id) {
                Ok((record, client)) if models::ensure_allowed(svc, &record).is_ok() => Some((choice, client)),
                _ => None,
            },
            None => None,
        }
    };
    let mut lexical = Vec::new();
    let mut semantic = Vec::new();
    let mut not_searched = Vec::new();
    for d in docs {
        if d.lexical_ready && d.generation_id.is_some() {
            let has_vectors = embedder.as_ref().is_some_and(|(c, _)| d.semantic_fingerprint.as_deref() == Some(c.fingerprint.as_str()));
            if has_vectors {
                semantic.push(d.clone());
            } else if mode == RetrievalMode::Semantic {
                not_searched.push(NotSearched { document_id: d.id.clone(), title: d.title.clone(), reason: "no_vectors".into() });
                continue;
            }
            lexical.push(d);
        } else {
            let reason = match d.state.as_str() {
                "needs_ocr" => "needs_ocr",
                "failed" => "failed",
                _ => "processing",
            };
            not_searched.push(NotSearched { document_id: d.id.clone(), title: d.title.clone(), reason: reason.into() });
        }
    }
    let effective = match mode {
        RetrievalMode::Lexical => RetrievalMode::Lexical,
        RetrievalMode::Semantic => RetrievalMode::Semantic,
        RetrievalMode::Hybrid if !semantic.is_empty() => RetrievalMode::Hybrid,
        RetrievalMode::Hybrid => RetrievalMode::Lexical,
    };
    let searched = if effective == RetrievalMode::Semantic { semantic.len() } else { lexical.len() } as u32;
    Ok(ResolvedScope {
        summary: ScopeSummary { label, requested: requested.len() as u32, searched, not_searched, mode: effective },
        lexical,
        semantic,
        embedder: if effective == RetrievalMode::Lexical { None } else { embedder },
    })
}

#[derive(PartialEq)]
struct Scored(f32, i64);
impl Eq for Scored {}
impl PartialOrd for Scored {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Scored {
    // Reversed so the heap's top is the *worst* kept candidate.
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.partial_cmp(&self.0).unwrap_or(Ordering::Equal).then(self.1.cmp(&other.1))
    }
}

fn top_k_by_similarity(query: &[f32], vectors: &[(i64, Vec<f32>)], k: usize) -> Vec<i64> {
    let mut heap: BinaryHeap<Scored> = BinaryHeap::with_capacity(k + 1);
    for (id, v) in vectors {
        if v.len() != query.len() {
            continue;
        }
        heap.push(Scored(dot(query, v), *id));
        if heap.len() > k {
            heap.pop();
        }
    }
    let mut out: Vec<Scored> = heap.into_vec();
    out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal).then(a.1.cmp(&b.1)));
    out.into_iter().map(|s| s.1).collect()
}

/// Embeds the query once for meaning-based search (None when the scope is keyword-only).
pub fn embed_query(scope: &ResolvedScope, query: &str) -> AppResult<Option<Vec<f32>>> {
    let Some((choice, client)) = &scope.embedder else { return Ok(None) };
    if scope.semantic.is_empty() {
        return Ok(None);
    }
    let q = client.embed(&choice.model, &[query.to_string()])?;
    let qv = q.into_iter().next().ok_or_else(|| AppError::internal("no query vector"))?;
    Ok(Some(normalize_vector(&qv, Some(choice.dimensions as usize)).map_err(AppError::validation)?))
}

/// Ranks chunks in the scope (optionally restricted to some documents) for `query`.
pub fn search_in(
    svc: &Services,
    scope: &ResolvedScope,
    query: &str,
    qvec: Option<&[f32]>,
    restrict: Option<&[DocumentId]>,
    limit: usize,
) -> AppResult<Vec<Hit>> {
    if query.chars().count() > MAX_QUERY_CHARS {
        return Err(AppError::validation("query too long"));
    }
    let pick = |docs: &[SearchableDoc]| -> Vec<DocumentId> {
        docs.iter().filter(|d| restrict.is_none_or(|r| r.contains(&d.id))).map(|d| d.id.clone()).collect()
    };
    let lexical_docs = pick(&scope.lexical);
    let mut lexical_ids: Vec<i64> = Vec::new();
    if scope.summary.mode != RetrievalMode::Semantic {
        if let Some(q) = fts_query(query) {
            if !lexical_docs.is_empty() {
                lexical_ids = svc.index.lexical_search(&q, &lexical_docs, CANDIDATES_PER_PATH as u32)?.into_iter().map(|(id, _)| id).collect();
            }
        }
    }
    let mut semantic_ids: Vec<i64> = Vec::new();
    if let (Some((choice, _)), Some(qv)) = (&scope.embedder, qvec) {
        let semantic_docs = pick(&scope.semantic);
        if !semantic_docs.is_empty() {
            let vectors = svc.index.vectors(&semantic_docs, &choice.fingerprint)?;
            semantic_ids = top_k_by_similarity(qv, &vectors, CANDIDATES_PER_PATH);
        }
    }
    let fused = reciprocal_rank_fusion(&[&lexical_ids, &semantic_ids], RRF_K);
    let ids: Vec<i64> = fused.iter().map(|f| f.0).collect();
    let rows: HashMap<i64, ChunkRow> = svc.index.chunks_by_ids(&ids)?.into_iter().map(|c| (c.id, c)).collect();
    let hits: Vec<Hit> = fused
        .iter()
        .filter_map(|(id, score)| {
            rows.get(id).map(|c| Hit {
                chunk: c.clone(),
                score: *score,
                lexical_rank: lexical_ids.iter().position(|x| x == id).map(|p| p as u32 + 1),
                semantic_rank: semantic_ids.iter().position(|x| x == id).map(|p| p as u32 + 1),
            })
        })
        .collect();
    let mut capped = cap_per_document(&hits, |h| h.chunk.document_id.as_str(), MAX_PER_DOCUMENT);
    capped.truncate(limit);
    Ok(capped)
}

pub fn hit_view(scope: &ResolvedScope, h: &Hit) -> SearchHitView {
    let doc = scope.doc(&h.chunk.document_id);
    SearchHitView {
        chunk_id: h.chunk.id,
        document_id: h.chunk.document_id.clone(),
        title: doc.map(|d| d.title.clone()).unwrap_or_default(),
        authors: doc.and_then(|d| d.authors.clone()),
        year: doc.and_then(|d| d.year),
        page_index: h.chunk.page_index,
        page_offset: h.chunk.start,
        excerpt: h.chunk.text.chars().take(600).collect(),
        score: h.score,
        lexical_rank: h.lexical_rank,
        semantic_rank: h.semantic_rank,
    }
}

pub fn search(svc: &Services, query: &str, input: &ScopeInput, mode: RetrievalMode, limit: u32) -> AppResult<SearchResults> {
    let scope = resolve_scope(svc, input, mode)?;
    let qvec = embed_query(&scope, query.trim())?;
    let hits = search_in(svc, &scope, query.trim(), qvec.as_deref(), None, limit.clamp(1, 50) as usize)?;
    Ok(SearchResults { hits: hits.iter().map(|h| hit_view(&scope, h)).collect(), scope: scope.summary })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_k_orders_by_similarity() {
        let q = vec![1.0, 0.0];
        let vs = vec![(1, vec![0.0, 1.0]), (2, vec![1.0, 0.0]), (3, vec![0.7, 0.7]), (4, vec![0.9, 0.1])];
        assert_eq!(top_k_by_similarity(&q, &vs, 2), vec![2, 4]);
        assert_eq!(top_k_by_similarity(&q, &vs, 10), vec![2, 4, 3, 1]);
    }
}
