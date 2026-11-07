//! OpenAI-compatible APIs (OpenAI, OpenRouter, GitHub Models, LM Studio, …):
//! GET /models, POST /embeddings, POST /chat/completions (SSE stream).
use std::time::Duration;

use serde_json::{json, Value};

use crate::application::error::{AppError, AppResult, ErrorCode};
use crate::application::jobs::JobControl;
use crate::application::ports::{ChatOutcome, ChatRequest, ModelInfo, ModelProvider, ProviderKind};

use super::http::{agent, json_or_err, net_err, read_body, sse_data, status_err, stream_lines};

pub struct OpenAiCompatible {
    base: String,
    key: String,
}

impl OpenAiCompatible {
    pub fn new(base: &str, key: String) -> Self {
        Self { base: base.trim_end_matches('/').to_string(), key }
    }
    /// A local gateway (e.g. LiteLLM without a master key) may need no key: no header is sent then.
    fn authed<B>(&self, r: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        if self.key.is_empty() {
            r
        } else {
            r.header("Authorization", format!("Bearer {}", self.key))
        }
    }
}

impl ModelProvider for OpenAiCompatible {
    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenaiCompatible
    }

    fn list_models(&self) -> AppResult<Vec<ModelInfo>> {
        let a = agent(Duration::from_secs(10), Duration::from_secs(30));
        let (url, github) = models_url(&self.base);
        let mut req = self.authed(a.get(url));
        if github {
            req = req.header("Accept", "application/vnd.github+json");
        }
        let resp = req.call().map_err(|e| net_err("models", e))?;
        let v = json_or_err("models", resp)?;
        Ok(parse_models(&v))
    }

    fn supports_embeddings(&self) -> bool {
        true
    }

    fn embed(&self, model: &str, inputs: &[String]) -> AppResult<Vec<Vec<f32>>> {
        let a = agent(Duration::from_secs(10), Duration::from_secs(120));
        let resp = self
            .authed(a.post(format!("{}/embeddings", self.base)))
            .send_json(json!({ "model": model, "input": inputs }))
            .map_err(|e| net_err("embeddings", e))?;
        let v = json_or_err("embeddings", resp)?;
        let mut data = v.get("data").and_then(Value::as_array).cloned().ok_or_else(|| AppError::new(ErrorCode::Unsupported, "embeddings: no data"))?;
        data.sort_by_key(|d| d.get("index").and_then(Value::as_u64).unwrap_or(0));
        Ok(data
            .iter()
            .map(|d| d.get("embedding").and_then(Value::as_array).map(|e| e.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect()).unwrap_or_default())
            .collect())
    }

    fn chat(&self, model: &str, req: &ChatRequest, control: &JobControl, on_delta: &mut dyn FnMut(&str)) -> AppResult<ChatOutcome> {
        let a = agent(Duration::from_secs(10), Duration::from_secs(300));
        let mut body = json!({
            "model": model,
            "stream": true,
            "messages": [
                { "role": "system", "content": req.system },
                { "role": "user", "content": req.user }
            ],
            "max_tokens": req.max_output_tokens
        });
        if req.json {
            body["response_format"] = json!({ "type": "json_object" });
        }
        let mut resp = self
            .authed(a.post(format!("{}/chat/completions", self.base)))
            .send_json(body)
            .map_err(|e| net_err("chat", e))?;
        let status = resp.status().as_u16();
        if status >= 300 {
            let b = read_body(&mut resp).unwrap_or_default();
            return Err(status_err("chat", status, &b));
        }
        let mut out = ChatOutcome::default();
        let cancelled = stream_lines(resp, control, |line| {
            let Some(data) = sse_data(line) else { return Ok(true) };
            if data == "[DONE]" {
                return Ok(false);
            }
            let v: Value = serde_json::from_str(data).map_err(|e| AppError::new(ErrorCode::Unsupported, format!("chat stream: {e}")))?;
            if let Some(err) = v.get("error") {
                return Err(AppError::new(ErrorCode::Unsupported, format!("chat: {err}")).retryable());
            }
            if let Some(piece) = v.pointer("/choices/0/delta/content").and_then(Value::as_str) {
                out.text.push_str(piece);
                on_delta(piece);
            }
            if let Some(t) = v.pointer("/usage/completion_tokens").and_then(Value::as_u64) {
                out.output_tokens = Some(t);
            }
            Ok(true)
        })?;
        out.cancelled = cancelled;
        Ok(out)
    }
}

/// Where a service lists its models. GitHub Models keeps its catalogue outside the inference path.
fn models_url(base: &str) -> (String, bool) {
    if base.starts_with("https://models.github.ai/") {
        ("https://models.github.ai/catalog/models".to_string(), true)
    } else {
        (format!("{base}/models"), false)
    }
}

/// Model kinds that cannot write text answers or embeddings (image, audio, video, safety, reranking…).
const NOT_TEXT: &[&str] = &[
    "whisper", "tts", "dall-e", "gpt-image", "imagen", "image-gen", "veo", "sora", "lyria", "video", "audio", "realtime",
    "transcribe", "speech", "moderation", "rerank", "guard", "detector", "ocr", "deplot", "kosmos", "babbage", "davinci",
    "diffusion", "flux", "stable-", "aqa", "clip",
];

