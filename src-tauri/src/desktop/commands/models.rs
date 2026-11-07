use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use tauri_plugin_opener::OpenerExt;
use ts_rs::TS;

use crate::application::error::AppError;
use crate::application::models;
use crate::application::ports::{ModelInfo, ProviderKind, SignInStart};
use crate::application::views::{ModelSettingsView, ModelTestResult, ProviderView};
use crate::desktop::error::{IpcError, IpcResult};
use crate::desktop::DesktopState;

use super::blocking;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SignInStatus {
    /// `pending`, `complete`, `denied`, `expired`, `error`
    pub status: String,
    pub message: Option<String>,
    pub provider: Option<ProviderView>,
}

#[tauri::command]
pub async fn get_model_settings(state: State<'_, Arc<DesktopState>>) -> IpcResult<ModelSettingsView> {
    blocking(state, |s| models::model_settings(&s.svc)).await
}

#[tauri::command]
pub async fn add_ollama_provider(state: State<'_, Arc<DesktopState>>, base_url: String) -> IpcResult<ProviderView> {
    blocking(state, move |s| models::add_ollama(&s.svc, &base_url)).await
}

#[tauri::command]
pub async fn add_api_provider(
    state: State<'_, Arc<DesktopState>>,
    kind: ProviderKind,
    label: String,
    base_url: String,
    api_key: String,
) -> IpcResult<ProviderView> {
    blocking(state, move |s| models::add_api_provider(&s.svc, kind, &label, &base_url, &api_key)).await
}

#[tauri::command]
pub async fn start_sign_in(state: State<'_, Arc<DesktopState>>, kind: ProviderKind) -> IpcResult<SignInStart> {
    blocking(state, move |s| {
        let start = s.svc.sign_in.start(kind)?;
        s.sign_in_kinds.lock().unwrap().insert(start.flow_id.clone(), kind);
        Ok(start)
    })
    .await
}

#[tauri::command]
pub async fn poll_sign_in(state: State<'_, Arc<DesktopState>>, flow_id: String) -> IpcResult<SignInStatus> {
    blocking(state, move |s| {
        let kind = *s.sign_in_kinds.lock().unwrap().get(&flow_id).ok_or_else(|| AppError::not_found("sign-in"))?;
        let (status, provider) = models::poll_sign_in(&s.svc, kind, &flow_id)?;
        if status != "pending" {
            s.sign_in_kinds.lock().unwrap().remove(&flow_id);
        }
        let (head, message) = match status.split_once(':') {
            Some((h, m)) => (h.to_string(), Some(m.to_string())),
            None => (status, None),
        };
        Ok(SignInStatus { status: head, message, provider })
    })
    .await
}

#[tauri::command]
pub async fn cancel_sign_in(state: State<'_, Arc<DesktopState>>, flow_id: String) -> IpcResult<()> {
    blocking(state, move |s| {
        s.svc.sign_in.cancel(&flow_id);
        s.sign_in_kinds.lock().unwrap().remove(&flow_id);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn remove_provider(state: State<'_, Arc<DesktopState>>, id: String) -> IpcResult<()> {
    blocking(state, move |s| models::remove_provider(&s.svc, &id)).await
}

#[tauri::command]
pub async fn list_provider_models(state: State<'_, Arc<DesktopState>>, provider_id: String) -> IpcResult<Vec<ModelInfo>> {
    blocking(state, move |s| models::list_models(&s.svc, &provider_id)).await
}

#[tauri::command]
pub async fn test_model(state: State<'_, Arc<DesktopState>>, provider_id: String, model: String, purpose: String) -> IpcResult<ModelTestResult> {
    blocking(state, move |s| models::test_model(&s.svc, &provider_id, &model, &purpose)).await
}

#[tauri::command]
pub async fn set_cloud_consent(state: State<'_, Arc<DesktopState>>, provider_id: String, allowed: bool) -> IpcResult<ModelSettingsView> {
    blocking(state, move |s| models::set_consent(&s.svc, &provider_id, allowed)).await
}

#[tauri::command]
pub async fn save_generation_model(state: State<'_, Arc<DesktopState>>, provider_id: String, model: String) -> IpcResult<ModelSettingsView> {
    blocking(state, move |s| models::save_generation(&s.svc, &provider_id, &model)).await
}

#[tauri::command]
pub async fn save_embedding_model(state: State<'_, Arc<DesktopState>>, provider_id: String, model: String) -> IpcResult<ModelSettingsView> {
    blocking(state, move |s| models::save_embedding(&s.svc, &s.scheduler, &provider_id, &model)).await
}

#[tauri::command]
pub async fn clear_embedding_model(state: State<'_, Arc<DesktopState>>) -> IpcResult<ModelSettingsView> {
    blocking(state, |s| models::clear_embedding(&s.svc)).await
}

/// Opens an allow-listed help or sign-in page in the system browser (user-initiated only).
#[tauri::command]
pub async fn open_external(app: tauri::AppHandle, url: String) -> IpcResult<()> {
    const ALLOWED: &[&str] = &[
        "https://github.com/login/device",
        "https://auth.openai.com/",
        "https://ollama.com/",
        "https://docs.ollama.com/",
        "https://chatgpt.com/",
        "https://platform.openai.com/",
        "https://console.anthropic.com/",
        "https://openrouter.ai/",
        "https://github.com/settings/",
        "https://github.com/features/copilot",
        "https://aistudio.google.com/",
        "https://console.x.ai/",
        "https://console.mistral.ai/",
        "https://platform.deepseek.com/",
        "https://console.groq.com/",
        "https://api.together.ai/",
        "https://fireworks.ai/",
        "https://cloud.cerebras.ai/",
        "https://huggingface.co/settings/",
        "https://build.nvidia.com/",
        "https://platform.moonshot.ai/",
        "https://modelstudio.console.alibabacloud.com/",
        "https://z.ai/",
        "https://dashboard.cohere.com/",
        "https://docs.litellm.ai/",
    ];
    if url.len() > 4096 || !ALLOWED.iter().any(|p| url.starts_with(p)) {
        return Err(IpcError::from(AppError::not_allowed("link not allowed")));
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| IpcError::from(AppError::internal(e.to_string())))
}
