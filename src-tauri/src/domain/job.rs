//! Background job state machine.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::error::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum JobKind {
    Extract,
    Index,
    Embed,
    DeleteDocument,
}

impl JobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Extract => "extract",
            Self::Index => "index",
            Self::Embed => "embed",
            Self::DeleteDocument => "delete_document",
        }
    }
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        match raw {
            "extract" => Ok(Self::Extract),
            "index" => Ok(Self::Index),
            "embed" => Ok(Self::Embed),
            "delete_document" => Ok(Self::DeleteDocument),
            _ => Err(DomainError::InvalidValue { field: "job_kind" }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum JobState {
    Queued,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        Ok(match raw {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "paused" => Self::Paused,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => return Err(DomainError::InvalidValue { field: "job_state" }),
        })
    }
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
    pub fn is_active(self) -> bool {
        !self.is_terminal()
    }

    /// Allowed transitions. A retry starts a new attempt via Failed/Cancelled -> Queued.
    pub fn can_transition_to(self, next: JobState) -> bool {
        use JobState::*;
        matches!(
            (self, next),
            (Queued, Running)
                | (Queued, Paused)
                | (Queued, Cancelled)
                | (Running, Paused)
                | (Running, Completed)
                | (Running, Failed)
                | (Running, Cancelled)
                | (Running, Queued) // interrupted by shutdown; resumes from its checkpoint
                | (Paused, Queued)
                | (Paused, Cancelled)
                | (Failed, Queued)
                | (Cancelled, Queued)
        )
    }

    pub fn transition(self, next: JobState) -> Result<JobState, DomainError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(DomainError::InvalidTransition { from: self.as_str(), to: next.as_str() })
        }
    }
}

/// Extraction checkpoint: the next page to process within an extraction revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractCheckpoint {
    pub revision: i64,
    pub next_page: u32,
}

#[cfg(test)]
mod tests {
    use super::JobState::*;

    #[test]
    fn transitions() {
        assert!(Queued.can_transition_to(Running));
        assert!(Running.can_transition_to(Paused));
        assert!(Paused.can_transition_to(Queued));
        assert!(Failed.can_transition_to(Queued));
        assert!(!Completed.can_transition_to(Running));
        assert!(!Paused.can_transition_to(Completed));
        assert!(Completed.transition(Queued).is_err());
    }

    #[test]
    fn terminal_states() {
        assert!(Completed.is_terminal() && Failed.is_terminal() && Cancelled.is_terminal());
        assert!(Queued.is_active() && Running.is_active() && Paused.is_active());
    }
}
