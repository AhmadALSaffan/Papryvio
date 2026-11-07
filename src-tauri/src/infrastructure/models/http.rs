//! Shared HTTP plumbing for model providers: bounded agents, no redirects, streaming
//! line readers for NDJSON and Server-Sent Events, and typed error mapping.
use std::io::{BufReader, Read};
use std::time::Duration;

use ureq::Agent;

use crate::application::error::{AppError, AppResult, ErrorCode};
use crate::application::jobs::{JobControl, Signal};
use crate::infrastructure::pdf::protocol::read_line_bounded;

pub const MAX_LINE: usize = 4 * 1024 * 1024;
pub const MAX_BODY: u64 = 32 * 1024 * 1024;

/// `connect` bounds connection setup; `read` bounds silence between bytes (model load
/// time on first use can be long, so it is generous for local providers).
pub fn agent(connect: Duration, read: Duration) -> Agent {
    Agent::config_builder()
        .timeout_connect(Some(connect))
        .timeout_recv_response(Some(read))
        .timeout_recv_body(Some(read))
        .max_redirects(0) // keep requests on the configured host
        .http_status_as_error(false)
        .user_agent("Papryvio/0.1")
        .build()
        .into()
}

pub fn net_err(context: &str, e: ureq::Error) -> AppError {
    let msg = e.to_string();
    let code = if msg.contains("timed out") || msg.contains("timeout") { "timeout" } else { "unreachable" };
    AppError::new(ErrorCode::Unsupported, format!("{context}: {code}: {msg}")).retryable()
}

/// Turns a non-2xx response into an error with the provider's message (secrets are never echoed).
pub fn status_err(context: &str, status: u16, body: &str) -> AppError {
    let detail: String = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message").or(Some(e)))
                .and_then(|m| m.as_str().map(str::to_string))
                .or_else(|| v.get("message").and_then(|m| m.as_str().map(str::to_string)))
        })
        .unwrap_or_else(|| body.chars().take(300).collect());
    let kind = match status {
        401 | 403 => "unauthorized",
        404 => "not_found",
        408 | 504 => "timeout",
        429 => "rate_limited",
        500..=599 => "server_error",
        _ => "rejected",
    };
    let retryable = matches!(status, 408 | 429 | 500..=599);
    let e = AppError::new(ErrorCode::Unsupported, format!("{context}: {kind} ({status}): {detail}"));
    if retryable {
        e.retryable()
    } else {
        e
    }
}

pub fn read_body(resp: &mut ureq::http::Response<ureq::Body>) -> AppResult<String> {
    resp.body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_string()
        .map_err(|e| AppError::new(ErrorCode::Unsupported, format!("read body: {e}")).retryable())
}

pub fn json_or_err(context: &str, mut resp: ureq::http::Response<ureq::Body>) -> AppResult<serde_json::Value> {
    let status = resp.status().as_u16();
    let body = read_body(&mut resp)?;
    if !(200..300).contains(&status) {
        return Err(status_err(context, status, &body));
    }
    serde_json::from_str(&body).map_err(|e| AppError::new(ErrorCode::Unsupported, format!("{context}: invalid JSON: {e}")))
}

/// Calls `on_line` for each line of a streamed body until it returns false, the stream ends,
/// or the job is cancelled. Returns whether it was cancelled.
pub fn stream_lines(
    resp: ureq::http::Response<ureq::Body>,
    control: &JobControl,
    mut on_line: impl FnMut(&str) -> AppResult<bool>,
) -> AppResult<bool> {
    let reader: Box<dyn Read + Send> = Box::new(resp.into_body().into_reader());
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        if control.signal() == Signal::Cancel {
            return Ok(true); // dropping the reader closes the request
        }
        match read_line_bounded(&mut reader, MAX_LINE, &mut buf) {
            Ok(None) => return Ok(false),
            Ok(Some(_)) => {
                let line = String::from_utf8_lossy(&buf);
                let line = line.trim_end_matches('\r');
                if line.is_empty() {
                    continue;
                }
                if !on_line(line)? {
                    return Ok(false);
                }
            }
            Err(e) => return Err(AppError::new(ErrorCode::Unsupported, format!("stream interrupted: {e}")).retryable()),
        }
    }
}

/// Server-Sent Events: yields the `data:` payload of each event (multi-line data joined).
pub fn sse_data(line: &str) -> Option<&str> {
    line.strip_prefix("data:").map(|d| d.trim_start())
}
