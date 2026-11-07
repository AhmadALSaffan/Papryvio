use std::collections::HashSet;

use rusqlite::types::ToSql;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};

use crate::application::error::{AppError, AppResult};
use crate::application::ports::{
    DocumentRecord, DocumentRepository, ExtractedMetadata, NewDocument, NewJob, UserMetadata,
};
use crate::application::views::{DocumentListPage, DocumentQuery, DocumentView};
use crate::domain::document::{
    plausible_authors, plausible_metadata_title, year_from_pdf_date, ExtractionState, FailureKind, MetadataOrigin,
};
use crate::domain::ids::{CollectionId, DocumentId};
use crate::domain::job::JobKind;

use super::jobs::job_for_entity;
use super::{db_err, SqliteStore};

const RECORD_COLUMNS: &str = "id, managed_file, original_file_name, title_origin, authors_origin, year_origin, \
     page_count, extraction_state, extraction_revision, deleted_at";

fn map_record(r: &Row<'_>) -> rusqlite::Result<DocumentRecord> {
    let state: String = r.get(7)?;
    Ok(DocumentRecord {
        id: DocumentId::from_trusted(r.get(0)?),
        managed_file: r.get(1)?,
        original_file_name: r.get(2)?,
        title_origin: MetadataOrigin::parse(&r.get::<_, String>(3)?).unwrap_or(MetadataOrigin::FileName),
        authors_origin: r.get::<_, Option<String>>(4)?.and_then(|s| MetadataOrigin::parse(&s)),
        year_origin: r.get::<_, Option<String>>(5)?.and_then(|s| MetadataOrigin::parse(&s)),
        page_count: r.get(6)?,
        state: ExtractionState::parse(&state).unwrap_or(ExtractionState::Failed),
        extraction_revision: r.get(8)?,
        deleted_at: r.get(9)?,
    })
}

const VIEW_COLUMNS: &str = "d.id, d.title, d.title_origin, d.authors, d.authors_origin, d.year, d.year_origin, \
     d.page_count, d.extraction_state, d.failure_kind, d.text_page_count, d.low_text_page_count, \
     d.lexical_ready, d.semantic_ready, d.original_file_name, d.file_size, d.created_at";

fn map_view(r: &Row<'_>) -> rusqlite::Result<DocumentView> {
    let state: String = r.get(8)?;
    Ok(DocumentView {
        id: DocumentId::from_trusted(r.get(0)?),
        title: r.get(1)?,
        title_origin: MetadataOrigin::parse(&r.get::<_, String>(2)?).unwrap_or(MetadataOrigin::FileName),
        authors: r.get(3)?,
        authors_origin: r.get::<_, Option<String>>(4)?.and_then(|s| MetadataOrigin::parse(&s)),
        year: r.get(5)?,
        year_origin: r.get::<_, Option<String>>(6)?.and_then(|s| MetadataOrigin::parse(&s)),
        page_count: r.get(7)?,
        state: ExtractionState::parse(&state).unwrap_or(ExtractionState::Failed),
        failure_kind: r.get::<_, Option<String>>(9)?.map(|s| FailureKind::parse(&s)),
        text_page_count: r.get(10)?,
        low_text_page_count: r.get(11)?,
        lexical_ready: r.get::<_, i64>(12)? != 0,
        semantic_ready: r.get::<_, i64>(13)? != 0,
        original_file_name: r.get(14)?,
        file_size: r.get::<_, i64>(15)? as u64,
        created_at: r.get(16)?,
        collection_ids: Vec::new(),
        job: None,
    })
}

fn enrich(conn: &Connection, view: &mut DocumentView) -> rusqlite::Result<()> {
    let mut st = conn.prepare_cached("SELECT collection_id FROM collection_documents WHERE document_id = ?1 ORDER BY added_at")?;
    view.collection_ids = st
        .query_map(params![view.id.as_str()], |r| Ok(CollectionId::from_trusted(r.get(0)?)))?
        .collect::<rusqlite::Result<_>>()?;
    view.job = job_for_entity(conn, view.id.as_str(), JobKind::Extract)?.map(|j| j.view());
    Ok(())
}

fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

impl DocumentRepository for SqliteStore {
    fn find_live_by_hash(&self, content_hash: &str) -> AppResult<Option<DocumentId>> {
        self.db.with(|c| {
            c.query_row(
                "SELECT id FROM documents WHERE content_hash = ?1 AND deleted_at IS NULL",
                params![content_hash],
                |r| Ok(DocumentId::from_trusted(r.get(0)?)),
            )
            .optional()
        })
    }

    fn register_import(&self, doc: &NewDocument, collection: &CollectionId, job: &NewJob) -> AppResult<()> {
        self.db.tx(|tx| {
            tx.execute(
                "INSERT INTO documents (id, content_hash, managed_file, original_file_name, file_size, title, title_origin,
                    extraction_state, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'file_name', 'imported', ?7, ?7)",
                params![doc.id.as_str(), doc.content_hash, doc.managed_file, doc.original_file_name, doc.file_size as i64, doc.title, doc.now],
            )
            .map_err(db_err)?;
            tx.execute(
                "INSERT INTO collection_documents (collection_id, document_id, added_at) VALUES (?1, ?2, ?3)",
                params![collection.as_str(), doc.id.as_str(), doc.now],
            )
            .map_err(db_err)?;
            super::jobs::insert_job(tx, job).map_err(db_err)?;
            Ok(())
        })
    }

    fn link(&self, collection: &CollectionId, doc: &DocumentId, now: i64) -> AppResult<bool> {
        self.db
            .with(|c| {
                c.execute(
                    "INSERT OR IGNORE INTO collection_documents (collection_id, document_id, added_at) VALUES (?1, ?2, ?3)",
                    params![collection.as_str(), doc.as_str(), now],
                )
            })
            .map(|n| n > 0)
    }

    fn unlink(&self, collection: &CollectionId, doc: &DocumentId) -> AppResult<bool> {
        self.db
            .with(|c| {
                c.execute(
                    "DELETE FROM collection_documents WHERE collection_id = ?1 AND document_id = ?2",
                    params![collection.as_str(), doc.as_str()],
                )
            })
            .map(|n| n > 0)
    }

    fn get(&self, id: &DocumentId) -> AppResult<Option<DocumentRecord>> {
        self.db.with(|c| {
            c.query_row(&format!("SELECT {RECORD_COLUMNS} FROM documents WHERE id = ?1"), params![id.as_str()], map_record)
                .optional()
        })
    }

    fn view(&self, id: &DocumentId) -> AppResult<Option<DocumentView>> {
        self.db.with(|c| {
            let v = c
                .query_row(
                    &format!("SELECT {VIEW_COLUMNS} FROM documents d WHERE d.id = ?1 AND d.deleted_at IS NULL"),
                    params![id.as_str()],
                    map_view,
                )
                .optional()?;
            match v {
                Some(mut v) => {
                    enrich(c, &mut v)?;
                    Ok(Some(v))
                }
                None => Ok(None),
            }
        })
    }

    fn list_views(&self, q: &DocumentQuery) -> AppResult<DocumentListPage> {
        let limit = q.limit.unwrap_or(200).clamp(1, 500);
        let offset = q.cursor.unwrap_or(0);
        let mut where_sql = String::from("d.deleted_at IS NULL");
        let mut args: Vec<Box<dyn ToSql>> = Vec::new();
        if let Some(cid) = &q.collection_id {
            args.push(Box::new(cid.as_str().to_string()));
            where_sql.push_str(&format!(
                " AND d.id IN (SELECT document_id FROM collection_documents WHERE collection_id = ?{})",
                args.len()
            ));
        }
        if let Some(f) = q.filter.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
            args.push(Box::new(format!("%{}%", escape_like(&f.to_lowercase()))));
            let n = args.len();
            where_sql.push_str(&format!(
                " AND (lower(d.title) LIKE ?{n} ESCAPE '\\' OR lower(COALESCE(d.authors,'')) LIKE ?{n} ESCAPE '\\' \
                   OR lower(d.original_file_name) LIKE ?{n} ESCAPE '\\')"
            ));
        }
        self.db.with(|c| {
            let total: u32 = c.query_row(
                &format!("SELECT COUNT(*) FROM documents d WHERE {where_sql}"),
                params_from_iter(args.iter().map(|a| a.as_ref())),
                |r| r.get(0),
            )?;
            let sql = format!(
                "SELECT {VIEW_COLUMNS} FROM documents d WHERE {where_sql} ORDER BY d.created_at DESC, d.id LIMIT {limit} OFFSET {offset}"
            );
            let mut st = c.prepare(&sql)?;
            let mut items: Vec<DocumentView> =
                st.query_map(params_from_iter(args.iter().map(|a| a.as_ref())), map_view)?.collect::<rusqlite::Result<_>>()?;
            for v in &mut items {
                enrich(c, v)?;
            }
            let next = offset + items.len() as u32;
            Ok(DocumentListPage { items, total, next_cursor: (next < total).then_some(next) })
        })
    }

    fn list_by_states(&self, states: &[ExtractionState]) -> AppResult<Vec<DocumentRecord>> {
        if states.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = (1..=states.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(",");
        self.db.with(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {RECORD_COLUMNS} FROM documents WHERE extraction_state IN ({placeholders})"
            ))?;
            let rows = st.query_map(params_from_iter(states.iter().map(|s| s.as_str())), map_record)?;
            rows.collect()
        })
    }

    fn all_ids(&self) -> AppResult<HashSet<String>> {
        self.db.with(|c| {
            let mut st = c.prepare("SELECT id FROM documents")?;
            let rows = st.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect()
        })
    }

    fn count_live(&self) -> AppResult<u32> {
        self.db.with(|c| c.query_row("SELECT COUNT(*) FROM documents WHERE deleted_at IS NULL", [], |r| r.get(0)))
    }

    fn set_state(&self, id: &DocumentId, state: ExtractionState, failure: Option<FailureKind>, now: i64) -> AppResult<()> {
        self.db
            .with(|c| {
                c.execute(
                    "UPDATE documents SET extraction_state = ?2, failure_kind = ?3, updated_at = ?4 WHERE id = ?1 AND deleted_at IS NULL",
                    params![id.as_str(), state.as_str(), failure.map(|f| f.as_str()), now],
                )
            })
            .map(|_| ())
    }

    fn apply_extracted_metadata(&self, id: &DocumentId, meta: &ExtractedMetadata, now: i64) -> AppResult<()> {
        let title = plausible_metadata_title(meta.title.as_deref());
        let authors = plausible_authors(meta.author.as_deref());
        let year = year_from_pdf_date(meta.creation_date.as_deref());
        self.db.tx(|tx| {
            tx.execute(
                "UPDATE documents SET page_count = ?2, updated_at = ?3 WHERE id = ?1",
                params![id.as_str(), meta.page_count, now],
            )
            .map_err(db_err)?;
            if let Some(t) = title {
                tx.execute(
                    "UPDATE documents SET title = ?2, title_origin = 'pdf_metadata' WHERE id = ?1 AND title_origin <> 'user'",
                    params![id.as_str(), t],
                )
                .map_err(db_err)?;
            }
            if let Some(a) = authors {
                tx.execute(
                    "UPDATE documents SET authors = ?2, authors_origin = 'pdf_metadata'
                     WHERE id = ?1 AND (authors_origin IS NULL OR authors_origin <> 'user')",
                    params![id.as_str(), a],
                )
                .map_err(db_err)?;
            }
            if let Some(y) = year {
                tx.execute(
                    "UPDATE documents SET year = ?2, year_origin = 'pdf_metadata'
                     WHERE id = ?1 AND (year_origin IS NULL OR year_origin <> 'user')",
                    params![id.as_str(), y],
                )
                .map_err(db_err)?;
            }
            Ok(())
        })
    }

    fn update_user_metadata(&self, id: &DocumentId, u: &UserMetadata, now: i64) -> AppResult<()> {
        self.db.tx(|tx| {
            if let Some(t) = &u.title {
                tx.execute("UPDATE documents SET title = ?2, title_origin = 'user' WHERE id = ?1", params![id.as_str(), t])
                    .map_err(db_err)?;
            }
            if let Some(a) = &u.authors {
                tx.execute(
                    "UPDATE documents SET authors = ?2, authors_origin = CASE WHEN ?2 IS NULL THEN NULL ELSE 'user' END WHERE id = ?1",
                    params![id.as_str(), a],
                )
                .map_err(db_err)?;
                // An explicit clear is still a user decision: remember it so PDF metadata does not refill it.
                if a.is_none() {
                    tx.execute("UPDATE documents SET authors_origin = 'user' WHERE id = ?1", params![id.as_str()]).map_err(db_err)?;
                }
            }
            if let Some(y) = &u.year {
                tx.execute("UPDATE documents SET year = ?2, year_origin = 'user' WHERE id = ?1", params![id.as_str(), y])
                    .map_err(db_err)?;
            }
            tx.execute("UPDATE documents SET updated_at = ?2 WHERE id = ?1", params![id.as_str(), now]).map_err(db_err)?;
            Ok(())
        })
    }

    fn begin_extraction_revision(&self, id: &DocumentId, now: i64) -> AppResult<i64> {
        self.db.tx(|tx| {
            let published: i64 = tx
                .query_row("SELECT extraction_revision FROM documents WHERE id = ?1", params![id.as_str()], |r| r.get(0))
                .optional()
                .map_err(db_err)?
                .ok_or_else(|| AppError::not_found("document"))?;
            let revision = published + 1;
            // Drop leftovers of an abandoned attempt at the same revision.
            tx.execute(
                "DELETE FROM document_pages WHERE document_id = ?1 AND extraction_revision >= ?2",
                params![id.as_str(), revision],
            )
            .map_err(db_err)?;
            tx.execute("UPDATE documents SET updated_at = ?2 WHERE id = ?1", params![id.as_str(), now]).map_err(db_err)?;
            Ok(revision)
        })
    }

    fn finish_extraction(
        &self,
        id: &DocumentId,
        revision: i64,
        text_pages: u32,
        low_text_pages: u32,
        state: ExtractionState,
        now: i64,
    ) -> AppResult<()> {
        self.db.tx(|tx| {
            tx.execute(
                "UPDATE documents SET extraction_revision = ?2, text_page_count = ?3, low_text_page_count = ?4,
                    extraction_state = ?5, failure_kind = NULL, updated_at = ?6
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![id.as_str(), revision, text_pages, low_text_pages, state.as_str(), now],
            )
            .map_err(db_err)?;
            tx.execute(
                "DELETE FROM document_pages WHERE document_id = ?1 AND extraction_revision <> ?2",
                params![id.as_str(), revision],
            )
            .map_err(db_err)?;
            Ok(())
        })
    }

    fn mark_deleting(&self, id: &DocumentId, now: i64) -> AppResult<()> {
        self.db
            .with(|c| {
                c.execute(
                    "UPDATE documents SET extraction_state = 'deleting', deleted_at = ?2, updated_at = ?2 WHERE id = ?1",
                    params![id.as_str(), now],
                )
            })
            .map(|_| ())
    }

    fn purge_derived(&self, id: &DocumentId, now: i64) -> AppResult<()> {
        self.db.tx(|tx| {
            tx.execute("DELETE FROM document_pages WHERE document_id = ?1", params![id.as_str()]).map_err(db_err)?;
            tx.execute("DELETE FROM chunks WHERE document_id = ?1", params![id.as_str()]).map_err(db_err)?;
            tx.execute("DELETE FROM index_generations WHERE document_id = ?1", params![id.as_str()]).map_err(db_err)?;
            tx.execute("DELETE FROM collection_documents WHERE document_id = ?1", params![id.as_str()]).map_err(db_err)?;
            tx.execute(
                "UPDATE documents SET extraction_state = 'deleted', managed_file = NULL, deleted_at = COALESCE(deleted_at, ?2),
                    lexical_ready = 0, semantic_ready = 0, active_generation_id = NULL, embedding_fingerprint = NULL, updated_at = ?2
                 WHERE id = ?1",
                params![id.as_str(), now],
            )
            .map_err(db_err)?;
            Ok(())
        })
    }
}
