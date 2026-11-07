//! Application error type shared by all use cases.
use serde::Serialize;
use thiserror::Error;
use ts_rs::TS;

use crate::domain::error::DomainError;

/// Stable, user-facing error categories. The UI maps each code to a localized message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ErrorCode {
    Validation,
    NotFound,
    Conflict,
    Storage,
    FileAccess,
    NotAPdf,
    FileTooLarge,
    FileEmpty,
    PdfEngineMissing,
    ImportTokenExpired,
    NotAllowed,
    Unsupported,
    Internal,
}

/// `detail` is for local diagnostics only and is never shown verbatim to the user.
#[derive(Debug, Error)]
#[error("{code:?}: {detail}")]
pub struct AppError {
    pub code: ErrorCode,
    pub detail: String,
    pub retryable: bool,
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn new(code: ErrorCode, detail: impl Into<String>) -> Self {
        Self { code, detail: detail.into(), retryable: false }
    }
    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }
    pub fn validation(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::Validation, detail)
    }
    pub fn not_found(what: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, what)
    }
    pub fn conflict(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, detail)
    }
    pub fn storage(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::Storage, detail).retryable()
    }
    pub fn file_access(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::FileAccess, detail)
    }
    pub fn not_allowed(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotAllowed, detail)
    }
    pub fn internal(detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, detail)
    }
}

impl From<DomainError> for AppError {
    fn from(e: DomainError) -> Self {
        AppError::validation(e.to_string())
    }
}
