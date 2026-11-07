use super::error::DomainError;
use super::ids::CollectionId;

pub const MAX_COLLECTION_NAME: usize = 80;

/// A validated, trimmed collection name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionName(String);

impl CollectionName {
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        let trimmed: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if trimmed.is_empty() {
            return Err(DomainError::EmptyName);
        }
        if trimmed.chars().count() > MAX_COLLECTION_NAME {
            return Err(DomainError::NameTooLong { max: MAX_COLLECTION_NAME });
        }
        if trimmed.chars().any(|c| c.is_control()) {
            return Err(DomainError::InvalidValue { field: "name" });
        }
        Ok(Self(trimmed))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct Collection {
    pub id: CollectionId,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_collapses_whitespace() {
        assert_eq!(CollectionName::parse("  My   papers ").unwrap().as_str(), "My papers");
    }

    #[test]
    fn rejects_empty_and_long() {
        assert_eq!(CollectionName::parse("   "), Err(DomainError::EmptyName));
        let long = "x".repeat(MAX_COLLECTION_NAME + 1);
        assert!(matches!(CollectionName::parse(&long), Err(DomainError::NameTooLong { .. })));
    }

    #[test]
    fn accepts_arabic() {
        assert_eq!(CollectionName::parse("مراجعة الاسترجاع").unwrap().as_str(), "مراجعة الاسترجاع");
    }
}
