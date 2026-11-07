//! PDF adapter: PDFium (via pdfium-render) running in an isolated worker process.
pub mod client;
pub mod protocol;
pub mod worker;

pub use client::WorkerPdfExtractor;
