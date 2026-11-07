//! Runs inside the separate worker process (`papryvio.exe --pdf-worker`).
//! Loading PDFium only here keeps a crash or hang in the native library away from the app.
use std::io::{self, BufReader, BufWriter, Write};

use pdfium_render::prelude::*;

use crate::application::ports::{ExtractedMetadata, ExtractedPage, SpanBox};
use crate::domain::text as textfix;

use super::protocol::{read_line_bounded, WorkerMessage, WorkerRequest, MAX_REQUEST_BYTES, PROTOCOL_VERSION};

fn send(out: &mut impl Write, msg: &WorkerMessage) -> io::Result<()> {
    serde_json::to_writer(&mut *out, msg)?;
    out.write_all(b"\n")?;
    out.flush()
}

fn error_kind(e: &PdfiumError) -> &'static str {
    match e {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError)
        | PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::SecurityError) => "password_protected",
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FormatError)
        | PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FileError) => "corrupt",
        _ => "unknown",
    }
}

pub fn run_worker() -> i32 {
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    match run(&mut out) {
        Ok(()) => 0,
        Err((kind, message)) => {
            let _ = send(&mut out, &WorkerMessage::Error { kind: kind.into(), message });
            2
        }
    }
}

fn run(out: &mut impl Write) -> Result<(), (&'static str, String)> {
    let mut input = BufReader::new(io::stdin().lock());
    let mut line = Vec::new();
    read_line_bounded(&mut input, MAX_REQUEST_BYTES, &mut line).map_err(|e| ("unknown", e.to_string()))?;
    let req: WorkerRequest = serde_json::from_slice(&line).map_err(|e| ("unknown", format!("bad request: {e}")))?;
    process(&req, out)
}

/// Extracts the requested document and writes protocol messages to `out`.
pub fn process(req: &WorkerRequest, out: &mut impl Write) -> Result<(), (&'static str, String)> {
    if req.protocol != PROTOCOL_VERSION {
        return Err(("unknown", "protocol mismatch".into()));
    }

    let pdfium = pdfium_instance(&req.pdfium_path)?;
    let doc = pdfium.load_pdf_from_file(&req.pdf_path, None).map_err(|e| (error_kind(&e), format!("{e:?}")))?;

    let pages = doc.pages();
    let page_count = pages.len().max(0) as u32;
    let metadata = doc.metadata();
    let tag = |t: PdfDocumentMetadataTagType| metadata.get(t).map(|v| v.value().to_string()).filter(|v| !v.trim().is_empty());
    let meta = ExtractedMetadata {
        page_count,
        title: tag(PdfDocumentMetadataTagType::Title),
        author: tag(PdfDocumentMetadataTagType::Author),
        creation_date: tag(PdfDocumentMetadataTagType::CreationDate),
    };
    send(out, &WorkerMessage::Meta(meta)).map_err(|e| ("unknown", e.to_string()))?;

    // Whether this document stores Arabic letter-reversed; pages with too little Arabic to tell
    // follow the last clear verdict.
    let mut arabic_reversed = false;
    for index in req.start_page..page_count {
        let page = pages.get(index as PdfPageIndex).map_err(|e| ("corrupt", format!("page {index}: {e:?}")))?;
        let mut extracted = extract_page(index, &page);
        if let Some(v) = textfix::arabic_reversed(&extracted.text) {
            arabic_reversed = v;
        }
        if arabic_reversed {
            // Same char count, so span offsets stay valid.
            extracted.text = textfix::unreverse_arabic(&extracted.text);
        }
        send(out, &WorkerMessage::Page(extracted)).map_err(|e| ("unknown", e.to_string()))?;
    }
    send(out, &WorkerMessage::Done).map_err(|e| ("unknown", e.to_string()))?;
    Ok(())
}

/// PDFium bindings are process-global; bind once and reuse.
fn pdfium_instance(path: &std::path::Path) -> Result<&'static Pdfium, (&'static str, String)> {
    static PDFIUM: std::sync::OnceLock<Pdfium> = std::sync::OnceLock::new();
    if let Some(p) = PDFIUM.get() {
        return Ok(p);
    }
    let bindings = Pdfium::bind_to_library(path).map_err(|e| ("pdf_engine_missing", format!("{e:?}")))?;
    Ok(PDFIUM.get_or_init(|| Pdfium::new(bindings)))
}

fn rotation_degrees(page: &PdfPage<'_>) -> u32 {
    match page.rotation() {
        Ok(PdfPageRenderRotation::Degrees90) => 90,
        Ok(PdfPageRenderRotation::Degrees180) => 180,
        Ok(PdfPageRenderRotation::Degrees270) => 270,
        _ => 0,
    }
}

