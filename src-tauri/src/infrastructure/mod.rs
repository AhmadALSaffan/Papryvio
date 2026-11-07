//! Adapters for SQLite, the file system, PDFium and the system clock.
pub mod auth;
pub mod filesystem;
pub mod models;
pub mod pdf;
pub mod sqlite;

use crate::application::ports::Clock;

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}
