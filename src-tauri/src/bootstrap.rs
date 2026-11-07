//! Composition root: wires infrastructure adapters into the application services.
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use tauri::{AppHandle, Manager};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::application::ingestion::recovery;
use crate::application::jobs::JobScheduler;
use crate::application::Services;
use crate::desktop::events::TauriEventSink;
use crate::desktop::{DesktopState, ImportTokens};
use crate::application::generation::GenerationRunner;
use crate::infrastructure::auth::{AccountAuth, SignIn};
use crate::infrastructure::filesystem::{AppPaths, LocalFileStore};
use crate::infrastructure::models::{DefaultProviderFactory, OsSecretStore};
use crate::infrastructure::pdf::WorkerPdfExtractor;
use crate::infrastructure::sqlite::{Db, SqliteStore};
use crate::infrastructure::SystemClock;
use crate::PDF_WORKER_FLAG;

/// Papryvio's GitHub OAuth App (device flow, no secret). Override at build time with
/// PAPRYVIO_GITHUB_CLIENT_ID to use another registered app.
pub const GITHUB_OAUTH_CLIENT_ID: &str = match option_env!("PAPRYVIO_GITHUB_CLIENT_ID") {
    Some(id) => id,
    None => "Ov23liveqPeexDt9CuqY",
};

static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

fn init_logging(dir: &std::path::Path) {
    let appender = tracing_appender::rolling::daily(dir, "papryvio.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let _ = LOG_GUARD.set(guard);
    let filter = EnvFilter::try_from_env("PAPRYVIO_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let file_layer = fmt::layer().with_writer(writer).with_ansi(false);
    let registry = tracing_subscriber::registry().with(filter).with(file_layer);
    if cfg!(debug_assertions) {
        let _ = registry.with(fmt::layer()).try_init();
    } else {
        let _ = registry.try_init();
    }
}

/// Locates pdfium.dll: bundled resources first, then the source tree during development.
fn pdfium_path(app: &AppHandle) -> PathBuf {
    let name = pdfium_render::prelude::Pdfium::pdfium_platform_library_name();
    let mut candidates = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        candidates.push(res.join("pdfium").join(&name));
    }
    if cfg!(debug_assertions) {
        candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join("pdfium").join(&name));
    }
    candidates.iter().find(|p| p.is_file()).cloned().unwrap_or_else(|| candidates[0].clone())
}

pub fn build(app: &AppHandle) -> Result<Arc<DesktopState>, Box<dyn std::error::Error>> {
    let root = app.path().app_data_dir()?;
    let paths = AppPaths::create(root)?;
    init_logging(&paths.logs);
    tracing::info!(version = env!("CARGO_PKG_VERSION"), data_dir = %paths.root.display(), "starting");

    let db = Db::open(&paths.database).map_err(|e| e.to_string())?;
    let store = Arc::new(SqliteStore::new(Arc::clone(&db)));
    let data_dir = paths.root.clone();
    let files = Arc::new(LocalFileStore::new(paths)?);
    let secrets: Arc<dyn crate::application::ports::SecretStore> = Arc::new(OsSecretStore::new("com.papryvio.desktop"));
    let auth = Arc::new(AccountAuth::new(data_dir.clone(), GITHUB_OAUTH_CLIENT_ID.to_string()));
    let factory = Arc::new(DefaultProviderFactory::new(Arc::clone(&secrets), Arc::clone(&auth), data_dir.join("copilot")));
    let pdfium = pdfium_path(app);
    tracing::info!(available = pdfium.is_file(), "pdf engine");
    let pdf = Arc::new(WorkerPdfExtractor::new(std::env::current_exe()?, pdfium, PDF_WORKER_FLAG));
    let events = Arc::new(TauriEventSink::new(app.clone()));

    let svc = Arc::new(Services {
        clock: Arc::new(SystemClock),
        collections: store.clone(),
        documents: store.clone(),
        pages: store.clone(),
        jobs: store.clone(),
        settings: store.clone(),
        onboarding: store.clone(),
        idempotency: store.clone(),
        files,
        pdf,
        events: events.clone(),
        index: store.clone(),
        answers: store.clone(),
        providers: store.clone(),
        secrets,
        provider_factory: factory,
        sign_in: Arc::new(SignIn(auth)),
    });
    let scheduler = JobScheduler::start(Arc::clone(&svc));
    let generation = GenerationRunner::start(Arc::clone(&svc));
    // Answers interrupted by a previous exit are closed honestly; the question stays for retry.
    for attempt in svc.answers.attempts_running().unwrap_or_default() {
        let _ = svc.answers.finish_attempt(&attempt, "failed", None, Some("interrupted"), "{}", svc.now());
    }
    if let Err(e) = recovery::recover_on_startup(&svc, &scheduler) {
        tracing::error!("startup recovery failed: {e}");
    }
    if let Err(e) = recovery::upgrade_text_pipeline(&svc, &scheduler) {
        tracing::error!("text pipeline upgrade failed: {e}");
    }
    Ok(Arc::new(DesktopState { svc, scheduler, generation, events, import_tokens: ImportTokens::default(), db, sign_in_kinds: Default::default() }))
}
