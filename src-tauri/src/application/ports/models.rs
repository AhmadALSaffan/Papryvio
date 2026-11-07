//! Model provider ports. Generation and embedding are separate capabilities; a provider
//! may offer either or both. Implementations: Ollama (local), OpenAI-compatible APIs,
//! Anthropic, Sign in with ChatGPT, GitHub Copilot.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::application::error::AppResult;
use crate::application::jobs::JobControl;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ProviderKind {
    Ollama,
    OpenaiCompatible,
    Anthropic,
    Chatgpt,
    GithubCopilot,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::OpenaiCompatible => "openai_compatible",
            Self::Anthropic => "anthropic",
            Self::Chatgpt => "chatgpt",
            Self::GithubCopilot => "github_copilot",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "ollama" => Self::Ollama,
            "openai_compatible" => Self::OpenaiCompatible,
            "anthropic" => Self::Anthropic,
            "chatgpt" => Self::Chatgpt,
            "github_copilot" => Self::GithubCopilot,
            _ => return None,
        })
    }
    /// Local providers keep paper text on this computer.
    pub fn is_local(self) -> bool {
        self == Self::Ollama
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub digest: Option<String>,
    #[ts(type = "number | null")]
    pub size_bytes: Option<u64>,
    pub can_generate: bool,
    pub can_embed: bool,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    /// Application instructions (kept apart from the untrusted paper text).
    pub system: String,
    /// The user turn: question plus the evidence bundle.
    pub user: String,
    /// Ask for a JSON object when the provider supports it.
    pub json: bool,
    pub max_output_tokens: u32,
}

#[derive(Debug, Clone, Default)]
pub struct ChatOutcome {
    pub text: String,
    pub cancelled: bool,
    pub load_ms: Option<u64>,
    pub output_tokens: Option<u64>,
}

pub trait ModelProvider: Send + Sync {
    fn kind(&self) -> ProviderKind;
    fn list_models(&self) -> AppResult<Vec<ModelInfo>>;
    fn supports_embeddings(&self) -> bool;
    /// One vector per input, in order. Must fail rather than silently truncate.
    fn embed(&self, model: &str, inputs: &[String]) -> AppResult<Vec<Vec<f32>>>;
    /// Streams the reply; `on_delta` receives text pieces. Stops when `control` is cancelled.
    fn chat(&self, model: &str, req: &ChatRequest, control: &JobControl, on_delta: &mut dyn FnMut(&str)) -> AppResult<ChatOutcome>;
}
