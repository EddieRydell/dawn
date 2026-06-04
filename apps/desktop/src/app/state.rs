use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use dawn_app_runtime::runtime::coordinator::AppCoordinator;
use dawn_language::path::Utf8PathBuf;
use tauri::State;

use crate::preview::audio_runtime::AudioRuntime;
use crate::preview::effect_preview_runtime::EffectPreviewRuntime;
use crate::preview::live_output::LiveOutputRuntime;
use crate::preview::transport::PreviewTransportRuntime;
use crate::project::watcher::FilesystemWatcherRuntime;

pub(crate) struct AppState {
    audio_runtime: Mutex<AudioRuntime>,
    effect_preview_runtime: Mutex<EffectPreviewRuntime>,
    preview_transport: Mutex<PreviewTransportRuntime>,
    live_output: Mutex<LiveOutputRuntime>,
    filesystem_watcher: Mutex<FilesystemWatcherRuntime>,
    runtime: Mutex<AppCoordinator>,
    startup_hydrated: AtomicBool,
    shutting_down: AtomicBool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            audio_runtime: Mutex::new(AudioRuntime::default()),
            effect_preview_runtime: Mutex::new(EffectPreviewRuntime::default()),
            preview_transport: Mutex::new(PreviewTransportRuntime::default()),
            live_output: Mutex::new(LiveOutputRuntime::default()),
            filesystem_watcher: Mutex::new(FilesystemWatcherRuntime::default()),
            runtime: Mutex::new(AppCoordinator::default()),
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

pub(crate) fn lock_audio_runtime<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<MutexGuard<'a, AudioRuntime>> {
    state
        .audio_runtime
        .lock()
        .map_err(|_| "audio runtime lock is poisoned".to_string())
}

pub(crate) fn lock_effect_preview_runtime<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<MutexGuard<'a, EffectPreviewRuntime>> {
    state
        .effect_preview_runtime
        .lock()
        .map_err(|_| "effect preview runtime lock is poisoned".to_string())
}

pub(crate) fn lock_preview_transport<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<MutexGuard<'a, PreviewTransportRuntime>> {
    state
        .preview_transport
        .lock()
        .map_err(|_| "preview transport lock is poisoned".to_string())
}

pub(crate) fn lock_live_output<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<MutexGuard<'a, LiveOutputRuntime>> {
    state
        .live_output
        .lock()
        .map_err(|_| "live output lock is poisoned".to_string())
}

pub(crate) fn lock_filesystem_watcher<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<MutexGuard<'a, FilesystemWatcherRuntime>> {
    state
        .filesystem_watcher
        .lock()
        .map_err(|_| "filesystem watcher lock is poisoned".to_string())
}

pub(crate) fn lock_runtime<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<MutexGuard<'a, AppCoordinator>> {
    state
        .runtime
        .lock()
        .map_err(|_| "runtime lock is poisoned".to_string())
}

pub(crate) fn project_path(path: String) -> Utf8PathBuf {
    Utf8PathBuf::from(path)
}
