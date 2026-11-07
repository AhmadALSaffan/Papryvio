use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("collection name must not be empty")]
    EmptyName,
    #[error("collection name is too long (max {max} characters)")]
    NameTooLong { max: usize },
    #[error("invalid value for {field}")]
    InvalidValue { field: &'static str },
    #[error("invalid state transition from {from} to {to}")]
    InvalidTransition { from: &'static str, to: &'static str },
}
