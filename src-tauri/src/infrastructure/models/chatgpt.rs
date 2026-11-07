//! A ChatGPT plan as the generation model, via the public Responses API and the access
//! token from "Sign in with ChatGPT". Plan-usage rules (OpenAI token-sharing docs):
//! every request is `store: false, stream: true`; the system prompt goes in `instructions`;
//! sampling/length parameters are not sent. No embeddings on a plan.
use std::time::Duration;

use serde_json::{json, Value};

use crate::application::error::{AppError, AppResult, ErrorCode};
use crate::application::jobs::JobControl;
use crate::application::ports::{ChatOutcome, ChatRequest, ModelInfo, ModelProvider, ProviderKind};

use super::http::{agent, json_or_err, net_err, read_body, sse_data, status_err, stream_lines};

pub const RESOURCE: &str = "https://api.openai.com/v1";

pub struct ChatGptPlan {
    token: String,
}

impl ChatGptPlan {
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

fn plan_error(code: Option<&str>, status: u16, detail: &str) -> AppError {
    let kind = match code {
        Some("subscription_sharing_usage_limit_exceeded") => "chatgpt_usage_limit",
        Some("subscription_sharing_user_not_eligible") => "chatgpt_not_eligible",
        Some("subscription_sharing_usage_unavailable") | Some("subscription_sharing_user_unavailable") => "chatgpt_unavailable",
        Some("subscription_sharing_invalid_user") => "chatgpt_signed_out",
        _ if status == 401 => "chatgpt_signed_out",
        _ if status == 429 => "chatgpt_usage_limit",
        _ => "chatgpt_error",
    };
    AppError::new(ErrorCode::Unsupported, format!("{kind}: {detail}")).retryable()
}

impl ModelProvider for ChatGptPlan {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Chatgpt
    }

    fn list_models(&self) -> AppResult<Vec<ModelInfo>> {
        let a = agent(Duration::from_secs(10), Duration::from_secs(30));
        let resp = a
            .get(format!("{RESOURCE}/models"))
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(|e| net_err("chatgpt models", e))?;
        let v = json_or_err("chatgpt models", resp)?;
        let list = v.get("data").or_else(|| v.get("models")).and_then(Value::as_array).cloned().unwrap_or_default();
        Ok(list
            .iter()
            .filter(|m| m.get("visibility").and_then(Value::as_str).unwrap_or("list") == "list")
            .filter_map(|m| {
                let id = m.get("slug").or_else(|| m.get("id")).and_then(Value::as_str)?.to_string();
                Some(ModelInfo {
                    label: m.get("display_name").or_else(|| m.get("name")).and_then(Value::as_str).unwrap_or(&id).to_string(),
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
        Err(AppError::new(ErrorCode::Unsupported, "a ChatGPT plan does not provide embeddings"))
    }

    fn chat(&self, model: &str, req: &ChatRequest, control: &JobControl, on_delta: &mut dyn FnMut(&str)) -> AppResult<ChatOutcome> {
        let a = agent(Duration::from_secs(10), Duration::from_secs(300));
        let body = json!({
            "model": model,
            "instructions": req.system,
            "input": [{ "role": "user", "content": [{ "type": "input_text", "text": req.user }] }],
            "store": false,
            "stream": true
        });
        let mut resp = a
            .post(format!("{RESOURCE}/responses"))
            .header("Authorization", &format!("Bearer {}", self.token))
            .send_json(body)
            .map_err(|e| net_err("chatgpt responses", e))?;
        let status = resp.status().as_u16();
        if status >= 300 {
            let b = read_body(&mut resp).unwrap_or_default();
            let code = serde_json::from_str::<Value>(&b).ok().and_then(|v| v.pointer("/error/code").and_then(Value::as_str).map(str::to_string));
            if code.is_some() || status == 401 || status == 429 {
                return Err(plan_error(code.as_deref(), status, &status_err("chatgpt", status, &b).detail));
            }
            return Err(status_err("chatgpt responses", status, &b));
        }
        let mut out = ChatOutcome::default();
        let cancelled = stream_lines(resp, control, |line| {
            let Some(data) = sse_data(line) else { return Ok(true) };
            let v: Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => return Ok(true),
            };
            match v.get("type").and_then(Value::as_str) {
                Some("response.output_text.delta") => {
                    if let Some(piece) = v.get("delta").and_then(Value::as_str) {
                        out.text.push_str(piece);
                        on_delta(piece);
                    }
                }
                Some("response.completed") => {
                    out.output_tokens = v.pointer("/response/usage/output_tokens").and_then(Value::as_u64);
                    return Ok(false);
                }
                Some("response.failed") | Some("error") => {
                    let code = v.pointer("/response/error/code").or_else(|| v.pointer("/error/code")).or_else(|| v.get("code")).and_then(Value::as_str);
                    let msg = v.pointer("/response/error/message").or_else(|| v.get("message")).and_then(Value::as_str).unwrap_or("");
                    return Err(plan_error(code, 0, msg));
                }
                _ => {}
            }
            Ok(true)
        })?;
        out.cancelled = cancelled;
        Ok(out)
    }
}
