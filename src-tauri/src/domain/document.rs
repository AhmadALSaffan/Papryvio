//! Document lifecycle, metadata provenance and text-quality rules.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::error::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ExtractionState {
    Imported,
    Extracting,
    TextReady,
    NeedsOcr,
    Indexing,
    Ready,
    Failed,
    Deleting,
    Deleted,
}

impl ExtractionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Imported => "imported",
            Self::Extracting => "extracting",
            Self::TextReady => "text_ready",
            Self::NeedsOcr => "needs_ocr",
            Self::Indexing => "indexing",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Deleting => "deleting",
            Self::Deleted => "deleted",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        Ok(match raw {
            "imported" => Self::Imported,
            "extracting" => Self::Extracting,
            "text_ready" => Self::TextReady,
            "needs_ocr" => Self::NeedsOcr,
            "indexing" => Self::Indexing,
            "ready" => Self::Ready,
            "failed" => Self::Failed,
            "deleting" => Self::Deleting,
            "deleted" => Self::Deleted,
            _ => return Err(DomainError::InvalidValue { field: "extraction_state" }),
        })
    }

    /// Whether the PDF file itself can be opened in the reader.
    pub fn is_readable(self) -> bool {
        !matches!(self, Self::Deleting | Self::Deleted)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MetadataOrigin {
    PdfMetadata,
    FileName,
    User,
}

impl MetadataOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PdfMetadata => "pdf_metadata",
            Self::FileName => "file_name",
            Self::User => "user",
        }
    }
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "pdf_metadata" => Some(Self::PdfMetadata),
            "file_name" => Some(Self::FileName),
            "user" => Some(Self::User),
            _ => None,
        }
    }
}

/// Why extraction did not produce usable text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FailureKind {
    PasswordProtected,
    Corrupt,
    WorkerCrashed,
    Timeout,
    OutputTooLarge,
    FileMissing,
    PdfEngineMissing,
    Cancelled,
    Unknown,
}

impl FailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PasswordProtected => "password_protected",
            Self::Corrupt => "corrupt",
            Self::WorkerCrashed => "worker_crashed",
            Self::Timeout => "timeout",
            Self::OutputTooLarge => "output_too_large",
            Self::FileMissing => "file_missing",
            Self::PdfEngineMissing => "pdf_engine_missing",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }
    pub fn parse(raw: &str) -> Self {
        match raw {
            "password_protected" => Self::PasswordProtected,
            "corrupt" => Self::Corrupt,
            "worker_crashed" => Self::WorkerCrashed,
            "timeout" => Self::Timeout,
            "output_too_large" => Self::OutputTooLarge,
            "file_missing" => Self::FileMissing,
            "pdf_engine_missing" => Self::PdfEngineMissing,
            "cancelled" => Self::Cancelled,
            _ => Self::Unknown,
        }
    }
    /// Retrying can help for operational failures, not for problems in the file itself.
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::WorkerCrashed | Self::Timeout | Self::Cancelled | Self::Unknown | Self::PdfEngineMissing
        )
    }
}

/// Internal pages are zero-based; the UI shows PDF page numbers starting at 1.
pub fn display_page_number(page_index: u32) -> u32 {
    page_index + 1
}

/// Minimum non-whitespace characters for a page to count as having a text layer.
pub const MIN_TEXT_CHARS_PER_PAGE: usize = 25;

pub const QUALITY_NO_TEXT: &str = "no_text";
pub const QUALITY_LOW_TEXT: &str = "low_text";
pub const QUALITY_IMAGE_ONLY: &str = "image_only";

/// Classifies a page's text layer.
pub fn page_quality_flags(text: &str, image_objects: u32) -> Vec<&'static str> {
    let visible = text.chars().filter(|c| !c.is_whitespace()).count();
    let mut flags = Vec::new();
    if visible == 0 {
        flags.push(QUALITY_NO_TEXT);
        if image_objects > 0 {
            flags.push(QUALITY_IMAGE_ONLY);
        }
    } else if visible < MIN_TEXT_CHARS_PER_PAGE {
        flags.push(QUALITY_LOW_TEXT);
    } else {
        // Many replacement characters usually mean a broken font encoding.
        let broken = text.chars().filter(|c| *c == '\u{FFFD}').count();
        if broken * 10 > visible {
            flags.push(QUALITY_LOW_TEXT);
        }
    }
    flags
}

pub fn page_has_text(text: &str) -> bool {
    page_quality_flags(text, 0).is_empty()
}

