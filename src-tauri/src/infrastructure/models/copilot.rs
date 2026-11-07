//! GitHub Copilot through the official Rust Copilot SDK (`github-copilot-sdk`), which runs
//! the bundled Copilot CLI and talks JSON-RPC to it. Papryvio only needs plain answers, so
//! every session has no tools (all built-ins excluded, every permission request denied)
//! and its own system prompt. One runtime client per account, isolated `COPILOT_HOME`.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use github_copilot_sdk::handler::DenyAllHandler;
use github_copilot_sdk::types::{MessageOptions, SessionConfig, SystemMessageConfig};
use github_copilot_sdk::{Client, ClientOptions};
use tokio::runtime::Runtime;

use crate::application::error::{AppError, AppResult, ErrorCode};
use crate::application::jobs::{JobControl, Signal};
use crate::application::ports::{ChatOutcome, ChatRequest, ModelInfo, ModelProvider, ProviderKind};

/// Clients older than this are restarted with the same token (the runtime trades it for
/// short-lived Copilot tokens and can lose them after many hours).
const MAX_CLIENT_AGE: Duration = Duration::from_secs(3 * 3600);
const ANSWER_TIMEOUT: Duration = Duration::from_secs(300);

fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("papryvio-copilot").enable_all().build().expect("copilot runtime")
    })
}

struct Cached {
    token_hash: String,
    born: std::time::Instant,
    client: Arc<Client>,
}

fn clients() -> &'static Mutex<HashMap<String, Cached>> {
    static C: OnceLock<Mutex<HashMap<String, Cached>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn sdk_err(context: &str, e: impl std::fmt::Display) -> AppError {
    let msg = e.to_string();
    let kind = if msg.to_lowercase().contains("not authenticated") || msg.contains("401") {
        "copilot_signed_out"
    } else if msg.to_lowercase().contains("not enabled") || msg.contains("403") {
        "copilot_not_enabled"
    } else {
        "copilot_error"
    };
    AppError::new(ErrorCode::Unsupported, format!("{kind}: {context}: {msg}")).retryable()
}

pub struct Copilot {
    account_id: String,
    token: String,
    home: PathBuf,
}

impl Copilot {
    pub fn new(account_id: &str, token: String, home: PathBuf) -> Self {
        Self { account_id: account_id.into(), token, home }
    }

    async fn client(&self) -> AppResult<Arc<Client>> {
        use sha2::{Digest, Sha256};
        let hash = hex::encode(Sha256::digest(self.token.as_bytes()));
        {
            let map = clients().lock().unwrap();
            if let Some(c) = map.get(&self.account_id) {
                if c.token_hash == hash && c.born.elapsed() < MAX_CLIENT_AGE {
                    return Ok(Arc::clone(&c.client));
                }
            }
        }
        let old = clients().lock().unwrap().remove(&self.account_id);
        if let Some(old) = old {
            let _ = old.client.stop().await;
        }
        std::fs::create_dir_all(&self.home).map_err(|e| AppError::file_access(e.to_string()))?;
        let mut opts = ClientOptions::default();
        opts.github_token = Some(self.token.clone());
        opts.use_logged_in_user = Some(false);
        opts.working_directory = self.home.clone();
        opts.env = vec![("COPILOT_HOME".into(), self.home.clone().into_os_string())];
        opts.session_idle_timeout_seconds = Some(900);
        let client = Arc::new(Client::start(opts).await.map_err(|e| sdk_err("start", e))?);
        clients().lock().unwrap().insert(self.account_id.clone(), Cached { token_hash: hash, born: std::time::Instant::now(), client: Arc::clone(&client) });
        Ok(client)
    }
}

/// Stops every running Copilot runtime (app exit).
pub fn shutdown_all() {
    let all: Vec<Arc<Client>> = clients().lock().unwrap().drain().map(|(_, c)| c.client).collect();
    if all.is_empty() {
        return;
    }
    runtime().block_on(async {
        for c in all {
            let _ = tokio::time::timeout(Duration::from_secs(3), c.stop()).await;
        }
    });
}

impl ModelProvider for Copilot {
    fn kind(&self) -> ProviderKind {
        ProviderKind::GithubCopilot
    }

    fn list_models(&self) -> AppResult<Vec<ModelInfo>> {
        runtime().block_on(async {
            let client = self.client().await?;
            let models = client.list_models().await.map_err(|e| sdk_err("models", e))?;
            Ok(models
                .into_iter()
                .map(|m| ModelInfo { label: m.name.clone(), id: m.id, digest: None, size_bytes: None, can_generate: true, can_embed: false })
                .collect())
        })
    }

    fn supports_embeddings(&self) -> bool {
        false
    }

    fn embed(&self, _model: &str, _inputs: &[String]) -> AppResult<Vec<Vec<f32>>> {
        Err(AppError::new(ErrorCode::Unsupported, "GitHub Copilot does not provide embeddings"))
    }

    fn chat(&self, model: &str, req: &ChatRequest, control: &JobControl, on_delta: &mut dyn FnMut(&str)) -> AppResult<ChatOutcome> {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let model = model.to_string();
        let system = req.system.clone();
        let user = req.user.clone();
        let handle = runtime().spawn({
            let this = Copilot { account_id: self.account_id.clone(), token: self.token.clone(), home: self.home.clone() };
            async move {
                let client = this.client().await?;
                let mut config = SessionConfig::default().with_permission_handler(Arc::new(DenyAllHandler));
                config.model = Some(model);
                config.streaming = Some(true);
                config.available_tools = Some(Vec::new());
                config.system_message = Some(SystemMessageConfig::new().with_mode("replace").with_content(system));
                let session = client.create_session(config).await.map_err(|e| sdk_err("session", e))?;
                let mut events = session.subscribe();
                let forward = tokio::spawn(async move {
                    while let Ok(event) = events.recv().await {
                        if event.event_type == "assistant.message_delta" {
                            if let Some(d) = event.data.get("deltaContent").and_then(|v| v.as_str()) {
                                let _ = tx.send(d.to_string());
                            }
                        }
                    }
                });
                let result = session.send_and_wait(MessageOptions::new(user).with_wait_timeout(ANSWER_TIMEOUT)).await;
                forward.abort();
                let final_text = match &result {
                    Ok(Some(ev)) => ev.data.get("content").and_then(|v| v.as_str()).map(str::to_string),
                    _ => None,
                };
                let _ = session.disconnect().await;
                result.map_err(|e| sdk_err("answer", e))?;
                Ok::<Option<String>, AppError>(final_text)
            }
        });
        let mut out = ChatOutcome::default();
        loop {
            if control.signal() == Signal::Cancel {
                handle.abort();
                out.cancelled = true;
                return Ok(out);
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(piece) => {
                    out.text.push_str(&piece);
                    on_delta(&piece);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if handle.is_finished() {
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    if handle.is_finished() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
        while let Ok(piece) = rx.try_recv() {
            out.text.push_str(&piece);
            on_delta(&piece);
        }
        let final_text = runtime().block_on(handle).map_err(|e| AppError::internal(format!("copilot task: {e}")))??;
        // When deltas were not streamed, the final message carries the whole reply.
        if out.text.trim().is_empty() {
            if let Some(t) = final_text {
                on_delta(&t);
                out.text = t;
            }
        }
        Ok(out)
    }
}
