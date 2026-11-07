use std::sync::Arc;

use tauri::ipc::Response;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::application::error::{AppError, AppResult};
use crate::application::ingestion::{delete, import, retry};
use crate::application::library;
use crate::application::views::{DocumentListPage, DocumentQuery, DocumentView, ImportReport, ImportSelection, JobView, MetadataUpdate};
use crate::desktop::error::{IpcError, IpcResult};
use crate::desktop::DesktopState;
use crate::domain::ids::{CollectionId, DocumentId, JobId};

use super::blocking;

fn doc_id(raw: &str) -> AppResult<DocumentId> {
    DocumentId::parse(raw).ok_or_else(|| AppError::validation("invalid document id"))
}
fn collection_id(raw: &str) -> AppResult<CollectionId> {
    CollectionId::parse(raw).ok_or_else(|| AppError::validation("invalid collection id"))
}
fn job_id(raw: &str) -> AppResult<JobId> {
    JobId::parse(raw).ok_or_else(|| AppError::validation("invalid job id"))
}

/// Opens the native file dialog in Rust. Returns `None` when the user cancels.
#[tauri::command]
pub async fn choose_import_files(app: tauri::AppHandle, state: State<'_, Arc<DesktopState>>) -> IpcResult<Option<ImportSelection>> {
    let st = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let picked = app.dialog().file().add_filter("PDF", &["pdf", "PDF"]).blocking_pick_files();
        let paths: Vec<_> = picked.unwrap_or_default().into_iter().filter_map(|f| f.into_path().ok()).collect();
        if paths.is_empty() {
            None
        } else {
            Some(st.import_tokens.register(paths))
        }
    })
    .await
    .map_err(|e| IpcError::from(AppError::internal(e.to_string())))
}

#[tauri::command]
pub async fn import_documents(
    state: State<'_, Arc<DesktopState>>,
    request_id: String,
    token: String,
    collection_id: String,
) -> IpcResult<ImportReport> {
    blocking(state, move |s| {
        let rid = super::request_id(&request_id)?;
        let cid = self::collection_id(&collection_id)?;
        if token.len() > 64 {
            return Err(AppError::validation("invalid token"));
        }
        let paths = s.import_tokens.take(&token);
        import::import_files(&s.svc, &s.scheduler, &rid, &token, paths.as_deref(), &cid)
    })
    .await
}

#[tauri::command]
pub async fn list_documents(state: State<'_, Arc<DesktopState>>, query: DocumentQuery) -> IpcResult<DocumentListPage> {
    blocking(state, move |s| library::list_documents(&s.svc, &query)).await
}

#[tauri::command]
pub async fn get_document(state: State<'_, Arc<DesktopState>>, id: String) -> IpcResult<DocumentView> {
    blocking(state, move |s| library::get_document(&s.svc, &doc_id(&id)?)).await
}

#[tauri::command]
pub async fn update_document_metadata(
    state: State<'_, Arc<DesktopState>>,
    id: String,
    update: MetadataUpdate,
) -> IpcResult<DocumentView> {
    blocking(state, move |s| library::update_metadata(&s.svc, &doc_id(&id)?, &update)).await
}

#[tauri::command]
pub async fn add_to_collection(state: State<'_, Arc<DesktopState>>, collection_id: String, document_id: String) -> IpcResult<bool> {
    blocking(state, move |s| library::add_to_collection(&s.svc, &self::collection_id(&collection_id)?, &doc_id(&document_id)?)).await
}

#[tauri::command]
pub async fn remove_from_collection(
    state: State<'_, Arc<DesktopState>>,
    collection_id: String,
    document_id: String,
) -> IpcResult<bool> {
    blocking(state, move |s| {
        library::remove_from_collection(&s.svc, &self::collection_id(&collection_id)?, &doc_id(&document_id)?)
    })
    .await
}

#[tauri::command]
pub async fn delete_document(state: State<'_, Arc<DesktopState>>, id: String) -> IpcResult<JobId> {
    blocking(state, move |s| delete::delete_document(&s.svc, &s.scheduler, &doc_id(&id)?)).await
}

#[tauri::command]
pub async fn retry_ingestion(state: State<'_, Arc<DesktopState>>, document_id: String) -> IpcResult<JobView> {
    blocking(state, move |s| retry::retry_extraction(&s.svc, &s.scheduler, &doc_id(&document_id)?)).await
}

#[tauri::command]
pub async fn get_job_state(state: State<'_, Arc<DesktopState>>, job_id: String) -> IpcResult<JobView> {
    blocking(state, move |s| {
        s.svc.jobs.get(&self::job_id(&job_id)?)?.map(|j| j.view()).ok_or_else(|| AppError::not_found("job"))
    })
    .await
}

#[tauri::command]
pub async fn pause_job(state: State<'_, Arc<DesktopState>>, job_id: String) -> IpcResult<JobView> {
    blocking(state, move |s| s.scheduler.pause(&self::job_id(&job_id)?)).await
}

#[tauri::command]
pub async fn resume_job(state: State<'_, Arc<DesktopState>>, job_id: String) -> IpcResult<JobView> {
    blocking(state, move |s| s.scheduler.resume(&self::job_id(&job_id)?)).await
}

#[tauri::command]
pub async fn cancel_job(state: State<'_, Arc<DesktopState>>, job_id: String) -> IpcResult<JobView> {
    blocking(state, move |s| s.scheduler.cancel(&self::job_id(&job_id)?)).await
}

/// Streams the managed copy of a document to the PDF viewer as raw bytes.
/// Only documents known to the library can be read; no path crosses the IPC boundary.
#[tauri::command]
pub async fn read_document_file(state: State<'_, Arc<DesktopState>>, id: String) -> IpcResult<Response> {
    let bytes = blocking(state, move |s| {
        let id = doc_id(&id)?;
        let doc = s.svc.documents.get(&id)?.ok_or_else(|| AppError::not_found("document"))?;
        if doc.deleted_at.is_some() || !doc.state.is_readable() {
            return Err(AppError::not_found("document"));
        }
        let rel = doc.managed_file.ok_or_else(|| AppError::not_found("file"))?;
        let path = s.svc.files.resolve_managed(&rel)?;
        std::fs::read(&path).map_err(|e| AppError::file_access(format!("read managed file: {e}")))
    })
    .await?;
    Ok(Response::new(bytes))
}
