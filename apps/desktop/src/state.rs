use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use dawn_app_core::app_model::AppModel;
use dawn_project::path::Utf8PathBuf;
use tauri::State;

use crate::audio_runtime::AudioRuntime;
use crate::effect_previews::{EffectPreviewCacheKey, SequenceEffectPreviewDto};
use crate::preview_transport::PreviewTransportRuntime;

pub(crate) struct AppState {
    pub(crate) model: Mutex<AppModel>,
    audio_runtime: Mutex<AudioRuntime>,
    effect_preview_cache: Mutex<HashMap<EffectPreviewCacheKey, SequenceEffectPreviewDto>>,
    preview_transport: Mutex<PreviewTransportRuntime>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            model: Mutex::new(AppModel::default()),
            audio_runtime: Mutex::new(AudioRuntime::default()),
            effect_preview_cache: Mutex::new(HashMap::new()),
            preview_transport: Mutex::new(PreviewTransportRuntime::default()),
        }
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

pub(crate) fn lock_preview_transport<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<MutexGuard<'a, PreviewTransportRuntime>> {
    state
        .preview_transport
        .lock()
        .map_err(|_| "preview transport lock is poisoned".to_string())
}

pub(crate) fn project_path(path: String) -> Utf8PathBuf {
    Utf8PathBuf::from(path)
}
