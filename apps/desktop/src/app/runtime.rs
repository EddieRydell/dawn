use dawn_app_runtime::dto::{
    AppRuntimeChangedDto, AppSnapshotDto, PreviewSnapshotDto, RuntimeSliceDto,
};
use dawn_app_runtime::preview::session::{AudioPlaybackStatus, PreviewSnapshot};
use dawn_app_runtime::runtime::coordinator::AppCoordinator;
use dawn_language::document::SequenceAudioDocument;
use tauri::{AppHandle, Emitter, State};

use crate::app::state::{lock_audio_runtime, lock_runtime, AppState, CommandResult};
use crate::preview::audio_runtime::AudioClock;
use crate::preview::{PreviewStateEventDto, PreviewTimingDto};

pub(crate) fn update_preview_from_audio_status(
    app: &AppHandle,
    state: &State<'_, AppState>,
    clock: AudioClock,
) -> CommandResult<AppSnapshotDto> {
    let snapshot = {
        let mut runtime = lock_runtime(state)?;
        apply_audio_clock_to_runtime(&mut runtime, &clock);
        runtime.app_snapshot()
    };
    emit_runtime_read_models(app, snapshot)
}

pub(crate) fn apply_audio_clock_to_runtime(coordinator: &mut AppCoordinator, clock: &AudioClock) {
    coordinator.apply_audio_clock_state(
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
    read_models: AppSnapshotDto,
) -> CommandResult<AppSnapshotDto> {
    app.emit(
        "app_runtime_changed",
        AppRuntimeChangedDto {
            snapshot: read_models.clone(),
            changed_slices: RuntimeSliceDto::all(),
        },
    )
    .map_err(|error| error.to_string())?;
    emit_preview_state_dto(app, &read_models)?;
    Ok(read_models)
}

pub(crate) fn emit_preview_state_dto(
    app: &AppHandle,
    read_models: &AppSnapshotDto,
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
