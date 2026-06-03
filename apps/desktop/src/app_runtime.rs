use dawn_app_runtime::dto::{PreviewSnapshotDto, RuntimeReadModelsDto};
use dawn_app_runtime::preview_session::{AudioPlaybackStatus, PreviewSnapshot};
use dawn_app_runtime::services::app_state::RuntimeState;
use dawn_project::document::SequenceAudioDocument;
use tauri::{AppHandle, Emitter, State};

use crate::audio_runtime::AudioClock;
use crate::preview::{PreviewStateEventDto, PreviewTimingDto};
use crate::state::{lock_audio_runtime, lock_runtime, AppState, CommandResult};

pub(crate) fn update_preview_from_audio_status(
    app: &AppHandle,
    state: &State<'_, AppState>,
    clock: AudioClock,
) -> CommandResult<RuntimeReadModelsDto> {
    let mut model = lock_runtime(state)?;
    let model = model.runtime_state_mut();
    apply_audio_clock_to_runtime(model, &clock);
    emit_runtime_read_models(app, model)
}

pub(crate) fn apply_audio_clock_to_runtime(model: &mut RuntimeState, clock: &AudioClock) {
    model.apply_audio_clock_state(
        clock.position_seconds,
        clock.status,
        clock.ended,
        clock.error.as_deref(),
    );
}

pub(crate) fn preload_active_preview_audio(
    state: &State<'_, AppState>,
    preview: &PreviewSnapshotDto,
) {
    let Some(audio) = preview.audio.as_ref() else {
        if preview.source_label != "No preview source" {
            if let Ok(runtime) = lock_audio_runtime(state) {
                runtime.clear();
            }
        }
        return;
    };
    let audio = SequenceAudioDocument {
        import: audio.import.clone(),
        resolved_path: audio.resolved_path.clone(),
        file_name: audio.file_name.clone(),
        exists: audio.exists,
    };
    if !preview.is_playing
        && !matches!(
            preview.audio_playback_status,
            AudioPlaybackStatus::Loading
                | AudioPlaybackStatus::LoadingToPlay
                | AudioPlaybackStatus::Playing
        )
    {
        if let Ok(runtime) = lock_audio_runtime(state) {
            let _clock = runtime.preload(&audio);
        }
    }
}

pub(crate) fn emit_runtime_read_models(
    app: &AppHandle,
    model: &RuntimeState,
) -> CommandResult<RuntimeReadModelsDto> {
    let read_models = RuntimeReadModelsDto::from(model);
    app.emit("runtime_workspace_changed", &read_models.workspace)
        .map_err(|error| error.to_string())?;
    app.emit("runtime_editor_changed", &read_models.editor)
        .map_err(|error| error.to_string())?;
    app.emit(
        "runtime_active_document_changed",
        &read_models.active_document,
    )
    .map_err(|error| error.to_string())?;
    app.emit("runtime_diagnostics_changed", &read_models.diagnostics)
        .map_err(|error| error.to_string())?;
    app.emit("runtime_preview_changed", &read_models.preview)
        .map_err(|error| error.to_string())?;
    app.emit("runtime_live_output_changed", &read_models.live_output)
        .map_err(|error| error.to_string())?;
    app.emit("runtime_status_changed", &read_models.status)
        .map_err(|error| error.to_string())?;
    app.emit("runtime_prefs_changed", &read_models.prefs)
        .map_err(|error| error.to_string())?;
    emit_preview_state_dto(app, &read_models)?;
    Ok(read_models)
}

pub(crate) fn emit_preview_state_dto(
    app: &AppHandle,
    read_models: &RuntimeReadModelsDto,
) -> CommandResult<()> {
    let snapshot = &read_models.preview.preview;
    app.emit(
        "preview_state_changed",
        PreviewStateEventDto {
            source_label: snapshot.source_label.clone(),
            is_playing: snapshot.is_playing,
            preview_updating: snapshot.preview_updating,
            effect_preview_active: snapshot.effect_preview_active,
            position_seconds: snapshot.position_seconds,
            home_seconds: snapshot.home_seconds,
            duration_seconds: snapshot.duration_seconds,
            audio: snapshot.audio.clone(),
            clock_source: snapshot.clock_source.clone(),
            audio_playback_status: snapshot.audio_playback_status,
            status: snapshot.status.clone(),
            timing: PreviewTimingDto::empty(0.0),
        },
    )
    .map_err(|error| error.to_string())
}

pub(crate) fn emit_preview_state_snapshot(
    app: &AppHandle,
    snapshot: &PreviewSnapshot,
    timing: PreviewTimingDto,
) {
    let _ = app.emit(
        "preview_state_changed",
        PreviewStateEventDto {
            source_label: snapshot.source_label.clone(),
            is_playing: snapshot.is_playing,
            preview_updating: snapshot.preview_updating,
            effect_preview_active: snapshot.effect_preview_active,
            position_seconds: snapshot.position_seconds,
            home_seconds: snapshot.home_seconds,
            duration_seconds: snapshot.duration_seconds,
            audio: snapshot.audio.clone().map(Into::into),
            clock_source: snapshot.clock_source.clone(),
            audio_playback_status: snapshot.audio_playback_status,
            status: snapshot.status.clone(),
            timing,
        },
    );
}

pub(crate) fn valid_preview_audio(snapshot: &PreviewSnapshot) -> Option<SequenceAudioDocument> {
    snapshot
        .audio
        .as_ref()
        .filter(|audio| audio.exists)
        .cloned()
}
