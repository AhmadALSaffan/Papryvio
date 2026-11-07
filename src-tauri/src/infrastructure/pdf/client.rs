//! Production `PdfExtractor`: launches the worker process, enforces time and output
//! limits, and kills it on cancel, pause or hang.
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::application::jobs::{JobControl, Signal};
use crate::application::ports::{ExtractEvent, ExtractFailure, ExtractOutcome, Flow, PdfExtractor};
use crate::domain::document::FailureKind;

use super::protocol::{read_line_bounded, WorkerMessage, WorkerRequest, MAX_MESSAGE_BYTES, PROTOCOL_VERSION};

pub struct WorkerPdfExtractor {
    exe: PathBuf,
    pdfium: PathBuf,
    worker_flag: &'static str,
    /// Longest silence allowed while opening the file or between two pages.
    pub idle_timeout: Duration,
}

impl WorkerPdfExtractor {
    pub fn new(exe: PathBuf, pdfium: PathBuf, worker_flag: &'static str) -> Self {
        Self { exe, pdfium, worker_flag, idle_timeout: Duration::from_secs(90) }
    }
}

const KILL_NONE: u8 = 0;
const KILL_CANCEL: u8 = 1;
const KILL_TIMEOUT: u8 = 2;

fn failure(kind: FailureKind, detail: impl Into<String>) -> ExtractFailure {
    ExtractFailure { kind, detail: detail.into() }
}

fn kill(child: &Mutex<Child>) {
    if let Ok(mut c) = child.lock() {
        let _ = c.kill();
    }
}

impl PdfExtractor for WorkerPdfExtractor {
    fn is_available(&self) -> bool {
        self.pdfium.is_file()
    }

    fn extract(
        &self,
        file: &Path,
        start_page: u32,
        control: &JobControl,
        on_event: &mut dyn FnMut(ExtractEvent) -> Flow,
    ) -> Result<ExtractOutcome, ExtractFailure> {
        let mut cmd = Command::new(&self.exe);
        cmd.arg(self.worker_flag).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| failure(FailureKind::WorkerCrashed, format!("spawn: {e}")))?;

        let request = WorkerRequest {
            protocol: PROTOCOL_VERSION,
            pdf_path: file.to_path_buf(),
            pdfium_path: self.pdfium.clone(),
            start_page,
        };
        {
            let mut stdin = child.stdin.take().ok_or_else(|| failure(FailureKind::WorkerCrashed, "no stdin"))?;
            let mut payload = serde_json::to_vec(&request).map_err(|e| failure(FailureKind::Unknown, e.to_string()))?;
            payload.push(b'\n');
            stdin.write_all(&payload).map_err(|e| failure(FailureKind::WorkerCrashed, format!("write request: {e}")))?;
        }
        let stdout = child.stdout.take().ok_or_else(|| failure(FailureKind::WorkerCrashed, "no stdout"))?;
        let child = Mutex::new(child);
        let last_activity = Mutex::new(Instant::now());
        let finished = AtomicBool::new(false);
        let kill_reason = AtomicU8::new(KILL_NONE);

        let result = std::thread::scope(|scope| {
            // Watchdog: enforces the idle timeout and reacts to cancel even mid-page.
            scope.spawn(|| {
                while !finished.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(100));
                    if control.signal() == Signal::Cancel {
                        kill_reason.store(KILL_CANCEL, Ordering::SeqCst);
                        kill(&child);
                        break;
                    }
                    let idle = last_activity.lock().map(|t| t.elapsed()).unwrap_or_default();
                    if idle > self.idle_timeout {
                        kill_reason.store(KILL_TIMEOUT, Ordering::SeqCst);
                        kill(&child);
                        break;
                    }
                }
            });

            let outcome = (|| {
                let mut reader = BufReader::with_capacity(256 * 1024, stdout);
                let mut line = Vec::new();
                let mut worker_error: Option<(String, String)> = None;
                loop {
                    let n = match read_line_bounded(&mut reader, MAX_MESSAGE_BYTES, &mut line) {
                        Ok(n) => n,
                        Err(e) => {
                            kill(&child);
                            return Err(failure(FailureKind::OutputTooLarge, e.to_string()));
                        }
                    };
                    let Some(_) = n else { break };
                    if let Ok(mut t) = last_activity.lock() {
                        *t = Instant::now();
                    }
                    let msg: WorkerMessage = match serde_json::from_slice(&line) {
                        Ok(m) => m,
                        Err(e) => {
                            kill(&child);
                            return Err(failure(FailureKind::WorkerCrashed, format!("bad message: {e}")));
                        }
                    };
                    let flow = match msg {
                        WorkerMessage::Meta(m) => on_event(ExtractEvent::Meta(m)),
                        WorkerMessage::Page(p) => on_event(ExtractEvent::Page(p)),
                        WorkerMessage::Done => return Ok(ExtractOutcome::Completed),
                        WorkerMessage::Error { kind, message } => {
                            worker_error = Some((kind, message));
                            continue;
                        }
                    };
                    match flow {
                        Flow::Continue => {}
                        Flow::Pause => {
                            kill(&child);
                            return Ok(ExtractOutcome::Paused);
                        }
                        Flow::Cancel => {
                            kill(&child);
                            return Ok(ExtractOutcome::Cancelled);
                        }
                    }
                }
                // EOF without Done.
                if let Some((kind, message)) = worker_error {
                    return Err(failure(FailureKind::parse(&kind), message));
                }
                match kill_reason.load(Ordering::SeqCst) {
                    KILL_CANCEL => Ok(ExtractOutcome::Cancelled),
                    KILL_TIMEOUT => Err(failure(FailureKind::Timeout, "worker stopped responding")),
                    _ => {
                        let status = child.lock().ok().and_then(|mut c| c.wait().ok());
                        Err(failure(FailureKind::WorkerCrashed, format!("worker exited early: {status:?}")))
                    }
                }
            })();
            finished.store(true, Ordering::SeqCst);
            outcome
        });

        // Always reap the process.
        if let Ok(mut c) = child.lock() {
            if !matches!(result, Ok(ExtractOutcome::Completed)) {
                let _ = c.kill();
            }
            let _ = c.wait();
        }
        result
    }
}
