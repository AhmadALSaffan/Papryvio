use rusqlite::{params, OptionalExtension, Row};

use crate::application::error::AppResult;
use crate::application::ports::{ProviderKind, ProviderRecord, ProviderRepository};

use super::SqliteStore;

const COLUMNS: &str = "id, kind, label, base_url, secret_ref, external_id, config, created_at";

fn map(r: &Row<'_>) -> rusqlite::Result<ProviderRecord> {
    Ok(ProviderRecord {
        id: r.get(0)?,
        kind: ProviderKind::parse(&r.get::<_, String>(1)?).unwrap_or(ProviderKind::OpenaiCompatible),
        label: r.get(2)?,
        base_url: r.get(3)?,
        secret_ref: r.get(4)?,
        external_id: r.get(5)?,
        config: serde_json::from_str(&r.get::<_, String>(6)?).unwrap_or_default(),
        created_at: r.get(7)?,
    })
}

impl ProviderRepository for SqliteStore {
    fn save(&self, p: &ProviderRecord, now: i64) -> AppResult<()> {
        self.db
            .with(|c| {
                c.execute(
                    "INSERT INTO providers (id, kind, label, base_url, secret_ref, external_id, config, is_local, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                     ON CONFLICT(id) DO UPDATE SET label = excluded.label, base_url = excluded.base_url, secret_ref = excluded.secret_ref,
                        external_id = excluded.external_id, config = excluded.config, updated_at = excluded.updated_at",
                    params![
                        p.id,
                        p.kind.as_str(),
                        p.label,
                        p.base_url,
                        p.secret_ref,
                        p.external_id,
                        p.config.to_string(),
                        p.kind.is_local() as i64,
                        p.created_at,
                        now
                    ],
                )
            })
            .map(|_| ())
    }

    fn get(&self, id: &str) -> AppResult<Option<ProviderRecord>> {
        self.db.with(|c| c.query_row(&format!("SELECT {COLUMNS} FROM providers WHERE id = ?1"), params![id], map).optional())
    }

    fn list(&self) -> AppResult<Vec<ProviderRecord>> {
        self.db.with(|c| {
            let mut st = c.prepare(&format!("SELECT {COLUMNS} FROM providers ORDER BY created_at"))?;
            let rows = st.query_map([], map)?;
            rows.collect()
        })
    }

    fn delete(&self, id: &str) -> AppResult<()> {
        self.db.with(|c| c.execute("DELETE FROM providers WHERE id = ?1", params![id])).map(|_| ())
    }
}
