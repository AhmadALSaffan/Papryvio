//! Tauri commands. Each validates its input, runs the use case on a blocking thread
//! (SQLite and file I/O never run on the async executor) and maps errors.
pub mod answers;
pub mod documents;
pub mod library;
pub mod models;
pub mod system;

use std::sync::Arc;

use tauri::State;

use crate::application::error::{AppError, AppResult};
use crate::desktop::error::{IpcError, IpcResult};
use crate::desktop::DesktopState;

pub(crate) async fn blocking<T, F>(state: State<'_, Arc<DesktopState>>, f: F) -> IpcResult<T>
where
    T: Send + 'static,
    F: FnOnce(&DesktopState) -> AppResult<T> + Send + 'static,
{
    let st = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || f(&st))
        .await
        .map_err(|e| IpcError::from(AppError::internal(format!("task join: {e}"))))?
        .map_err(IpcError::from)
}

/// Validates a client-generated request id used for idempotency.
pub(crate) fn request_id(raw: &str) -> AppResult<String> {
    let ok = !raw.is_empty() && raw.len() <= 64 && raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if ok {
        Ok(raw.to_string())
    } else {
        Err(AppError::validation("invalid request id"))
    }
}
