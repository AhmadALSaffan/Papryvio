use std::sync::Arc;

use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::application::error::AppError;
use crate::application::views::{
    AskStarted, ComparisonRow, ComparisonView, ConversationSummary, ConversationView, EvidenceLocation, EvidenceTarget, RetrievalMode,
    ScopeInput, SearchResults,
};
use crate::application::{answering, comparison, retrieval};
use crate::desktop::error::{IpcError, IpcResult};
use crate::desktop::DesktopState;
use crate::domain::ids::JobId;

use super::{blocking, request_id};

fn uuid_like(raw: &str) -> Result<String, AppError> {
    uuid::Uuid::parse_str(raw).map(|u| u.hyphenated().to_string()).map_err(|_| AppError::validation("invalid id"))
}

#[tauri::command]
pub async fn search_documents(
    state: State<'_, Arc<DesktopState>>,
    query: String,
    scope: ScopeInput,
    mode: RetrievalMode,
    limit: Option<u32>,
) -> IpcResult<SearchResults> {
    blocking(state, move |s| retrieval::search(&s.svc, &query, &scope, mode, limit.unwrap_or(20))).await
}

#[tauri::command]
pub async fn ask_question(
    state: State<'_, Arc<DesktopState>>,
    request_id: String,
    conversation_id: Option<String>,
    question: String,
    scope: ScopeInput,
) -> IpcResult<AskStarted> {
    blocking(state, move |s| {
        let rid = request_id_ok(&request_id)?;
        let conv = conversation_id.as_deref().map(uuid_like).transpose()?;
        answering::ask(&s.svc, &s.generation, &rid, conv.as_deref(), &question, &scope)
    })
    .await
}

fn request_id_ok(raw: &str) -> Result<String, AppError> {
    request_id(raw)
}

#[tauri::command]
pub async fn cancel_generation(state: State<'_, Arc<DesktopState>>, attempt_id: String) -> IpcResult<bool> {
    blocking(state, move |s| Ok(answering::cancel(&s.generation, &uuid_like(&attempt_id)?))).await
}

#[tauri::command]
pub async fn retry_answer(state: State<'_, Arc<DesktopState>>, request_id: String, message_id: String, scope: Option<ScopeInput>) -> IpcResult<String> {
    blocking(state, move |s| {
        let rid = request_id_ok(&request_id)?;
        answering::retry(&s.svc, &s.generation, &rid, &uuid_like(&message_id)?, scope.as_ref())
    })
    .await
}

#[tauri::command]
pub async fn get_conversation(state: State<'_, Arc<DesktopState>>, id: String) -> IpcResult<ConversationView> {
    blocking(state, move |s| answering::conversation(&s.svc, &uuid_like(&id)?)).await
}

#[tauri::command]
pub async fn list_conversations(state: State<'_, Arc<DesktopState>>) -> IpcResult<Vec<ConversationSummary>> {
    blocking(state, |s| answering::conversations(&s.svc)).await
}

#[tauri::command]
pub async fn delete_conversation(state: State<'_, Arc<DesktopState>>, id: String) -> IpcResult<()> {
    blocking(state, move |s| s.svc.answers.delete_conversation(&uuid_like(&id)?)).await
}

#[tauri::command]
pub async fn open_evidence(state: State<'_, Arc<DesktopState>>, target: EvidenceTarget) -> IpcResult<EvidenceLocation> {
    blocking(state, move |s| answering::open_evidence(&s.svc, &target)).await
}

#[tauri::command]
pub async fn build_comparison(state: State<'_, Arc<DesktopState>>, request_id: String, document_ids: Vec<String>, rows: Vec<ComparisonRow>) -> IpcResult<String> {
    blocking(state, move |s| comparison::build(&s.svc, &s.generation, &request_id_ok(&request_id)?, &document_ids, &rows)).await
}

#[tauri::command]
pub async fn get_comparison(state: State<'_, Arc<DesktopState>>, id: String) -> IpcResult<ComparisonView> {
    blocking(state, move |s| s.svc.answers.comparison(&uuid_like(&id)?)?.ok_or_else(|| AppError::not_found("comparison"))).await
}

#[tauri::command]
pub async fn list_comparisons(state: State<'_, Arc<DesktopState>>) -> IpcResult<Vec<ComparisonView>> {
    blocking(state, |s| s.svc.answers.latest_comparisons(20)).await
}

#[tauri::command]
pub async fn retry_comparison_cell(state: State<'_, Arc<DesktopState>>, comparison_id: String, cell_id: String) -> IpcResult<()> {
    blocking(state, move |s| comparison::retry_cell(&s.svc, &s.generation, &uuid_like(&comparison_id)?, &uuid_like(&cell_id)?)).await
}

#[tauri::command]
pub async fn cancel_comparison(state: State<'_, Arc<DesktopState>>, id: String) -> IpcResult<bool> {
    blocking(state, move |s| Ok(s.generation.cancel(&uuid_like(&id)?))).await
}

/// Asks where to save (native dialog in Rust) and writes the export. Returns the file name.
#[tauri::command]
pub async fn export_comparison(app: tauri::AppHandle, state: State<'_, Arc<DesktopState>>, id: String, format: String) -> IpcResult<Option<String>> {
    let st = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || -> Result<Option<String>, AppError> {
        let id = uuid_like(&id)?;
        let content = comparison::export(&st.svc, &id, &format)?;
        let (ext, name) = if format == "csv" { ("csv", "comparison.csv") } else { ("md", "comparison.md") };
        let Some(path) = app.dialog().file().set_file_name(name).add_filter(ext.to_uppercase(), &[ext]).blocking_save_file() else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|e| AppError::file_access(e.to_string()))?;
        std::fs::write(&path, content.as_bytes()).map_err(|e| AppError::file_access(e.to_string()))?;
        Ok(path.file_name().map(|n| n.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| IpcError::from(AppError::internal(e.to_string())))?
    .map_err(IpcError::from)
}

/// New attempt for any failed or cancelled background job (indexing, embedding).
#[tauri::command]
pub async fn retry_job(state: State<'_, Arc<DesktopState>>, job_id: String) -> IpcResult<crate::application::views::JobView> {
    blocking(state, move |s| s.scheduler.retry(&JobId::parse(&job_id).ok_or_else(|| AppError::validation("bad job id"))?)).await
}
