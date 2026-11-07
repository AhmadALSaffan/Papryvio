use rusqlite::{params, OptionalExtension, Row};

use crate::application::error::{AppError, AppResult};
use crate::application::ports::CollectionRepository;
use crate::application::views::CollectionView;
use crate::domain::collection::Collection;
use crate::domain::ids::CollectionId;

use super::SqliteStore;

const VIEW_SQL: &str = "
SELECT c.id, c.name, c.created_at, c.updated_at,
       COUNT(d.id),
       COALESCE(SUM(d.extraction_state IN ('text_ready','indexing','ready')), 0),
       COALESCE(SUM(d.extraction_state IN ('imported','extracting')), 0),
       COALESCE(SUM(d.extraction_state = 'needs_ocr'), 0),
       COALESCE(SUM(d.extraction_state = 'failed'), 0)
FROM collections c
LEFT JOIN collection_documents cd ON cd.collection_id = c.id
LEFT JOIN documents d ON d.id = cd.document_id AND d.deleted_at IS NULL
";

fn map_view(r: &Row<'_>) -> rusqlite::Result<CollectionView> {
    Ok(CollectionView {
        id: CollectionId::from_trusted(r.get(0)?),
        name: r.get(1)?,
        created_at: r.get(2)?,
        updated_at: r.get(3)?,
        document_count: r.get(4)?,
        text_ready_count: r.get(5)?,
        processing_count: r.get(6)?,
        needs_ocr_count: r.get(7)?,
        failed_count: r.get(8)?,
    })
}

impl CollectionRepository for SqliteStore {
    fn create(&self, c: &Collection) -> AppResult<()> {
        self.db
            .with(|conn| {
                conn.execute(
                    "INSERT INTO collections (id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
                    params![c.id.as_str(), c.name, c.created_at, c.updated_at],
                )
            })
            .map(|_| ())
    }

    fn rename(&self, id: &CollectionId, name: &str, now: i64) -> AppResult<()> {
        let n = self.db.with(|conn| {
            conn.execute("UPDATE collections SET name = ?2, updated_at = ?3 WHERE id = ?1", params![id.as_str(), name, now])
        })?;
        if n == 0 {
            return Err(AppError::not_found("collection"));
        }
        Ok(())
    }

    fn delete(&self, id: &CollectionId) -> AppResult<()> {
        self.db.with(|conn| conn.execute("DELETE FROM collections WHERE id = ?1", params![id.as_str()])).map(|_| ())
    }

    fn exists(&self, id: &CollectionId) -> AppResult<bool> {
        self.db.with(|conn| {
            conn.query_row("SELECT 1 FROM collections WHERE id = ?1", params![id.as_str()], |_| Ok(()))
                .optional()
                .map(|o| o.is_some())
        })
    }

    fn list(&self) -> AppResult<Vec<CollectionView>> {
        self.db.with(|conn| {
            let mut st = conn.prepare(&format!("{VIEW_SQL} GROUP BY c.id ORDER BY lower(c.name)"))?;
            let rows = st.query_map([], map_view)?;
            rows.collect()
        })
    }

    fn get(&self, id: &CollectionId) -> AppResult<Option<CollectionView>> {
        self.db.with(|conn| {
            conn.query_row(&format!("{VIEW_SQL} WHERE c.id = ?1 GROUP BY c.id"), params![id.as_str()], map_view).optional()
        })
    }
}
