use dawn_app_core::actions::AppAction;
use dawn_app_core::app_model::{AppModel, DispatchOutcome};
use dawn_app_core::dto::{AppSnapshotDto, PreviewSnapshotDto};
use dawn_app_core::preview_session::PreviewSnapshot;
use dawn_project::document::SequenceAudioDocument;
use tauri::{AppHandle, Emitter, State};

use crate::audio_runtime::AudioClock;
use crate::preview_types::{PreviewStateEventDto, PreviewTimingDto};
use crate::state::{lock_audio_runtime, lock_model, AppState, CommandResult};

pub(crate) fn dispatch(
    app: &AppHandle,
    state: &State<'_, AppState>,
    action: AppAction,
) -> CommandResult<AppSnapshotDto> {
    let clear_audio_runtime = should_clear_audio_runtime_for_action(&action);
    let mut model = lock_model(state)?;
    let outcome = model.dispatch(action)?;
    let snapshot = model.snapshot_dto();
    if outcome == DispatchOutcome::SnapshotChanged {
        if clear_audio_runtime {
            if let Ok(runtime) = lock_audio_runtime(state) {
                runtime.clear();
            }
        }
        if let Some(clock) = sync_active_audio_load(state, &snapshot.preview) {
            let analysis = model.analysis.clone();
            apply_audio_clock_to_model(&mut model, &clock, analysis.as_ref());
            let snapshot = model.snapshot_dto();
            app.emit("app_snapshot_changed", &snapshot)
                .map_err(|error| error.to_string())?;
            emit_preview_state_dto(app, &snapshot)?;
            return Ok(snapshot);
        }
        app.emit("app_snapshot_changed", &snapshot)
            .map_err(|error| error.to_string())?;
        emit_preview_state_dto(app, &snapshot)?;
    }
    Ok(snapshot)
}

fn should_clear_audio_runtime_for_action(action: &AppAction) -> bool {
    matches!(
        action,
        AppAction::OpenProject(_)
            | AppAction::ReloadProject
            | AppAction::CloseFile(_)
            | AppAction::RenamePath { .. }
            | AppAction::DeletePath(_)
    )
}

pub(crate) fn update_preview_from_audio_status(
    app: &AppHandle,
    state: &State<'_, AppState>,
    clock: AudioClock,
) -> CommandResult<AppSnapshotDto> {
    let mut model = lock_model(state)?;
    let analysis = model.analysis.clone();
    apply_audio_clock_to_model(&mut model, &clock, analysis.as_ref());
    emit_model_snapshot(app, &model)
}

pub(crate) fn apply_audio_clock_to_model(
    model: &mut AppModel,
    clock: &AudioClock,
    analysis: Option<&dawn_project::analysis::ProjectAnalysis>,
) {
    if let Some(error) = &clock.error {
        model.preview.pause_at(clock.position_seconds, analysis);
        model.preview.set_timing_status("nativeAudio", "error");
        model.status = format!("Audio error: {error}");
        return;
    }
    if clock.ended {
        model
            .preview
            .render_at_native_audio_clock(clock.position_seconds, true, analysis);
        model.preview.set_timing_status("nativeAudio", "ended");
        model.status = "Preview complete".to_string();
        return;
    }
    match clock.status.as_str() {
        "loading" => {
            model.preview.pause_at(clock.position_seconds, analysis);
            model.preview.set_timing_status("nativeAudio", "loading");
            model.status = "Loading audio".to_string();
        }
        "playing" => {
            model
                .preview
                .play_from_native_audio_clock(clock.position_seconds, analysis);
            model.preview.set_timing_status("nativeAudio", "playing");
            model.status = "Preview playing".to_string();
        }
        "ended" => {
            model
                .preview
                .render_at_native_audio_clock(clock.position_seconds, true, analysis);
            model.preview.set_timing_status("nativeAudio", "ended");
            model.status = "Preview complete".to_string();
        }
        status => {
            model.preview.pause_at(clock.position_seconds, analysis);
            model.preview.set_timing_status("nativeAudio", status);
            model.status = "Preview ready".to_string();
        }
    }
}

pub(crate) fn sync_active_audio_load(
    state: &State<'_, AppState>,
    preview: &PreviewSnapshotDto,
) -> Option<AudioClock> {
    let Some(audio) = preview.audio.as_ref() else {
        if preview.source_label != "No preview source" {
            if let Ok(runtime) = lock_audio_runtime(state) {
                runtime.clear();
            }
        }
        return None;
    };
    if !audio.exists {
        if let Ok(runtime) = lock_audio_runtime(state) {
            runtime.clear();
        }
        return None;
    }
    let audio = SequenceAudioDocument {
        import: audio.import.clone(),
        resolved_path: audio.resolved_path.clone(),
        file_name: audio.file_name.clone(),
        exists: audio.exists,
    };
    if !preview.is_playing
        && !matches!(
            preview.audio_playback_status.as_str(),
            "loading" | "playing"
        )
    {
        if let Ok(runtime) = lock_audio_runtime(state) {
            return runtime.load_active(&audio).ok();
        }
    }
    None
}

pub(crate) fn emit_model_snapshot(
    app: &AppHandle,
    model: &AppModel,
) -> CommandResult<AppSnapshotDto> {
    let snapshot = model.snapshot_dto();
    app.emit("app_snapshot_changed", &snapshot)
        .map_err(|error| error.to_string())?;
    emit_preview_state_dto(app, &snapshot)?;
    Ok(snapshot)
}

pub(crate) fn emit_preview_state_dto(
    app: &AppHandle,
    snapshot: &AppSnapshotDto,
) -> CommandResult<()> {
    app.emit(
        "preview_state_changed",
        PreviewStateEventDto {
            source_label: snapshot.preview.source_label.clone(),
            is_playing: snapshot.preview.is_playing,
            position_seconds: snapshot.preview.position_seconds,
            home_seconds: snapshot.preview.home_seconds,
            duration_seconds: snapshot.preview.duration_seconds,
            audio: snapshot.preview.audio.clone(),
            clock_source: snapshot.preview.clock_source.clone(),
            audio_playback_status: snapshot.preview.audio_playback_status.clone(),
            status: snapshot.preview.status.clone(),
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
            position_seconds: snapshot.position_seconds,
            home_seconds: snapshot.home_seconds,
            duration_seconds: snapshot.duration_seconds,
            audio: snapshot.audio.clone().map(Into::into),
            clock_source: snapshot.clock_source.clone(),
            audio_playback_status: snapshot.audio_playback_status.clone(),
            status: snapshot.status.clone(),
            timing,
        },
    );
}

pub(crate) fn valid_sequence_audio(snapshot: &PreviewSnapshot) -> Option<SequenceAudioDocument> {
    snapshot
        .audio
        .as_ref()
        .filter(|audio| audio.exists)
        .cloned()
}