fn extract_page(index: u32, page: &PdfPage<'_>) -> ExtractedPage {
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut char_len: u32 = 0;

    if let Ok(page_text) = page.text() {
        let mut prev_bottom: Option<f32> = None;
        for segment in page_text.segments().iter() {
            let raw = segment.text();
            let seg = raw.trim_matches(|c: char| c == '\r' || c == '\n');
            if seg.trim().is_empty() {
                continue;
            }
            let b = segment.bounds();
            if let Some(pb) = prev_bottom {
                // A segment that starts below the previous one begins a new line.
                let sep = if b.top().value < pb + 1.0 { '\n' } else { ' ' };
                text.push(sep);
                char_len += 1;
            }
            let start = char_len;
            let seg_chars = seg.chars().count() as u32;
            text.push_str(seg);
            char_len += seg_chars;
            spans.push(SpanBox {
                start,
                end: char_len,
                left: b.left().value,
                bottom: b.bottom().value,
                right: b.right().value,
                top: b.top().value,
            });
            prev_bottom = Some(b.bottom().value);
        }
        // Fallback when segment geometry is unavailable but text exists.
        if text.trim().is_empty() {
            let all = page_text.all();
            if !all.trim().is_empty() {
                text = all;
                spans.clear();
            }
        }
    }

    let image_objects = if text.trim().is_empty() {
        page.objects().iter().filter(|o| o.object_type() == PdfPageObjectType::Image).count() as u32
    } else {
        0
    };

    ExtractedPage {
        index,
        width: page.width().value,
        height: page.height().value,
        rotation: rotation_degrees(page),
        label: page.label().map(str::to_string).filter(|l| !l.is_empty()),
        text,
        spans,
        image_objects,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn parse(out: &[u8]) -> Vec<WorkerMessage> {
        out.split(|b| *b == b'\n').filter(|l| !l.is_empty()).map(|l| serde_json::from_slice(l).unwrap()).collect()
    }

    /// Uses the real PDFium library from `resources/pdfium` (run `npm run setup:pdfium` first).
    #[test]
    fn extracts_real_pdf_with_pdfium() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let pdfium = root.join("resources/pdfium").join(Pdfium::pdfium_platform_library_name());
        if !pdfium.is_file() {
            eprintln!("skipping: {} not found", pdfium.display());
            return;
        }
        let fixtures = root.join("tests/fixtures");
        let run = |name: &str, start_page: u32| {
            let req = WorkerRequest { protocol: PROTOCOL_VERSION, pdf_path: fixtures.join(name), pdfium_path: pdfium.clone(), start_page };
            let mut out = Vec::new();
            let res = process(&req, &mut out);
            (res, parse(&out))
        };

        let (res, msgs) = run("two-pages.pdf", 0);
        res.unwrap();
        let WorkerMessage::Meta(meta) = &msgs[0] else { panic!("meta first") };
        assert_eq!(meta.page_count, 2);
        assert_eq!(meta.title.as_deref(), Some("Evidence Linking in Local Research Assistants"));
        assert_eq!(meta.author.as_deref(), Some("Test Author"));
        let WorkerMessage::Page(p1) = &msgs[1] else { panic!("page") };
        assert_eq!(p1.index, 0);
        assert!(p1.text.contains("Retrieval augmented generation"), "text: {}", p1.text);
        assert!(!p1.spans.is_empty());
        let s = p1.spans[0];
        assert!(s.left >= 70.0 && s.top <= 735.0 && s.top > s.bottom, "span {s:?}");
        let span_text: String = p1.text.chars().skip(s.start as usize).take((s.end - s.start) as usize).collect();
        assert!(span_text.starts_with("Evidence"), "span text {span_text}");
        let WorkerMessage::Page(p2) = &msgs[2] else { panic!("page 2") };
        assert_eq!(p2.rotation, 90);
        assert!(p2.text.contains("4.1 points"));
        assert!(matches!(msgs.last(), Some(WorkerMessage::Done)));

        // Resuming from a checkpoint only emits the remaining pages.
        let (_, msgs) = run("two-pages.pdf", 1);
        let pages: Vec<_> = msgs.iter().filter_map(|m| if let WorkerMessage::Page(p) = m { Some(p.index) } else { None }).collect();
        assert_eq!(pages, vec![1]);

        // No text layer.
        let (_, msgs) = run("blank-scan.pdf", 0);
        for m in &msgs {
            if let WorkerMessage::Page(p) = m {
                assert!(p.text.trim().is_empty());
            }
        }

        // Not a PDF.
        let (res, _) = run("README.md", 0);
        assert_eq!(res.unwrap_err().0, "corrupt");
    }
}

