//! Framed protocol between the app and the PDF worker process.
//! One JSON request on stdin; newline-delimited JSON messages on stdout.
use std::io::{self, BufRead};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::application::ports::{ExtractedMetadata, ExtractedPage};

pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
/// Largest single message (one page with its spans). Dense pages stay well below this.
pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkerRequest {
    pub protocol: u32,
    pub pdf_path: PathBuf,
    pub pdfium_path: PathBuf,
    pub start_page: u32,
}

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerMessage {
    Meta(ExtractedMetadata),
    Page(ExtractedPage),
    Done,
    Error { kind: String, message: String },
}

/// Reads one `\n`-terminated line without ever buffering more than `max` bytes.
/// Returns `Ok(None)` at EOF.
pub fn read_line_bounded<R: BufRead>(reader: &mut R, max: usize, out: &mut Vec<u8>) -> io::Result<Option<usize>> {
    out.clear();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(if out.is_empty() { None } else { Some(out.len()) });
        }
        match available.iter().position(|b| *b == b'\n') {
            Some(i) => {
                if out.len() + i > max {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
                }
                out.extend_from_slice(&available[..i]);
                reader.consume(i + 1);
                return Ok(Some(out.len()));
            }
            None => {
                let n = available.len();
                if out.len() + n > max {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
                }
                out.extend_from_slice(available);
                reader.consume(n);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_reader_handles_split_lines_and_limits() {
        let data = b"{\"a\":1}\n{\"b\":2}\npartial";
        let mut r = io::BufReader::with_capacity(3, &data[..]);
        let mut buf = Vec::new();
        assert_eq!(read_line_bounded(&mut r, 100, &mut buf).unwrap(), Some(7));
        assert_eq!(buf, b"{\"a\":1}");
        read_line_bounded(&mut r, 100, &mut buf).unwrap();
        assert_eq!(buf, b"{\"b\":2}");
        assert_eq!(read_line_bounded(&mut r, 100, &mut buf).unwrap(), Some(7));
        assert_eq!(read_line_bounded(&mut r, 100, &mut buf).unwrap(), None);

        let mut r = io::BufReader::new(&b"0123456789\n"[..]);
        assert!(read_line_bounded(&mut r, 5, &mut buf).is_err());
    }

    #[test]
    fn message_roundtrip() {
        let m = WorkerMessage::Error { kind: "corrupt".into(), message: "x".into() };
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.contains("\"type\":\"error\""));
        let back: WorkerMessage = serde_json::from_str(&s).unwrap();
        assert!(matches!(back, WorkerMessage::Error { .. }));
    }
}
