//! Onboarding flow definition. Progress is stored per flow version.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::error::DomainError;

pub const FLOW_VERSION: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum StepKey {
    Welcome,
    Models,
    Collection,
    Import,
    Readiness,
    FirstQuestion,
    OpenSource,
    Compare,
}

impl StepKey {
    pub const ALL: [StepKey; 8] = [
        StepKey::Welcome,
        StepKey::Models,
        StepKey::Collection,
        StepKey::Import,
        StepKey::Readiness,
        StepKey::FirstQuestion,
        StepKey::OpenSource,
        StepKey::Compare,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Welcome => "welcome",
            Self::Models => "models",
            Self::Collection => "collection",
            Self::Import => "import",
            Self::Readiness => "readiness",
            Self::FirstQuestion => "first_question",
            Self::OpenSource => "open_source",
            Self::Compare => "compare",
        }
    }
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        Self::ALL
            .into_iter()
            .find(|s| s.as_str() == raw)
            .ok_or(DomainError::InvalidValue { field: "step_key" })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum StepState {
    Pending,
    Completed,
    Skipped,
}

impl StepState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Completed => "completed",
            Self::Skipped => "skipped",
        }
    }
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        match raw {
            "pending" => Ok(Self::Pending),
            "completed" => Ok(Self::Completed),
            "skipped" => Ok(Self::Skipped),
            _ => Err(DomainError::InvalidValue { field: "step_state" }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum StepAction {
    Complete,
    Skip,
    Reset,
}
