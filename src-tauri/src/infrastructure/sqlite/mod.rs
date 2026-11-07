//! SQLite adapter. One connection owned behind a mutex: a single, explicit writer.
//! Callers run on blocking threads (never on the async executor).
mod answers;
mod collections;
mod documents;
mod index;
mod jobs;
mod migrations;
mod misc;
mod pages;
mod providers;

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OptionalExtension};

use crate::application::error::{AppError, AppResult};

pub use migrations::SCHEMA_VERSION;

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> AppResult<Arc<Db>> {
        let conn = Connection::open(path).map_err(db_err)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> AppResult<Arc<Db>> {
        Self::init(Connection::open_in_memory().map_err(db_err)?)
    }

    fn init(mut conn: Connection) -> AppResult<Arc<Db>> {
        conn.pragma_update(None, "journal_mode", "WAL").map_err(db_err)?;
        conn.pragma_update(None, "foreign_keys", "ON").map_err(db_err)?;
        conn.pragma_update(None, "synchronous", "NORMAL").map_err(db_err)?;
        conn.busy_timeout(std::time::Duration::from_secs(5)).map_err(db_err)?;
        migrations::migrate(&mut conn)?;
        Ok(Arc::new(Db { conn: Mutex::new(conn) }))
    }

    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> AppResult<T> {
        let conn = self.conn.lock().map_err(|_| AppError::internal("db mutex poisoned"))?;
        f(&conn).map_err(db_err)
    }

    pub fn tx<T>(&self, f: impl FnOnce(&rusqlite::Transaction<'_>) -> AppResult<T>) -> AppResult<T> {
        let mut conn = self.conn.lock().map_err(|_| AppError::internal("db mutex poisoned"))?;
        let tx = conn.transaction().map_err(db_err)?;
        let out = f(&tx)?;
        tx.commit().map_err(db_err)?;
        Ok(out)
    }

    pub fn sqlite_version(&self) -> AppResult<String> {
        self.with(|c| c.query_row("select sqlite_version()", [], |r| r.get(0)))
    }

    pub fn fts5_available(&self) -> AppResult<bool> {
        self.with(|c| {
            c.query_row("select sqlite_compileoption_used('ENABLE_FTS5')", [], |r| r.get::<_, i64>(0))
                .optional()
                .map(|v| v == Some(1))
        })
    }
}

pub(crate) fn db_err(e: rusqlite::Error) -> AppError {
    if let rusqlite::Error::SqliteFailure(f, _) = &e {
        if f.code == rusqlite::ErrorCode::ConstraintViolation {
            return AppError::conflict(e.to_string());
        }
        if f.code == rusqlite::ErrorCode::DiskFull {
            return AppError::storage("disk full");
        }
    }
    AppError::storage(e.to_string())
}

/// Implements every repository port on top of the shared connection.
#[derive(Clone)]
pub struct SqliteStore {
    pub(crate) db: Arc<Db>,
}

impl SqliteStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod rag_tests;
