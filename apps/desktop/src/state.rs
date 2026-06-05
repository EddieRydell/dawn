use std::sync::{Arc, Mutex, MutexGuard};

use dawn_backend::AppBackend;

use crate::{audio_runtime::AudioRuntime, preview_transport::PreviewTransportRuntime};

pub(crate) type CommandResult<T> = Result<T, String>;

#[derive(Clone)]
pub(crate) struct BackendState {
    backend: Arc<Mutex<AppBackend>>,
    audio: Arc<AudioRuntime>,
    preview_transport: Arc<Mutex<PreviewTransportRuntime>>,
}

impl Default for BackendState {
    fn default() -> Self {
        Self {
            backend: Arc::new(Mutex::new(AppBackend::new())),
            audio: Arc::new(AudioRuntime::default()),
            preview_transport: Arc::new(Mutex::new(PreviewTransportRuntime::default())),
        }
    }
}

impl BackendState {
    pub(crate) fn backend(&self) -> Arc<Mutex<AppBackend>> {
        Arc::clone(&self.backend)
    }

    pub(crate) fn audio(&self) -> Arc<AudioRuntime> {
        Arc::clone(&self.audio)
    }

    pub(crate) fn lock_preview_transport(
        &self,
    ) -> CommandResult<MutexGuard<'_, PreviewTransportRuntime>> {
        self.preview_transport
            .lock()
            .map_err(|_| "preview transport lock is poisoned".to_string())
    }

    pub(crate) fn lock_backend(&self) -> CommandResult<MutexGuard<'_, AppBackend>> {
        self.backend
            .lock()
            .map_err(|_| "backend lock is poisoned".to_string())
    }
}

pub(crate) type AppState = BackendState;
