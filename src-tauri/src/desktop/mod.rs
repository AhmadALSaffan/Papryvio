//! Desktop adapter: Tauri commands, events, IPC validation and error mapping.
pub mod commands;
pub mod error;
pub mod events;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::application::jobs::JobScheduler;
use crate::application::views::{ImportSelection, SelectedFile};
use crate::application::Services;
use crate::infrastructure::sqlite::Db;

use events::TauriEventSink;

const IMPORT_TOKEN_TTL: Duration = Duration::from_secs(15 * 60);

/// Files the user picked in a native dialog or dropped on the window, keyed by an opaque token.
/// The UI never sends file system paths; it only refers to these tokens.
#[derive(Default)]
pub struct ImportTokens {
    inner: Mutex<HashMap<String, (Vec<PathBuf>, Instant)>>,
}

impl ImportTokens {
    pub fn register(&self, paths: Vec<PathBuf>) -> ImportSelection {
        let token = uuid::Uuid::new_v4().to_string();
        let files = paths
            .iter()
            .map(|p| SelectedFile {
                name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                size_bytes: std::fs::metadata(p).map(|m| m.len()).unwrap_or(0),
            })
            .collect();
        let mut map = self.inner.lock().unwrap();
        map.retain(|_, (_, at)| at.elapsed() < IMPORT_TOKEN_TTL);
        map.insert(token.clone(), (paths, Instant::now()));
        ImportSelection { token, files }
    }

    /// Single use: the token is consumed.
    pub fn take(&self, token: &str) -> Option<Vec<PathBuf>> {
        let mut map = self.inner.lock().unwrap();
        map.remove(token).filter(|(_, at)| at.elapsed() < IMPORT_TOKEN_TTL).map(|(p, _)| p)
    }
}

pub struct DesktopState {
    pub svc: Arc<Services>,
    pub scheduler: Arc<JobScheduler>,
    pub generation: Arc<crate::application::generation::GenerationRunner>,
    /// Which provider each running sign-in flow belongs to.
    pub sign_in_kinds: Mutex<HashMap<String, crate::application::ports::ProviderKind>>,
    pub events: Arc<TauriEventSink>,
    pub import_tokens: ImportTokens,
    pub db: Arc<Db>,
}
