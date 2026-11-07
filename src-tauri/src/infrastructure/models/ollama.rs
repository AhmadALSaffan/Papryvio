//! Ollama on loopback: GET /api/tags, POST /api/show, POST /api/embed, POST /api/chat (NDJSON stream).
use std::time::Duration;

use serde_json::{json, Value};

use crate::application::error::{AppError, AppResult, ErrorCode};
use crate::application::jobs::JobControl;
use crate::application::ports::{ChatOutcome, ChatRequest, ModelInfo, ModelProvider, ProviderKind};

use super::http::{agent, json_or_err, net_err, status_err, read_body, stream_lines};

pub struct Ollama {
    base: String,
}

impl Ollama {
    pub fn new(base: &str) -> Self {
        Self { base: base.trim_end_matches('/').to_string() }
    }
}

fn unreachable_hint(e: AppError) -> AppError {
    if e.detail.contains("unreachable") {
        AppError::new(ErrorCode::Unsupported, format!("ollama_not_running: {}", e.detail)).retryable()
    } else {
        e
    }
}

impl ModelProvider for Ollama {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Ollama
    }

    fn list_models(&self) -> AppResult<Vec<ModelInfo>> {
        let a = agent(Duration::from_secs(3), Duration::from_secs(15));
        let resp = a.get(format!("{}/api/tags", self.base)).call().map_err(|e| unreachable_hint(net_err("ollama tags", e)))?;
        let v = json_or_err("ollama tags", resp)?;
        let mut out = Vec::new();
        for m in v.get("models").and_then(Value::as_array).cloned().unwrap_or_default() {
            let name = m.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
            if name.is_empty() {
                continue;
            }
            // Cloud-hosted Ollama models leave the computer; they are not offered as local models.
            if name.contains(":cloud") || name.ends_with("-cloud") || m.get("remote_host").is_some() {
                continue;
            }
            let caps: Vec<String> = a
                .post(format!("{}/api/show", self.base))
                .send_json(json!({ "model": name }))
                .ok()
                .and_then(|r| json_or_err("ollama show", r).ok())
                .and_then(|s| s.get("capabilities").and_then(Value::as_array).cloned())
                .map(|c| c.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let family = m.pointer("/details/family").and_then(Value::as_str).unwrap_or("").to_lowercase();
            let looks_embed = name.contains("embed") || family.contains("bert");
            let can_embed = if caps.is_empty() { looks_embed } else { caps.iter().any(|c| c == "embedding") };
            let can_generate = if caps.is_empty() { !looks_embed } else { caps.iter().any(|c| c == "completion") };
            out.push(ModelInfo {
                label: name.clone(),
                id: name,
                digest: m.get("digest").and_then(Value::as_str).map(str::to_string),
                size_bytes: m.get("size").and_then(Value::as_u64),
                can_generate,
                can_embed,
            });
        }
        Ok(out)
    }

    fn supports_embeddings(&self) -> bool {
        true
    }

    fn embed(&self, model: &str, inputs: &[String]) -> AppResult<Vec<Vec<f32>>> {
        let a = agent(Duration::from_secs(3), Duration::from_secs(300));
        // truncate=false: an over-long input is an error, never silently cut.
        let resp = a
            .post(format!("{}/api/embed", self.base))
            .send_json(json!({ "model": model, "input": inputs, "truncate": false }))
            .map_err(|e| unreachable_hint(net_err("ollama embed", e)))?;
        let v = json_or_err("ollama embed", resp)?;
        let vecs = v.get("embeddings").and_then(Value::as_array).ok_or_else(|| AppError::new(ErrorCode::Unsupported, "ollama embed: no embeddings"))?;
        Ok(vecs
            .iter()
            .map(|e| e.as_array().map(|a| a.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect()).unwrap_or_default())
            .collect())
    }

    fn chat(&self, model: &str, req: &ChatRequest, control: &JobControl, on_delta: &mut dyn FnMut(&str)) -> AppResult<ChatOutcome> {
        let a = agent(Duration::from_secs(3), Duration::from_secs(600));
        let mut body = json!({
            "model": model,
            "stream": true,
            "messages": [
                { "role": "system", "content": req.system },
                { "role": "user", "content": req.user }
            ],
            "options": { "num_predict": req.max_output_tokens, "temperature": 0.1, "num_ctx": 16384 }
        });
        if req.json {
            body["format"] = json!("json");
        }
        // Answers are short and structured: reasoning ("thinking") output is turned off.
        body["think"] = json!(false);
        let mut resp = a.post(format!("{}/api/chat", self.base)).send_json(&body).map_err(|e| unreachable_hint(net_err("ollama chat", e)))?;
        let mut status = resp.status().as_u16();
        if status == 400 {
            let b = read_body(&mut resp).unwrap_or_default();
            if !b.contains("think") {
                return Err(status_err("ollama chat", status, &b));
            }
            // Older or non-thinking models reject the flag: retry without it.
            body.as_object_mut().map(|o| o.remove("think"));
            resp = a.post(format!("{}/api/chat", self.base)).send_json(&body).map_err(|e| unreachable_hint(net_err("ollama chat", e)))?;
            status = resp.status().as_u16();
        }
        if status >= 300 {
            let b = read_body(&mut resp).unwrap_or_default();
            return Err(status_err("ollama chat", status, &b));
        }
        let mut out = ChatOutcome::default();
        let cancelled = stream_lines(resp, control, |line| {
            let v: Value = serde_json::from_str(line).map_err(|e| AppError::new(ErrorCode::Unsupported, format!("ollama stream: {e}")))?;
            if let Some(err) = v.get("error").and_then(Value::as_str) {
                return Err(AppError::new(ErrorCode::Unsupported, format!("ollama: {err}")).retryable());
            }
            if let Some(piece) = v.pointer("/message/content").and_then(Value::as_str) {
                if !piece.is_empty() {
                    out.text.push_str(piece);
                    on_delta(piece);
                }
            }
            if v.get("done").and_then(Value::as_bool) == Some(true) {
                out.load_ms = v.get("load_duration").and_then(Value::as_u64).map(|ns| ns / 1_000_000);
                out.output_tokens = v.get("eval_count").and_then(Value::as_u64);
                return Ok(false);
            }
            Ok(true)
        })?;
        out.cancelled = cancelled;
        Ok(out)
    }
}
