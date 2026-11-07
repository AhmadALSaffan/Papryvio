use rusqlite::types::ToSql;
use rusqlite::{params, params_from_iter, OptionalExtension};

use crate::application::error::AppResult;
use crate::application::ports::{ChunkRow, IndexRepository, NewChunk, SearchableDoc, SpanBox};
use crate::domain::ids::DocumentId;
use crate::domain::retrieval::{blob_to_vector, vector_to_blob};

use super::{db_err, SqliteStore};

fn placeholders(start: usize, n: usize) -> String {
    (start..start + n).map(|i| format!("?{i}")).collect::<Vec<_>>().join(",")
}

impl IndexRepository for SqliteStore {
    fn pages(&self, doc: &DocumentId) -> AppResult<Vec<(u32, String)>> {
        self.db.with(|c| {
            let mut st = c.prepare(
                "SELECT p.page_index, p.text FROM document_pages p JOIN documents d ON d.id = p.document_id
                 WHERE p.document_id = ?1 AND p.extraction_revision = d.extraction_revision ORDER BY p.page_index",
            )?;
            let rows = st.query_map(params![doc.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })
    }

    fn page_text(&self, doc: &DocumentId, page_index: u32) -> AppResult<Option<String>> {
        self.db.with(|c| {
            c.query_row(
                "SELECT p.text FROM document_pages p JOIN documents d ON d.id = p.document_id
                 WHERE p.document_id = ?1 AND p.extraction_revision = d.extraction_revision AND p.page_index = ?2",
                params![doc.as_str(), page_index],
                |r| r.get(0),
            )
            .optional()
        })
    }

    fn page_spans(&self, doc: &DocumentId, page_index: u32) -> AppResult<Vec<SpanBox>> {
        self.db.with(|c| {
            let mut st = c.prepare(
                "SELECT s.start_offset, s.end_offset, s.left, s.bottom, s.right, s.top
                 FROM text_spans s JOIN document_pages p ON p.id = s.page_id JOIN documents d ON d.id = p.document_id
                 WHERE p.document_id = ?1 AND p.extraction_revision = d.extraction_revision AND p.page_index = ?2
                 ORDER BY s.start_offset",
            )?;
            let rows = st.query_map(params![doc.as_str(), page_index], |r| {
                Ok(SpanBox {
                    start: r.get(0)?,
                    end: r.get(1)?,
                    left: r.get::<_, f64>(2)? as f32,
                    bottom: r.get::<_, f64>(3)? as f32,
                    right: r.get::<_, f64>(4)? as f32,
                    top: r.get::<_, f64>(5)? as f32,
                })
            })?;
            rows.collect()
        })
    }

    fn create_generation(&self, doc: &DocumentId, chunker_version: i64, chunks: &[NewChunk], now: i64) -> AppResult<String> {
        let id = uuid::Uuid::now_v7().to_string();
        self.db.tx(|tx| {
            let revision: i64 = tx
                .query_row("SELECT extraction_revision FROM documents WHERE id = ?1", params![doc.as_str()], |r| r.get(0))
                .map_err(db_err)?;
            tx.execute(
                "INSERT INTO index_generations (id, document_id, extraction_revision, chunker_version, state, chunk_count, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'building', ?5, ?6)",
                params![id, doc.as_str(), revision, chunker_version, chunks.len() as i64, now],
            )
            .map_err(db_err)?;
            let mut st = tx
                .prepare_cached(
                    "INSERT INTO chunks (generation_id, document_id, page_index, chunk_index, text, search_text, start_offset, end_offset, token_estimate)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                )
                .map_err(db_err)?;
            for c in chunks {
                st.execute(params![id, doc.as_str(), c.page_index, c.chunk_index, c.text, c.search_text, c.start, c.end, c.token_estimate])
                    .map_err(db_err)?;
            }
            Ok(())
        })?;
        Ok(id)
    }

    fn publish_generation(&self, doc: &DocumentId, generation: &str, now: i64) -> AppResult<()> {
        self.db.tx(|tx| {
            // Old generations and their chunks (FTS rows via trigger, vectors via cascade) go.
            tx.execute(
                "DELETE FROM chunks WHERE document_id = ?1 AND generation_id <> ?2",
                params![doc.as_str(), generation],
            )
            .map_err(db_err)?;
            tx.execute(
                "DELETE FROM index_generations WHERE document_id = ?1 AND id <> ?2",
                params![doc.as_str(), generation],
            )
            .map_err(db_err)?;
            tx.execute("UPDATE index_generations SET state = 'published' WHERE id = ?1", params![generation]).map_err(db_err)?;
            tx.execute(
                "UPDATE documents SET active_generation_id = ?2, lexical_ready = 1, semantic_ready = 0, embedding_fingerprint = NULL,
                    extraction_state = 'ready', updated_at = ?3
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![doc.as_str(), generation, now],
            )
            .map_err(db_err)?;
            Ok(())
        })
    }

    fn active_generation(&self, doc: &DocumentId) -> AppResult<Option<String>> {
        self.db.with(|c| {
            c.query_row(
                "SELECT active_generation_id FROM documents WHERE id = ?1 AND deleted_at IS NULL",
                params![doc.as_str()],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()
            .map(Option::flatten)
        })
    }

    fn embedding_progress(&self, generation: &str, fingerprint: &str) -> AppResult<(u32, u32)> {
        self.db.with(|c| {
            c.query_row(
                "SELECT COUNT(*), COUNT(e.chunk_id) FROM chunks ch
                 LEFT JOIN chunk_embeddings e ON e.chunk_id = ch.id AND e.fingerprint = ?2
                 WHERE ch.generation_id = ?1",
                params![generation, fingerprint],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
    }

    fn chunks_missing_embedding(&self, generation: &str, fingerprint: &str, limit: u32) -> AppResult<Vec<(i64, String)>> {
        self.db.with(|c| {
            let mut st = c.prepare(
                "SELECT ch.id, ch.text FROM chunks ch
                 WHERE ch.generation_id = ?1
                   AND NOT EXISTS (SELECT 1 FROM chunk_embeddings e WHERE e.chunk_id = ch.id AND e.fingerprint = ?2)
                 ORDER BY ch.id LIMIT ?3",
            )?;
            let rows = st.query_map(params![generation, fingerprint, limit], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })
    }

    fn store_embeddings(&self, fingerprint: &str, dims: u32, items: &[(i64, Vec<f32>)]) -> AppResult<()> {
        self.db.tx(|tx| {
            let mut st = tx
                .prepare_cached("INSERT OR REPLACE INTO chunk_embeddings (chunk_id, fingerprint, dimensions, vector) VALUES (?1, ?2, ?3, ?4)")
                .map_err(db_err)?;
            for (id, v) in items {
                st.execute(params![id, fingerprint, dims, vector_to_blob(v)]).map_err(db_err)?;
            }
            Ok(())
        })
    }

    fn mark_semantic_ready(&self, doc: &DocumentId, generation: &str, fingerprint: &str, now: i64) -> AppResult<bool> {
        self.db
            .with(|c| {
                c.execute(
                    "UPDATE documents SET semantic_ready = 1, embedding_fingerprint = ?3, updated_at = ?4
                     WHERE id = ?1 AND active_generation_id = ?2 AND deleted_at IS NULL",
                    params![doc.as_str(), generation, fingerprint, now],
                )
            })
            .map(|n| n > 0)
    }

    fn reset_semantic(&self, keep_fingerprint: Option<&str>) -> AppResult<()> {
        self.db.tx(|tx| {
            tx.execute("UPDATE documents SET semantic_ready = 0, embedding_fingerprint = NULL", []).map_err(db_err)?;
            match keep_fingerprint {
                Some(fp) => tx.execute("DELETE FROM chunk_embeddings WHERE fingerprint <> ?1", params![fp]),
                None => tx.execute("DELETE FROM chunk_embeddings", []),
            }
            .map_err(db_err)?;
            Ok(())
        })
    }

    fn searchable_docs(&self, ids: &[DocumentId]) -> AppResult<Vec<SearchableDoc>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT id, title, authors, year, page_count, active_generation_id, lexical_ready, embedding_fingerprint, extraction_state
             FROM documents WHERE deleted_at IS NULL AND id IN ({}) ORDER BY title",
            placeholders(1, ids.len())
        );
        self.db.with(|c| {
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(params_from_iter(ids.iter().map(|i| i.as_str())), |r| {
                Ok(SearchableDoc {
                    id: DocumentId::from_trusted(r.get(0)?),
                    title: r.get(1)?,
                    authors: r.get(2)?,
                    year: r.get(3)?,
                    page_count: r.get(4)?,
                    generation_id: r.get(5)?,
                    lexical_ready: r.get::<_, i64>(6)? != 0,
                    semantic_fingerprint: r.get(7)?,
                    state: r.get(8)?,
                })
            })?;
            rows.collect()
        })
    }

    fn lexical_search(&self, fts_query: &str, docs: &[DocumentId], limit: u32) -> AppResult<Vec<(i64, f64)>> {
        if docs.is_empty() {
            return Ok(Vec::new());
        }
        // bm25() is lower-is-better in SQLite FTS5.
        let sql = format!(
            "SELECT c.id, bm25(chunks_fts) AS score FROM chunks_fts
             JOIN chunks c ON c.id = chunks_fts.rowid
             JOIN documents d ON d.id = c.document_id AND d.active_generation_id = c.generation_id
             WHERE chunks_fts MATCH ?1 AND d.deleted_at IS NULL AND c.document_id IN ({})
             ORDER BY score ASC, c.id ASC LIMIT {}",
            placeholders(2, docs.len()),
            limit
        );
        let mut args: Vec<&dyn ToSql> = vec![&fts_query];
        let ids: Vec<&str> = docs.iter().map(|d| d.as_str()).collect();
        for i in &ids {
            args.push(i);
        }
        self.db.with(|c| {
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(params_from_iter(args.iter()), |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })
    }

    fn vectors(&self, docs: &[DocumentId], fingerprint: &str) -> AppResult<Vec<(i64, Vec<f32>)>> {
        if docs.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT e.chunk_id, e.vector FROM chunk_embeddings e
             JOIN chunks c ON c.id = e.chunk_id
             JOIN documents d ON d.id = c.document_id AND d.active_generation_id = c.generation_id
             WHERE e.fingerprint = ?1 AND d.deleted_at IS NULL AND c.document_id IN ({})",
            placeholders(2, docs.len())
        );
        let mut args: Vec<&dyn ToSql> = vec![&fingerprint];
        let ids: Vec<&str> = docs.iter().map(|d| d.as_str()).collect();
        for i in &ids {
            args.push(i);
        }
        self.db.with(|c| {
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(params_from_iter(args.iter()), |r| Ok((r.get(0)?, blob_to_vector(&r.get::<_, Vec<u8>>(1)?))))?;
            rows.collect()
        })
    }

    fn chunks_by_ids(&self, ids: &[i64]) -> AppResult<Vec<ChunkRow>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT id, document_id, generation_id, page_index, text, start_offset, end_offset FROM chunks WHERE id IN ({})",
            placeholders(1, ids.len())
        );
        self.db.with(|c| {
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(params_from_iter(ids.iter()), |r| {
                Ok(ChunkRow {
                    id: r.get(0)?,
                    document_id: DocumentId::from_trusted(r.get(1)?),
                    generation_id: r.get(2)?,
                    page_index: r.get(3)?,
                    text: r.get(4)?,
                    start: r.get(5)?,
                    end: r.get(6)?,
                })
            })?;
            rows.collect()
        })
    }

    fn chunks_for_page(&self, generation_id: &str, page_index: u32) -> AppResult<Vec<ChunkRow>> {
        self.db.with(|c| {
            let mut st = c.prepare(
                "SELECT id, document_id, generation_id, page_index, text, start_offset, end_offset FROM chunks
                 WHERE generation_id = ?1 AND page_index = ?2 ORDER BY chunk_index",
            )?;
            let rows = st.query_map(params![generation_id, page_index], |r| {
                Ok(ChunkRow {
                    id: r.get(0)?,
                    document_id: DocumentId::from_trusted(r.get(1)?),
                    generation_id: r.get(2)?,
                    page_index: r.get(3)?,
                    text: r.get(4)?,
                    start: r.get(5)?,
                    end: r.get(6)?,
                })
            })?;
            rows.collect()
        })
    }

    fn indexed_documents(&self) -> AppResult<Vec<DocumentId>> {
        self.db.with(|c| {
            let mut st = c.prepare("SELECT id FROM documents WHERE deleted_at IS NULL AND active_generation_id IS NOT NULL")?;
            let rows = st.query_map([], |r| Ok(DocumentId::from_trusted(r.get(0)?)))?;
            rows.collect()
        })
    }

    fn fts_consistent(&self) -> AppResult<bool> {
        self.db.with(|c| {
            c.execute("INSERT INTO chunks_fts(chunks_fts, rank) VALUES ('integrity-check', 1)", [])?;
            let fts: i64 = c.query_row("SELECT COUNT(*) FROM chunks_fts", [], |r| r.get(0))?;
            let rows: i64 = c.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
            Ok(fts == rows)
        })
    }
}