fn strings_at(m: &Value, pointers: &[&str]) -> Vec<String> {
    pointers
        .iter()
        .filter_map(|p| m.pointer(p).and_then(Value::as_array))
        .flat_map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_lowercase)))
        .collect()
}

/// Reads `/models` from any OpenAI-compatible service (OpenAI-style `{data:[…]}`, a bare array, or Cohere's
/// `{models:[…]}`) and tells chat models from embedding models using the metadata each service provides.
pub(crate) fn parse_models(v: &Value) -> Vec<ModelInfo> {
    let list = v
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| v.get("models").and_then(Value::as_array))
        .or_else(|| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out: Vec<ModelInfo> = list
        .iter()
        .filter_map(|m| {
            let id = m.get("id").or_else(|| m.get("name")).and_then(Value::as_str)?.to_string();
            let lower = id.to_lowercase();
            let outputs = strings_at(m, &["/architecture/output_modalities", "/supported_output_modalities", "/output_modalities"]);
            let endpoints = strings_at(m, &["/endpoints"]);
            let kind = m.get("type").and_then(Value::as_str).unwrap_or("").to_lowercase();
            let (can_generate, can_embed) = if !outputs.is_empty() {
                (outputs.iter().any(|o| o == "text"), outputs.iter().any(|o| o.starts_with("embedding")))
            } else if !endpoints.is_empty() {
                (endpoints.iter().any(|e| e == "chat"), endpoints.iter().any(|e| e == "embed"))
            } else if !kind.is_empty() && kind != "model" {
                (matches!(kind.as_str(), "chat" | "language" | "code"), kind.starts_with("embedding"))
            } else if let Some(chat) = m.pointer("/capabilities/completion_chat").and_then(Value::as_bool) {
                (chat, lower.contains("embed"))
            } else {
                let embed = lower.contains("embed");
                (!embed && !NOT_TEXT.iter().any(|k| lower.contains(k)), embed)
            };
            let (can_generate, can_embed) = if NOT_TEXT.iter().any(|k| lower.contains(k)) { (false, false) } else { (can_generate, can_embed) };
            if !can_generate && !can_embed {
                return None;
            }
            let label = m
                .get("display_name")
                .or_else(|| m.get("name").filter(|n| n.as_str() != Some(id.as_str())))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| id.trim_start_matches("models/").to_string());
            Some(ModelInfo { label, id, digest: None, size_bytes: None, can_generate, can_embed })
        })
        .collect();
    out.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    out.dedup_by(|a, b| a.id == b.id);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: Value) -> (Vec<String>, Vec<String>) {
        let m = parse_models(&v);
        (
            m.iter().filter(|x| x.can_generate).map(|x| x.id.clone()).collect(),
            m.iter().filter(|x| x.can_embed).map(|x| x.id.clone()).collect(),
        )
    }

    #[test]
    fn openai_style_list_drops_non_text_models() {
        let (gen, emb) = ids(json!({ "data": [
            { "id": "gpt-4.1-mini" }, { "id": "text-embedding-3-small" }, { "id": "whisper-1" },
            { "id": "gpt-image-1" }, { "id": "omni-moderation-latest" }, { "id": "tts-1" }, { "id": "babbage-002" }
        ]}));
        assert_eq!(gen, vec!["gpt-4.1-mini"]);
        assert_eq!(emb, vec!["text-embedding-3-small"]);
    }

    #[test]
    fn github_catalog_uses_output_modalities() {
        let (gen, emb) = ids(json!([
            { "id": "openai/gpt-4.1", "name": "OpenAI GPT-4.1", "supported_output_modalities": ["text"] },
            { "id": "openai/text-embedding-3-large", "name": "Embedding 3 large", "supported_output_modalities": ["embeddings"] }
        ]));
        assert_eq!(gen, vec!["openai/gpt-4.1"]);
        assert_eq!(emb, vec!["openai/text-embedding-3-large"]);
        assert_eq!(models_url("https://models.github.ai/inference").0, "https://models.github.ai/catalog/models");
        assert_eq!(models_url("https://api.groq.com/openai/v1").0, "https://api.groq.com/openai/v1/models");
    }

    #[test]
    fn typed_and_capability_lists() {
        // Together (`type`), OpenRouter / Hugging Face (`architecture`), Mistral (`capabilities`), Cohere (`endpoints`).
        let (gen, emb) = ids(json!({ "data": [
            { "id": "meta-llama/Llama-3.3-70B", "type": "chat" },
            { "id": "BAAI/bge-base-en", "type": "embedding" },
            { "id": "black-forest-labs/FLUX.1", "type": "image" },
            { "id": "qwen/qwen3", "architecture": { "output_modalities": ["text"] } },
            { "id": "mistral-small-latest", "capabilities": { "completion_chat": true } },
            { "id": "models/gemini-2.5-flash" }
        ]}));
        assert_eq!(gen, vec!["models/gemini-2.5-flash", "meta-llama/Llama-3.3-70B", "mistral-small-latest", "qwen/qwen3"]);
        assert_eq!(emb, vec!["BAAI/bge-base-en"]);
        let (gen, emb) = ids(json!({ "models": [
            { "name": "command-a", "endpoints": ["chat", "generate"] },
            { "name": "embed-v4.0", "endpoints": ["embed"] },
            { "name": "rerank-v3.5", "endpoints": ["rerank"] }
        ]}));
        assert_eq!(gen, vec!["command-a"]);
        assert_eq!(emb, vec!["embed-v4.0"]);
    }
}
