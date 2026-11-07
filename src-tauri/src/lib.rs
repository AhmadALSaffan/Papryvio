//! Papryvio core: a local research workspace for PDF papers.
//!
//! Layers (dependencies point inward):
//! `domain` ← `application` (use cases + ports) ← `infrastructure` (adapters)
//! and `desktop` (Tauri IPC). `bootstrap` is the composition root.
pub mod application;
pub mod bootstrap;
pub mod desktop;
pub mod domain;
pub mod infrastructure;

use std::sync::Arc;

use tauri::{DragDropEvent, Manager, RunEvent, WindowEvent};

use desktop::commands::{answers, documents, library, models, system};
use desktop::DesktopState;

/// Command-line flag that turns the executable into the isolated PDF worker.
pub const PDF_WORKER_FLAG: &str = "--pdf-worker";

pub fn run_pdf_worker() -> i32 {
    infrastructure::pdf::worker::run_worker()
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = bootstrap::build(app.handle())?;
            app.manage(state);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::DragDrop(drag) = event {
                let Some(state) = window.try_state::<Arc<DesktopState>>() else { return };
                match drag {
                    DragDropEvent::Enter { .. } => state.events.drag_state(true),
                    DragDropEvent::Leave => state.events.drag_state(false),
                    DragDropEvent::Drop { paths, .. } => {
                        state.events.drag_state(false);
                        let files: Vec<_> = paths.iter().filter(|p| p.is_file()).cloned().collect();
                        if !files.is_empty() {
                            let selection = state.import_tokens.register(files);
                            state.events.files_dropped(selection);
                        }
                    }
                    _ => {}
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            system::get_bootstrap_state,
            system::get_settings,
            system::update_settings,
            system::get_storage_info,
            system::get_onboarding,
            system::update_onboarding,
            system::set_onboarding_dismissed,
            system::restart_onboarding,
            system::get_diagnostics,
            library::list_collections,
            library::create_collection,
            library::rename_collection,
            library::delete_collection,
            documents::choose_import_files,
            documents::import_documents,
            documents::list_documents,
            documents::get_document,
            documents::update_document_metadata,
            documents::add_to_collection,
            documents::remove_from_collection,
            documents::delete_document,
            documents::retry_ingestion,
            documents::get_job_state,
            documents::pause_job,
            documents::resume_job,
            documents::cancel_job,
            documents::read_document_file,
            models::get_model_settings,
            models::add_ollama_provider,
            models::add_api_provider,
            models::start_sign_in,
            models::poll_sign_in,
            models::cancel_sign_in,
            models::remove_provider,
            models::list_provider_models,
            models::test_model,
            models::set_cloud_consent,
            models::save_generation_model,
            models::save_embedding_model,
            models::clear_embedding_model,
            models::open_external,
            answers::search_documents,
            answers::ask_question,
            answers::cancel_generation,
            answers::retry_answer,
            answers::get_conversation,
            answers::list_conversations,
            answers::delete_conversation,
            answers::open_evidence,
            answers::build_comparison,
            answers::get_comparison,
            answers::list_comparisons,
            answers::retry_comparison_cell,
            answers::cancel_comparison,
            answers::export_comparison,
            answers::retry_job,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Papryvio");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            if let Some(state) = handle.try_state::<Arc<DesktopState>>() {
                tracing::info!("shutting down: checkpointing jobs");
                state.generation.shutdown();
                state.scheduler.shutdown();
                infrastructure::models::copilot::shutdown_all();
            }
        }
    });
}
