//! Read models returned to the desktop adapter. They are serialized as IPC DTOs
//! (camelCase) and exported to TypeScript through ts-rs.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::document::{ExtractionState, FailureKind, MetadataOrigin};
use crate::domain::ids::{AttemptId, CollectionId, DocumentId, JobId};
use crate::domain::job::{JobKind, JobState};
use crate::domain::onboarding::{StepKey, StepState};

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CollectionView {
    pub id: CollectionId,
    pub name: String,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
    /// Live documents linked to this collection.
    pub document_count: u32,
    /// Documents whose text was extracted (keyword search becomes possible in the indexing phase).
    pub text_ready_count: u32,
    /// Documents still waiting for or going through extraction.
    pub processing_count: u32,
    /// Documents with no extractable text (scanned).
    pub needs_ocr_count: u32,
    pub failed_count: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JobView {
    pub id: JobId,
    pub kind: JobKind,
    pub entity_id: String,
    pub attempt_id: AttemptId,
    pub state: JobState,
    /// Machine-readable stage key, e.g. `reading_pages`.
    pub stage: String,
    pub completed_units: u32,
    /// `None` while the total is unknown: the UI shows an indeterminate indicator.
    pub total_units: Option<u32>,
    pub error_kind: Option<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DocumentView {
    pub id: DocumentId,
    pub title: String,
    pub title_origin: MetadataOrigin,
    pub authors: Option<String>,
    pub authors_origin: Option<MetadataOrigin>,
    pub year: Option<i32>,
    pub year_origin: Option<MetadataOrigin>,
    pub page_count: Option<u32>,
    pub state: ExtractionState,
    pub failure_kind: Option<FailureKind>,
    pub text_page_count: u32,
    pub low_text_page_count: u32,
    pub lexical_ready: bool,
    pub semantic_ready: bool,
    pub original_file_name: String,
    #[ts(type = "number")]
    pub file_size: u64,
    #[ts(type = "number")]
    pub created_at: i64,
    pub collection_ids: Vec<CollectionId>,
    pub job: Option<JobView>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DocumentListPage {
    pub items: Vec<DocumentView>,
    pub total: u32,
    pub next_cursor: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct DocumentQuery {
    pub collection_id: Option<CollectionId>,
    pub filter: Option<String>,
    pub cursor: Option<u32>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SettingsView {
    /// `en` or `ar`.
    pub locale: String,
    pub answer_locale: String,
    pub enter_to_send: bool,
    pub library_collapsed: bool,
    pub active_collection_id: Option<CollectionId>,
}

#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct SettingsPatch {
    pub locale: Option<String>,
    pub answer_locale: Option<String>,
    pub enter_to_send: Option<bool>,
    pub library_collapsed: Option<bool>,
    /// An empty string clears the active collection.
    pub active_collection_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OnboardingStepView {
    pub key: StepKey,
    pub state: StepState,
    pub related_entity_id: Option<String>,
    #[ts(type = "number | null")]
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OnboardingView {
    #[ts(type = "number")]
    pub flow_version: i64,
    pub steps: Vec<OnboardingStepView>,
    /// True once the user finished or dismissed the guided flow at least once.
    pub dismissed: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StorageView {
    pub data_dir: String,
    #[ts(type = "number")]
    pub library_bytes: u64,
    #[ts(type = "number")]
    pub database_bytes: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BootstrapView {
    pub app_version: String,
    #[ts(type = "number")]
    pub schema_version: i64,
    pub pdf_engine_available: bool,
    pub settings: SettingsView,
    pub onboarding: OnboardingView,
    pub collections: Vec<CollectionView>,
    pub active_jobs: Vec<JobView>,
    pub storage: StorageView,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SelectedFile {
    pub name: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportSelection {
    /// Opaque token that refers to files the user picked in a native dialog or dropped on the window.
    pub token: String,
    pub files: Vec<SelectedFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RejectReason {
    NotAPdf,
    TooLarge,
    Empty,
    Unreadable,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", tag = "type")]
#[ts(export)]
pub enum ImportOutcome {
    #[serde(rename_all = "camelCase")]
    Imported { document_id: DocumentId, job_id: JobId },
    #[serde(rename_all = "camelCase")]
    AlreadyInLibrary { document_id: DocumentId, added_to_collection: bool },
    #[serde(rename_all = "camelCase")]
    Rejected { reason: RejectReason },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportItemResult {
    pub file_name: String,
    pub outcome: ImportOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportReport {
    pub collection_id: CollectionId,
    pub items: Vec<ImportItemResult>,
}

#[derive(Debug, Clone, Default, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, optional_fields)]
pub struct MetadataUpdate {
    pub title: Option<String>,
    /// Empty string clears the field.
    pub authors: Option<String>,
    /// `null` with `clearYear` clears the field.
    pub year: Option<i32>,
    #[serde(default)]
    pub clear_year: bool,
}

pub use super::views2::*;
