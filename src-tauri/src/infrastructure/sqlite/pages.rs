use rusqlite::params;

use crate::application::error::AppResult;
use crate::application::ports::{ExtractedPage, PageRepository};
use crate::domain::document::{QUALITY_LOW_TEXT, QUALITY_NO_TEXT};
use crate::domain::ids::DocumentId;

use super::{db_err, SqliteStore};

impl PageRepository for SqliteStore {
    fn save_page(&self, doc: &DocumentId, revision: i64, page: &ExtractedPage, quality_flags: &str) -> AppResult<()> {
        self.db.tx(|tx| {
            // Idempotent per (document, revision, page): replaying after a crash overwrites.
            tx.execute(
                "DELETE FROM document_pages WHERE document_id = ?1 AND extraction_revision = ?2 AND page_index = ?3",
                params![doc.as_str(), revision, page.index],
            )
            .map_err(db_err)?;
            tx.execute(
                "INSERT INTO document_pages (document_id, extraction_revision, page_index, printed_label, width, height,
                    rotation, text, char_count, quality_flags)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    doc.as_str(),
                    revision,
                    page.index,
                    page.label,
                    page.width as f64,
                    page.height as f64,
                    page.rotation,
                    page.text,
                    page.text.chars().count() as i64,
                    quality_flags
                ],
            )
            .map_err(db_err)?;
            let page_id = tx.last_insert_rowid();
            let mut st = tx
                .prepare_cached(
                    "INSERT INTO text_spans (page_id, start_offset, end_offset, left, bottom, right, top)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )
                .map_err(db_err)?;
            for s in &page.spans {
                st.execute(params![page_id, s.start, s.end, s.left as f64, s.bottom as f64, s.right as f64, s.top as f64])
                    .map_err(db_err)?;
            }
            Ok(())
        })
    }

    fn clear_revision(&self, doc: &DocumentId, revision: i64) -> AppResult<()> {
        self.db
            .with(|c| {
                c.execute(
                    "DELETE FROM document_pages WHERE document_id = ?1 AND extraction_revision = ?2",
                    params![doc.as_str(), revision],
                )
            })
            .map(|_| ())
    }

    fn text_page_counts(&self, doc: &DocumentId, revision: i64) -> AppResult<(u32, u32)> {
        self.db.with(|c| {
            c.query_row(
                "SELECT
                    COALESCE(SUM(instr(quality_flags, ?3) = 0 AND instr(quality_flags, ?4) = 0), 0),
                    COALESCE(SUM(instr(quality_flags, ?4) > 0), 0)
                 FROM document_pages WHERE document_id = ?1 AND extraction_revision = ?2",
                params![doc.as_str(), revision, QUALITY_NO_TEXT, QUALITY_LOW_TEXT],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
    }
}
