//! Typed identifiers so document, collection, job and attempt ids cannot be mixed up.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

macro_rules! typed_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
        #[serde(transparent)]
        #[ts(export, type = "string")]
        pub struct $name(String);

        impl $name {
            /// Time-ordered UUID v7.
            pub fn new() -> Self {
                Self(uuid::Uuid::now_v7().to_string())
            }
            /// Parses an id received over IPC; only canonical UUIDs are accepted.
            pub fn parse(raw: &str) -> Option<Self> {
                uuid::Uuid::parse_str(raw).ok().map(|u| Self(u.hyphenated().to_string()))
            }
            /// Wraps a value read back from our own storage.
            pub fn from_trusted(raw: String) -> Self {
                Self(raw)
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

typed_id!(CollectionId);
typed_id!(DocumentId);
typed_id!(JobId);
typed_id!(AttemptId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rejects_non_uuid() {
        assert!(DocumentId::parse("../etc/passwd").is_none());
        assert!(DocumentId::parse("").is_none());
        let id = DocumentId::new();
        assert_eq!(DocumentId::parse(id.as_str()), Some(id));
    }
}
