//! Converts application errors into IPC errors. Internal details stay in the local log;
//! the UI receives a stable code and a correlation id.
use serde::Serialize;
use ts_rs::TS;

use crate::application::error::{AppError, ErrorCode};

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IpcError {
    pub code: ErrorCode,
    pub retryable: bool,
    pub correlation_id: String,
}

impl From<AppError> for IpcError {
    fn from(e: AppError) -> Self {
        let correlation_id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        match e.code {
            ErrorCode::Internal | ErrorCode::Storage => {
                tracing::error!(correlation_id, code = ?e.code, "{}", e.detail)
            }
            _ => tracing::warn!(correlation_id, code = ?e.code, "{}", e.detail),
        }
        IpcError { code: e.code, retryable: e.retryable, correlation_id }
    }
}

pub type IpcResult<T> = Result<T, IpcError>;
