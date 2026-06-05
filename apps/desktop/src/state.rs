use std::sync::{Arc, Mutex, MutexGuard};

use dawn_backend::AppBackend;

pub(crate) type CommandResult<T> = Result<T, String>;

#[derive(Debug, Clone)]
pub(crate) struct BackendState {
    backend: Arc<Mutex<AppBackend>>,
}

impl Default for BackendState {
    fn default() -> Self {
        Self {
            backend: Arc::new(Mutex::new(AppBackend::new())),
        }
    }
}

impl BackendState {
    pub(crate) fn backend(&self) -> Arc<Mutex<AppBackend>> {
        Arc::clone(&self.backend)
    }

    pub(crate) fn lock_backend(&self) -> CommandResult<MutexGuard<'_, AppBackend>> {
        self.backend
            .lock()
            .map_err(|_| "backend lock is poisoned".to_string())
    }
}

pub(crate) type AppState = BackendState;