/// Final state after all pages were extracted.
pub fn state_after_extraction(text_pages: u32, total_pages: u32) -> ExtractionState {
    if total_pages == 0 || text_pages == 0 {
        ExtractionState::NeedsOcr
    } else {
        ExtractionState::TextReady
    }
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Picks a title from PDF metadata only when it looks like a real title.
pub fn plausible_metadata_title(raw: Option<&str>) -> Option<String> {
    let t = collapse_ws(raw?);
    let n = t.chars().count();
    if !(4..=400).contains(&n) || !t.chars().any(|c| c.is_alphabetic()) {
        return None;
    }
    let lower = t.to_lowercase();
    let junk_exact = ["untitled", "document", "slide 1", "title", "paper"];
    let junk_ext = [".pdf", ".docx", ".doc", ".dvi", ".tex", ".ps"];
    if junk_exact.contains(&lower.as_str())
        || lower.starts_with("microsoft word")
        || junk_ext.iter().any(|e| lower.ends_with(e))
    {
        return None;
    }
    Some(t)
}

/// Title derived from the file name, used until the PDF provides one or the user edits it.
pub fn title_from_file_name(file_name: &str) -> String {
    let lower = file_name.to_lowercase();
    let stem = if lower.ends_with(".pdf") { &file_name[..file_name.len() - 4] } else { file_name };
    let cleaned = collapse_ws(&stem.replace('_', " "));
    if cleaned.is_empty() {
        file_name.to_string()
    } else {
        cleaned
    }
}

pub fn plausible_authors(raw: Option<&str>) -> Option<String> {
    let t = collapse_ws(raw?);
    let n = t.chars().count();
    if !(2..=300).contains(&n) || !t.chars().any(|c| c.is_alphabetic()) {
        return None;
    }
    Some(t)
}

/// Parses the year from a PDF date string such as `D:20230514120000+02'00'`.
pub fn year_from_pdf_date(raw: Option<&str>) -> Option<i32> {
    let s = raw?.trim();
    let s = s.strip_prefix("D:").unwrap_or(s);
    let digits: String = s.chars().take(4).collect();
    if digits.len() != 4 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let year: i32 = digits.parse().ok()?;
    (1900..=2100).contains(&year).then_some(year)
}

pub fn validate_user_year(year: Option<i32>) -> Result<Option<i32>, DomainError> {
    match year {
        None => Ok(None),
        Some(y) if (1000..=2200).contains(&y) => Ok(Some(y)),
        Some(_) => Err(DomainError::InvalidValue { field: "year" }),
    }
}

/// Normalizes a user-entered metadata field. Empty input clears the field.
pub fn validate_user_text(raw: Option<&str>, field: &'static str, max: usize) -> Result<Option<String>, DomainError> {
    match raw {
        None => Ok(None),
        Some(s) => {
            let t = collapse_ws(s);
            if t.is_empty() {
                return Ok(None);
            }
            if t.chars().count() > max || t.chars().any(|c| c.is_control()) {
                return Err(DomainError::InvalidValue { field });
            }
            Ok(Some(t))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_number_display_is_one_based() {
        assert_eq!(display_page_number(0), 1);
        assert_eq!(display_page_number(13), 14);
    }

    #[test]
    fn quality_flags() {
        assert_eq!(page_quality_flags("", 1), vec![QUALITY_NO_TEXT, QUALITY_IMAGE_ONLY]);
        assert_eq!(page_quality_flags("  \n ", 0), vec![QUALITY_NO_TEXT]);
        assert_eq!(page_quality_flags("Fig. 3", 0), vec![QUALITY_LOW_TEXT]);
        assert!(page_quality_flags(&"word ".repeat(40), 0).is_empty());
    }

    #[test]
    fn scanned_document_needs_ocr() {
        assert_eq!(state_after_extraction(0, 18), ExtractionState::NeedsOcr);
        assert_eq!(state_after_extraction(3, 18), ExtractionState::TextReady);
        assert_eq!(state_after_extraction(0, 0), ExtractionState::NeedsOcr);
    }

    #[test]
    fn metadata_title_heuristics() {
        assert_eq!(plausible_metadata_title(Some("Microsoft Word - draft7.docx")), None);
        assert_eq!(plausible_metadata_title(Some("paper.pdf")), None);
        assert_eq!(plausible_metadata_title(Some("  ")), None);
        assert_eq!(
            plausible_metadata_title(Some("Dense  Passage Retrieval for QA")).as_deref(),
            Some("Dense Passage Retrieval for QA")
        );
    }

    #[test]
    fn file_name_title() {
        assert_eq!(title_from_file_name("my_paper_2023.pdf"), "my paper 2023");
        assert_eq!(title_from_file_name("ورقة.PDF"), "ورقة");
    }

    #[test]
    fn pdf_date_year() {
        assert_eq!(year_from_pdf_date(Some("D:20230514120000+02'00'")), Some(2023));
        assert_eq!(year_from_pdf_date(Some("garbage")), None);
        assert_eq!(year_from_pdf_date(None), None);
    }

    #[test]
    fn state_roundtrip() {
        for s in ["imported", "extracting", "text_ready", "needs_ocr", "indexing", "ready", "failed", "deleting", "deleted"] {
            assert_eq!(ExtractionState::parse(s).unwrap().as_str(), s);
        }
    }
}
