//! Numbered, embedded migrations tracked with `PRAGMA user_version`.
use rusqlite::Connection;

use crate::application::error::{AppError, AppResult};

use super::db_err;

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../../../migrations/0001_init.sql")),
    (2, include_str!("../../../migrations/0002_models_index_answers.sql")),
];

pub const SCHEMA_VERSION: i64 = 2;

pub fn migrate(conn: &mut Connection) -> AppResult<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).map_err(db_err)?;
    if current > SCHEMA_VERSION {
        return Err(AppError::internal(format!(
            "database schema v{current} is newer than this app (v{SCHEMA_VERSION})"
        )));
    }
    for (version, sql) in MIGRATIONS {
        if *version <= current {
            continue;
        }
        let tx = conn.transaction().map_err(db_err)?;
        tx.execute_batch(sql).map_err(db_err)?;
        tx.pragma_update(None, "user_version", version).map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        tracing::info!(version, "applied migration");
    }
    Ok(())
}
