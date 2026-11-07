use std::sync::Arc;

use tauri::State;

use crate::application::error::AppError;
use crate::application::views::{BootstrapView, OnboardingView, SettingsPatch, SettingsView, StorageView};
use crate::application::{onboarding, settings};
use crate::desktop::error::IpcResult;
use crate::desktop::DesktopState;
use crate::domain::onboarding::{StepAction, StepKey};
use crate::infrastructure::sqlite::SCHEMA_VERSION;

use super::blocking;

fn storage(s: &DesktopState) -> StorageView {
    StorageView {
        data_dir: s.svc.files.data_dir().display().to_string(),
        library_bytes: s.svc.files.library_bytes(),
        database_bytes: s.svc.files.database_bytes(),
    }
}

/// Everything the UI needs on start, read from the database (the source of truth).
#[tauri::command]
pub async fn get_bootstrap_state(state: State<'_, Arc<DesktopState>>, ui_version: String) -> IpcResult<BootstrapView> {
    blocking(state, move |s| {
        let app_version = env!("CARGO_PKG_VERSION").to_string();
        if ui_version != app_version {
            tracing::warn!(ui_version, app_version, "UI and core versions differ");
        }
        Ok(BootstrapView {
            app_version,
            schema_version: SCHEMA_VERSION,
            pdf_engine_available: s.svc.pdf.is_available(),
            settings: settings::get_settings(&s.svc)?,
            onboarding: onboarding::get_progress(&s.svc)?,
            collections: s.svc.collections.list()?,
            active_jobs: s.svc.jobs.list_active()?.iter().map(|j| j.view()).collect(),
            storage: storage(s),
        })
    })
    .await
}

#[tauri::command]
pub async fn get_settings(state: State<'_, Arc<DesktopState>>) -> IpcResult<SettingsView> {
    blocking(state, |s| settings::get_settings(&s.svc)).await
}

#[tauri::command]
pub async fn update_settings(state: State<'_, Arc<DesktopState>>, patch: SettingsPatch) -> IpcResult<SettingsView> {
    blocking(state, move |s| settings::update_settings(&s.svc, &patch)).await
}

#[tauri::command]
pub async fn get_storage_info(state: State<'_, Arc<DesktopState>>) -> IpcResult<StorageView> {
    blocking(state, |s| Ok(storage(s))).await
}

#[tauri::command]
pub async fn get_onboarding(state: State<'_, Arc<DesktopState>>) -> IpcResult<OnboardingView> {
    blocking(state, |s| onboarding::get_progress(&s.svc)).await
}

#[tauri::command]
pub async fn update_onboarding(
    state: State<'_, Arc<DesktopState>>,
    flow_version: i64,
    step_key: String,
    action: StepAction,
    related_entity_id: Option<String>,
) -> IpcResult<OnboardingView> {
    blocking(state, move |s| {
        if flow_version != crate::domain::onboarding::FLOW_VERSION {
            return Err(AppError::conflict("onboarding flow version changed"));
        }
        let step = StepKey::parse(&step_key)?;
        onboarding::update_step(&s.svc, step, action, related_entity_id.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn set_onboarding_dismissed(state: State<'_, Arc<DesktopState>>, dismissed: bool) -> IpcResult<OnboardingView> {
    blocking(state, move |s| onboarding::set_dismissed(&s.svc, dismissed)).await
}

#[tauri::command]
pub async fn restart_onboarding(state: State<'_, Arc<DesktopState>>) -> IpcResult<OnboardingView> {
    blocking(state, |s| onboarding::restart(&s.svc)).await
}

/// Local diagnostics (no document content): versions and capabilities.
#[tauri::command]
pub async fn get_diagnostics(state: State<'_, Arc<DesktopState>>) -> IpcResult<String> {
    blocking(state, |s| {
        let fts5 = s.db.fts5_available()?;
        let sqlite = s.db.sqlite_version()?;
        Ok(format!(
            "Papryvio {}\nschema v{}\nSQLite {} (FTS5: {})\nPDF engine: {}\nOS: {} {}\n",
            env!("CARGO_PKG_VERSION"),
            SCHEMA_VERSION,
            sqlite,
            if fts5 { "yes" } else { "no" },
            if s.svc.pdf.is_available() { "available" } else { "missing" },
            std::env::consts::OS,
            std::env::consts::ARCH,
        ))
    })
    .await
}
