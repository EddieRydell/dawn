use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use dawn_language::path::Utf8PathBuf;
use deprecated_dawn_backend::AppBackend;
use tauri::State;

use crate::preview::transport::PreviewTransportRuntime;

pub(crate) struct AppState {
    preview_transport: Mutex<PreviewTransportRuntime>,
    backend: Mutex<AppBackend>,
    startup_hydrated: AtomicBool,
    shutting_down: AtomicBool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            preview_transport: Mutex::new(PreviewTransportRuntime::default()),
            backend: Mutex::new(AppBackend::default()),
            startup_hydrated: AtomicBool::new(false),
            shutting_down: AtomicBool::new(false),
        }
    }
}

impl AppState {
    pub(crate) fn begin_shutdown(&self) {
        self.shutting_down.store(true, Ordering::Relaxed);
    }

    pub(crate) fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Relaxed)
    }

    pub(crate) fn mark_startup_hydrated(&self) -> bool {
        self.startup_hydrated.swap(true, Ordering::Relaxed)
    }
}

pub(crate) type CommandResult<T> = Result<T, String>;

pub(crate) fn lock_preview_transport<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<MutexGuard<'a, PreviewTransportRuntime>> {
    state
        .preview_transport
        .lock()
        .map_err(|_| "preview transport lock is poisoned".to_string())
}

pub(crate) fn lock_backend<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<MutexGuard<'a, AppBackend>> {
    state
        .backend
        .lock()
        .map_err(|_| "backend lock is poisoned".to_string())
}

pub(crate) fn project_path(path: String) -> Utf8PathBuf {
    Utf8PathBuf::from(path)
}
