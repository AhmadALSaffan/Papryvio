//! Anthropic Messages API (API key): GET /v1/models, POST /v1/messages (SSE). No embeddings.
use std::time::Duration;

use serde_json::{json, Value};

use crate::application::error::{AppError, AppResult, ErrorCode};
use crate::application::jobs::JobControl;
use crate::application::ports::{ChatOutcome, ChatRequest, ModelInfo, ModelProvider, ProviderKind};

use super::http::{agent, json_or_err, net_err, read_body, sse_data, status_err, stream_lines};

const VERSION: &str = "2023-06-01";

pub struct Anthropic {
    base: String,
    key: String,
}

impl Anthropic {
    pub fn new(base: &str, key: String) -> Self {
        Self { base: base.trim_end_matches('/').to_string(), key }
    }
}

impl ModelProvider for Anthropic {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Anthropic
    }

    fn list_models(&self) -> AppResult<Vec<ModelInfo>> {
        let a = agent(Duration::from_secs(10), Duration::from_secs(30));
        let resp = a
            .get(format!("{}/v1/models?limit=100", self.base))
            .header("x-api-key", &self.key)
            .header("anthropic-version", VERSION)
            .call()
            .map_err(|e| net_err("models", e))?;
        let v = json_or_err("models", resp)?;
        Ok(v.get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|m| {
                let id = m.get("id").and_then(Value::as_str)?.to_string();
                Some(ModelInfo {
                    label: m.get("display_name").and_then(Value::as_str).unwrap_or(&id).to_string(),
                    id,
                    digest: None,
                    size_bytes: None,
                    can_generate: true,
                    can_embed: false,
                })
            })
            .collect())
    }

    fn supports_embeddings(&self) -> bool {
        false
    }

    fn embed(&self, _model: &str, _inputs: &[String]) -> AppResult<Vec<Vec<f32>>> {
        Err(AppError::new(ErrorCode::Unsupported, "this provider has no embedding models"))
    }

    fn chat(&self, model: &str, req: &ChatRequest, control: &JobControl, on_delta: &mut dyn FnMut(&str)) -> AppResult<ChatOutcome> {
        let a = agent(Duration::from_secs(10), Duration::from_secs(300));
        let mut resp = a
            .post(format!("{}/v1/messages", self.base))
            .header("x-api-key", &self.key)
            .header("anthropic-version", VERSION)
            .send_json(json!({
                "model": model,
                "max_tokens": req.max_output_tokens,
                "stream": true,
                "system": req.system,
                "messages": [{ "role": "user", "content": req.user }]
            }))
            .map_err(|e| net_err("messages", e))?;
        let status = resp.status().as_u16();
        if status >= 300 {
            let b = read_body(&mut resp).unwrap_or_default();
            return Err(status_err("messages", status, &b));
        }
        let mut out = ChatOutcome::default();
        let cancelled = stream_lines(resp, control, |line| {
            let Some(data) = sse_data(line) else { return Ok(true) };
            let v: Value = serde_json::from_str(data).map_err(|e| AppError::new(ErrorCode::Unsupported, format!("messages stream: {e}")))?;
            match v.get("type").and_then(Value::as_str) {
                Some("content_block_delta") => {
                    if let Some(piece) = v.pointer("/delta/text").and_then(Value::as_str) {
                        out.text.push_str(piece);
                        on_delta(piece);
                    }
                }
                Some("message_delta") => out.output_tokens = v.pointer("/usage/output_tokens").and_then(Value::as_u64),
                Some("message_stop") => return Ok(false),
                Some("error") => return Err(AppError::new(ErrorCode::Unsupported, format!("messages: {}", v["error"])).retryable()),
                _ => {}
            }
            Ok(true)
        })?;
        out.cancelled = cancelled;
        Ok(out)
    }
}
