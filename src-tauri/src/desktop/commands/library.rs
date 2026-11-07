use std::sync::Arc;

use tauri::State;

use crate::application::error::AppError;
use crate::application::library;
use crate::application::views::CollectionView;
use crate::desktop::error::IpcResult;
use crate::desktop::DesktopState;
use crate::domain::ids::CollectionId;

use super::blocking;

fn collection_id(raw: &str) -> Result<CollectionId, AppError> {
    CollectionId::parse(raw).ok_or_else(|| AppError::validation("invalid collection id"))
}

#[tauri::command]
pub async fn list_collections(state: State<'_, Arc<DesktopState>>) -> IpcResult<Vec<CollectionView>> {
    blocking(state, |s| library::list_collections(&s.svc)).await
}

#[tauri::command]
pub async fn create_collection(
    state: State<'_, Arc<DesktopState>>,
    request_id: String,
    name: String,
) -> IpcResult<CollectionView> {
    blocking(state, move |s| {
        let rid = super::request_id(&request_id)?;
        library::create_collection(&s.svc, &rid, &name)
    })
    .await
}

#[tauri::command]
pub async fn rename_collection(state: State<'_, Arc<DesktopState>>, id: String, name: String) -> IpcResult<CollectionView> {
    blocking(state, move |s| library::rename_collection(&s.svc, &collection_id(&id)?, &name)).await
}

#[tauri::command]
pub async fn delete_collection(state: State<'_, Arc<DesktopState>>, id: String) -> IpcResult<()> {
    blocking(state, move |s| library::delete_collection(&s.svc, &collection_id(&id)?)).await
}
