use rusqlite::{params, Connection, OptionalExtension};

use crate::application::error::AppResult;
use crate::application::ports::{AnswerRepository, CellUpdate, CitationRecord, NewAttempt};
use crate::application::views::{
    AttemptView, CellEvidence, ComparisonCellView, ComparisonDocView, ComparisonRow, ComparisonView, ConversationSummary,
    ConversationView, ScopeInput, ScopeSummary, SourceView, TurnView,
};
use crate::domain::evidence::{CitationRef, Evidence, ValidatedAnswer};
use crate::domain::ids::DocumentId;

use super::{db_err, SqliteStore};

fn attempt_view(c: &Connection, id: &str) -> rusqlite::Result<Option<AttemptView>> {
    let row = c
        .query_row(
            "SELECT id, state, result, scope_summary, error_kind, model_fingerprint, started_at, finished_at FROM generation_attempts WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                ))
            },
        )
        .optional()?;
    let Some((id, state, result, scope, error_kind, model, started_at, finished_at)) = row else { return Ok(None) };
    let mut st = c.prepare_cached(
        "SELECT e.evidence_id, e.document_id, e.document_title, e.page_index, e.excerpt_snapshot,
                (d.id IS NULL OR d.deleted_at IS NOT NULL)
         FROM retrieved_evidence e LEFT JOIN documents d ON d.id = e.document_id
         WHERE e.attempt_id = ?1 ORDER BY e.rank",
    )?;
    let sources = st
        .query_map(params![id], |r| {
            Ok(SourceView {
                evidence_id: r.get(0)?,
                document_id: DocumentId::from_trusted(r.get(1)?),
                title: r.get(2)?,
                page_index: r.get(3)?,
                excerpt: r.get(4)?,
                deleted: r.get::<_, i64>(5)? != 0,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(AttemptView {
        id,
        state,
        answer: result.and_then(|r| serde_json::from_str(&r).ok()),
        sources,
        scope: scope.and_then(|s| serde_json::from_str(&s).ok()),
        error_kind,
        model: model.split('|').nth(1).unwrap_or(&model).to_string(),
        started_at,
        finished_at,
    }))
}

fn comparison_view(c: &Connection, id: &str) -> rusqlite::Result<Option<ComparisonView>> {
    let row = c
        .query_row(
            "SELECT id, state, selected_documents, selected_rows, created_at FROM comparisons WHERE id = ?1",
            params![id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, i64>(4)?)),
        )
        .optional()?;
    let Some((id, state, docs_json, rows_json, created_at)) = row else { return Ok(None) };
    let doc_ids: Vec<String> = serde_json::from_str(&docs_json).unwrap_or_default();
    let rows: Vec<ComparisonRow> = serde_json::from_str(&rows_json).unwrap_or_default();
    let mut documents = Vec::new();
    for d in &doc_ids {
        let v = c
            .query_row(
                "SELECT title, authors, year, deleted_at IS NOT NULL FROM documents WHERE id = ?1",
                params![d],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<i32>>(2)?, r.get::<_, i64>(3)? != 0)),
            )
            .optional()?;
        let (title, authors, year, deleted) = v.unwrap_or_else(|| (String::new(), None, None, true));
        documents.push(ComparisonDocView { id: DocumentId::from_trusted(d.clone()), title, authors, year, deleted });
    }
    let mut st = c.prepare_cached(
        "SELECT id, document_id, row_key, status, text, evidence, searched FROM comparison_cells WHERE comparison_id = ?1",
    )?;
    let cells = st
        .query_map(params![id], |r| {
            Ok(ComparisonCellView {
                id: r.get(0)?,
                document_id: DocumentId::from_trusted(r.get(1)?),
                row_key: r.get(2)?,
                status: r.get(3)?,
                text: r.get(4)?,
                evidence: serde_json::from_str::<Vec<CellEvidence>>(&r.get::<_, String>(5)?).unwrap_or_default(),
                searched: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let completed = cells.iter().filter(|c| c.status != "pending").count() as u32;
    let linked = cells.iter().filter(|c| matches!(c.status.as_str(), "supported" | "partial") && !c.evidence.is_empty()).count() as u32;
    Ok(Some(ComparisonView { id, state, documents, rows, cells, linked_cells: linked, completed_cells: completed, created_at }))
}

impl AnswerRepository for SqliteStore {
    fn create_conversation(&self, id: &str, title: &str, collection: Option<&str>, now: i64) -> AppResult<()> {
        self.db
            .with(|c| {
                c.execute(
                    "INSERT INTO conversations (id, title, collection_id, created_at, updated_at)
                     VALUES (?1, ?2, (SELECT id FROM collections WHERE id = ?3), ?4, ?4)",
                    params![id, title, collection, now],
                )
            })
            .map(|_| ())
    }

    fn conversation_exists(&self, id: &str) -> AppResult<bool> {
        self.db.with(|c| c.query_row("SELECT 1 FROM conversations WHERE id = ?1", params![id], |_| Ok(())).optional().map(|o| o.is_some()))
    }

    fn list_conversations(&self, limit: u32) -> AppResult<Vec<ConversationSummary>> {
        self.db.with(|c| {
            let mut st = c.prepare("SELECT id, title, collection_id, created_at, updated_at FROM conversations ORDER BY updated_at DESC LIMIT ?1")?;
            let rows = st.query_map(params![limit], |r| {
                Ok(ConversationSummary { id: r.get(0)?, title: r.get(1)?, collection_id: r.get(2)?, created_at: r.get(3)?, updated_at: r.get(4)? })
            })?;
            rows.collect()
        })
    }

    fn conversation(&self, id: &str) -> AppResult<Option<ConversationView>> {
        self.db.with(|c| {
            let head = c
                .query_row("SELECT id, title, collection_id, created_at FROM conversations WHERE id = ?1", params![id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)?))
                })
                .optional()?;
            let Some((id, title, collection_id, created_at)) = head else { return Ok(None) };
            let mut st = c.prepare("SELECT id, text, created_at FROM messages WHERE conversation_id = ?1 AND role = 'user' ORDER BY created_at, id")?;
            let msgs = st
                .query_map(params![id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut turns = Vec::new();
            for (mid, text, at) in msgs {
                let (latest, count): (Option<String>, u32) = c.query_row(
                    "SELECT (SELECT id FROM generation_attempts WHERE message_id = ?1 ORDER BY started_at DESC, id DESC LIMIT 1),
                            (SELECT COUNT(*) FROM generation_attempts WHERE message_id = ?1)",
                    params![mid],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                let attempt = match latest {
                    Some(a) => attempt_view(c, &a)?,
                    None => None,
                };
                turns.push(TurnView { message_id: mid, question: text, asked_at: at, attempt, attempt_count: count });
            }
            Ok(Some(ConversationView { id, title, collection_id, created_at, turns }))
        })
    }

    fn delete_conversation(&self, id: &str) -> AppResult<()> {
        self.db.with(|c| c.execute("DELETE FROM conversations WHERE id = ?1", params![id])).map(|_| ())
    }

    fn add_message(&self, id: &str, conversation: &str, role: &str, text: &str, state: &str, now: i64) -> AppResult<()> {
        self.db.tx(|tx| {
            tx.execute(
                "INSERT INTO messages (id, conversation_id, role, text, state, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, conversation, role, text, state, now],
            )
            .map_err(db_err)?;
            tx.execute("UPDATE conversations SET updated_at = ?2 WHERE id = ?1", params![conversation, now]).map_err(db_err)?;
            Ok(())
        })
    }

    fn message_question(&self, message_id: &str) -> AppResult<Option<(String, String)>> {
        self.db.with(|c| {
            c.query_row("SELECT conversation_id, text FROM messages WHERE id = ?1", params![message_id], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()
        })
    }

    fn find_attempt_by_request(&self, request_id: &str) -> AppResult<Option<(String, String)>> {
        self.db.with(|c| {
            c.query_row(
                "SELECT a.id, m.conversation_id FROM generation_attempts a JOIN messages m ON m.id = a.message_id WHERE a.request_id = ?1",
                params![request_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
        })
    }

    fn create_attempt(&self, a: &NewAttempt) -> AppResult<()> {
        self.db
            .with(|c| {
                c.execute(
                    "INSERT INTO generation_attempts (id, message_id, request_id, model_fingerprint, prompt_version, scope_snapshot,
                        retrieval_config, state, started_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'retrieving', ?8)",
                    params![a.id, a.message_id, a.request_id, a.model_fingerprint, a.prompt_version, a.scope_snapshot, a.retrieval_config, a.now],
                )
            })
            .map(|_| ())
    }

    fn set_attempt_state(&self, id: &str, state: &str) -> AppResult<()> {
        self.db.with(|c| c.execute("UPDATE generation_attempts SET state = ?2 WHERE id = ?1", params![id, state])).map(|_| ())
    }

    fn set_attempt_scope(&self, id: &str, summary: &ScopeSummary) -> AppResult<()> {
        let json = serde_json::to_string(summary).unwrap_or_default();
        self.db.with(|c| c.execute("UPDATE generation_attempts SET scope_summary = ?2 WHERE id = ?1", params![id, json])).map(|_| ())
    }

    fn attempt_inputs(&self, id: &str) -> AppResult<Option<(String, ScopeInput)>> {
        self.db.with(|c| {
            c.query_row("SELECT message_id, scope_snapshot FROM generation_attempts WHERE id = ?1", params![id], |r| {
                Ok((r.get::<_, String>(0)?, serde_json::from_str(&r.get::<_, String>(1)?).unwrap_or_default()))
            })
            .optional()
        })
    }

    fn attempt_scope(&self, id: &str) -> AppResult<Option<ScopeInput>> {
        Ok(self.attempt_inputs(id)?.map(|(_, s)| s))
    }

    fn finish_attempt(&self, id: &str, state: &str, result: Option<&ValidatedAnswer>, error_kind: Option<&str>, timing: &str, now: i64) -> AppResult<()> {
        let result = result.map(|r| serde_json::to_string(r).unwrap_or_default());
        self.db
            .with(|c| {
                c.execute(
                    "UPDATE generation_attempts SET state = ?2, result = ?3, error_kind = ?4, timing = ?5, finished_at = ?6
                     WHERE id = ?1 AND state NOT IN ('answered','partial','no_evidence','failed','cancelled')",
                    params![id, state, result, error_kind, timing, now],
                )
            })
            .map(|_| ())
    }

    fn save_evidence(&self, attempt: &str, bundle: &[Evidence]) -> AppResult<()> {
        self.db.tx(|tx| {
            tx.execute("DELETE FROM retrieved_evidence WHERE attempt_id = ?1", params![attempt]).map_err(db_err)?;
            for (rank, e) in bundle.iter().enumerate() {
                tx.execute(
                    "INSERT INTO retrieved_evidence (attempt_id, evidence_id, chunk_id, document_id, generation_id, rank, page_index,
                        start_offset, end_offset, document_title, excerpt_snapshot)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![
                        attempt,
                        e.id,
                        e.chunk_id,
                        e.document_id.as_str(),
                        e.generation_id,
                        rank as i64,
                        e.page_index,
                        e.page_offset,
                        e.page_offset + e.text.chars().count() as u32,
                        e.title,
                        e.text
                    ],
                )
                .map_err(db_err)?;
            }
            Ok(())
        })
    }

    fn save_citations(&self, attempt: &str, answer: &mut ValidatedAnswer) -> AppResult<()> {
        self.db.tx(|tx| {
            for (ci, claim) in answer.claims.iter_mut().enumerate() {
                for cite in claim.citations.iter_mut() {
                    let id = uuid::Uuid::now_v7().to_string();
                    tx.execute(
                        "INSERT INTO citations (id, attempt_id, claim_index, evidence_id, document_id, page_index, span_start, span_end, quote)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        params![
                            id,
                            attempt,
                            ci as i64,
                            cite.evidence_id,
                            cite.document_id.as_str(),
                            cite.page_index,
                            cite.span.map(|s| s.0),
                            cite.span.map(|s| s.1),
                            cite.quote
                        ],
                    )
                    .map_err(db_err)?;
                    cite.citation_id = Some(id);
                }
            }
            Ok(())
        })
    }

    fn citation(&self, id: &str) -> AppResult<Option<CitationRecord>> {
        self.db.with(|c| {
            c.query_row(
                "SELECT ci.id, ci.attempt_id, ci.document_id, ci.page_index, ci.span_start, ci.span_end, ci.quote, COALESCE(e.excerpt_snapshot, '')
                 FROM citations ci LEFT JOIN retrieved_evidence e ON e.attempt_id = ci.attempt_id AND e.evidence_id = ci.evidence_id
                 WHERE ci.id = ?1",
                params![id],
                |r| {
                    let s: Option<u32> = r.get(4)?;
                    let e: Option<u32> = r.get(5)?;
                    Ok(CitationRecord {
                        id: r.get(0)?,
                        attempt_id: r.get(1)?,
                        document_id: DocumentId::from_trusted(r.get(2)?),
                        page_index: r.get(3)?,
                        span: s.zip(e),
                        quote: r.get(6)?,
                        excerpt: r.get(7)?,
                    })
                },
            )
            .optional()
        })
    }

    fn attempts_running(&self) -> AppResult<Vec<String>> {
        self.db.with(|c| {
            let mut st = c.prepare("SELECT id FROM generation_attempts WHERE state IN ('retrieving','generating','validating')")?;
            let rows = st.query_map([], |r| r.get(0))?;
            rows.collect()
        })
    }

    fn create_comparison(&self, id: &str, request_id: &str, docs: &[DocumentId], rows_json: &str, model_fp: &str, cells: &[(String, DocumentId, String, String)], now: i64) -> AppResult<()> {
        let docs_json = serde_json::to_string(&docs.iter().map(|d| d.as_str()).collect::<Vec<_>>()).unwrap();
        self.db.tx(|tx| {
            tx.execute(
                "INSERT INTO comparisons (id, request_id, selected_documents, selected_rows, model_fingerprint, state, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6, ?6)",
                params![id, request_id, docs_json, rows_json, model_fp, now],
            )
            .map_err(db_err)?;
            for (cell_id, doc, key, label) in cells {
                tx.execute(
                    "INSERT INTO comparison_cells (id, comparison_id, document_id, row_key, row_label, status, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6)",
                    params![cell_id, id, doc.as_str(), key, label, now],
                )
                .map_err(db_err)?;
            }
            Ok(())
        })
    }

    fn find_comparison_by_request(&self, request_id: &str) -> AppResult<Option<String>> {
        self.db.with(|c| c.query_row("SELECT id FROM comparisons WHERE request_id = ?1", params![request_id], |r| r.get(0)).optional())
    }

    fn update_cell(&self, u: &CellUpdate) -> AppResult<()> {
        self.db
            .with(|c| {
                c.execute(
                    "UPDATE comparison_cells SET status = ?2, text = ?3, evidence = ?4, searched = ?5, updated_at = ?6 WHERE id = ?1",
                    params![u.id, u.status, u.text, u.evidence_json, u.searched, u.now],
                )
            })
            .map(|_| ())
    }

    fn set_comparison_state(&self, id: &str, state: &str, now: i64) -> AppResult<()> {
        self.db
            .with(|c| c.execute("UPDATE comparisons SET state = ?2, updated_at = ?3 WHERE id = ?1", params![id, state, now]))
            .map(|_| ())
    }

    fn comparison(&self, id: &str) -> AppResult<Option<ComparisonView>> {
        self.db.with(|c| comparison_view(c, id))
    }

    fn latest_comparisons(&self, limit: u32) -> AppResult<Vec<ComparisonView>> {
        self.db.with(|c| {
            let mut st = c.prepare("SELECT id FROM comparisons ORDER BY created_at DESC LIMIT ?1")?;
            let ids = st.query_map(params![limit], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let mut out = Vec::new();
            for id in ids {
                if let Some(v) = comparison_view(c, &id)? {
                    out.push(v);
                }
            }
            Ok(out)
        })
    }

    fn cell_citation(&self, cell_id: &str, number: u32) -> AppResult<Option<(CitationRef, String)>> {
        let ev: Option<String> = self.db.with(|c| {
            c.query_row("SELECT evidence FROM comparison_cells WHERE id = ?1", params![cell_id], |r| r.get(0)).optional()
        })?;
        let Some(ev) = ev else { return Ok(None) };
        let items: Vec<CellEvidence> = serde_json::from_str(&ev).unwrap_or_default();
        Ok(items.into_iter().find(|e| e.number == number).map(|e| {
            (
                CitationRef {
                    number: e.number,
                    evidence_id: String::new(),
                    document_id: e.document_id,
                    page_index: e.page_index,
                    span: e.span,
                    quote: e.quote,
                    citation_id: None,
                },
                e.excerpt,
            )
        }))
    }
}
