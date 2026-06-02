use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use dawn_app_core::app_model::AppModel;
use dawn_app_core::output_runtime::SequencePreparationCache;
use dawn_project::path::Utf8PathBuf;
use tauri::State;

use crate::audio_runtime::AudioRuntime;
use crate::effect_previews::{EffectPreviewCacheKey, SequenceEffectPreviewDto};
use crate::filesystem_watcher::FilesystemWatcherRuntime;
use crate::live_output::LiveOutputRuntime;
use crate::preview_transport::PreviewTransportRuntime;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SequencePreparationCacheKey {
    pub(crate) sequence_path: String,
    pub(crate) object_key: String,
}

pub(crate) struct AppState {
    pub(crate) model: Mutex<AppModel>,
    audio_runtime: Mutex<AudioRuntime>,
    effect_preview_cache: Mutex<HashMap<EffectPreviewCacheKey, SequenceEffectPreviewDto>>,
    effect_preview_preparation_cache:
        Mutex<HashMap<SequencePreparationCacheKey, SequencePreparationCache>>,
    preview_transport: Mutex<PreviewTransportRuntime>,
    live_output: Mutex<LiveOutputRuntime>,
    filesystem_watcher: Mutex<FilesystemWatcherRuntime>,
    shutting_down: AtomicBool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            model: Mutex::new(AppModel::default()),
            audio_runtime: Mutex::new(AudioRuntime::default()),
            effect_preview_cache: Mutex::new(HashMap::new()),
            effect_preview_preparation_cache: Mutex::new(HashMap::new()),
            preview_transport: Mutex::new(PreviewTransportRuntime::default()),
            live_output: Mutex::new(LiveOutputRuntime::default()),
            filesystem_watcher: Mutex::new(FilesystemWatcherRuntime::default()),
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
}

pub(crate) type CommandResult<T> = Result<T, String>;

pub(crate) fn lock_model<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<MutexGuard<'a, AppModel>> {
    state
        .model
        .lock()
        .map_err(|_| "application state lock is poisoned".to_string())
}

pub(crate) fn lock_audio_runtime<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<MutexGuard<'a, AudioRuntime>> {
    state
        .audio_runtime
        .lock()
        .map_err(|_| "audio runtime lock is poisoned".to_string())
}

pub(crate) fn lock_effect_preview_cache<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<MutexGuard<'a, HashMap<EffectPreviewCacheKey, SequenceEffectPreviewDto>>> {
    state
        .effect_preview_cache
        .lock()
        .map_err(|_| "effect preview cache lock is poisoned".to_string())
}

pub(crate) fn lock_effect_preview_preparation_cache<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<MutexGuard<'a, HashMap<SequencePreparationCacheKey, SequencePreparationCache>>> {
    state
        .effect_preview_preparation_cache
        .lock()
        .map_err(|_| "effect preview preparation cache lock is poisoned".to_string())
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

pub(crate) fn project_path(path: String) -> Utf8PathBuf {
    Utf8PathBuf::from(path)
}
