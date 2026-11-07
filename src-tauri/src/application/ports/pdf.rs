//! PDF extraction port. The production adapter runs PDFium in a separate worker process.
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::application::jobs::control::JobControl;
use crate::domain::document::FailureKind;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ExtractedMetadata {
    pub page_count: u32,
    pub title: Option<String>,
    pub author: Option<String>,
    pub creation_date: Option<String>,
}

/// Bounding box of a text span in PDF user space (points, origin bottom-left, unrotated page).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct SpanBox {
    /// Character offsets into the page text (UTF-8 char indices, end exclusive).
    pub start: u32,
    pub end: u32,
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExtractedPage {
    pub index: u32,
    pub width: f32,
    pub height: f32,
    /// Clockwise rotation in degrees: 0, 90, 180 or 270.
    pub rotation: u32,
    pub label: Option<String>,
    pub text: String,
    pub spans: Vec<SpanBox>,
    pub image_objects: u32,
}

#[derive(Debug, Clone)]
pub enum ExtractEvent {
    Meta(ExtractedMetadata),
    Page(ExtractedPage),
}

/// What the pipeline wants after handling an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Pause,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractOutcome {
    Completed,
    Paused,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct ExtractFailure {
    pub kind: FailureKind,
    pub detail: String,
}

pub trait PdfExtractor: Send + Sync {
    /// Whether the native PDF engine is installed next to the application.
    fn is_available(&self) -> bool;

    /// Extracts metadata and pages starting at `start_page`, calling `on_event` for each.
    /// Implementations must stop promptly when `control` requests pause or cancel.
    fn extract(
        &self,
        file: &Path,
        start_page: u32,
        control: &JobControl,
        on_event: &mut dyn FnMut(ExtractEvent) -> Flow,
    ) -> Result<ExtractOutcome, ExtractFailure>;
}
