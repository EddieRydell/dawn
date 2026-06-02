use std::time::Instant;

use dawn_app_core::actions::AppAction;
use dawn_app_core::app_model::{AppModel, DispatchOutcome};
use dawn_app_core::dto::{AppSnapshotDto, PreviewSnapshotDto};
use dawn_app_core::preview_session::{AudioPlaybackStatus, PreviewSnapshot};
use dawn_project::document::SequenceAudioDocument;
use tauri::{AppHandle, Emitter, State};

use crate::audio_runtime::AudioClock;
use crate::preview::{PreviewStateEventDto, PreviewTimingDto};
use crate::state::{
    lock_audio_runtime, lock_filesystem_watcher, lock_model, AppState, CommandResult,
};

pub(crate) fn dispatch(
    app: &AppHandle,
    state: &State<'_, AppState>,
    action: AppAction,
) -> CommandResult<AppSnapshotDto> {
    let total_started = Instant::now();
    let clear_audio_runtime = should_clear_audio_runtime_for_action(&action);
    let lock_started = Instant::now();
    let mut model = lock_model(state)?;
    let lock_ms = elapsed_ms(lock_started);
    let dispatch_started = Instant::now();
    let outcome = model.dispatch(action)?;
    let dispatch_ms = elapsed_ms(dispatch_started);
    let snapshot_started = Instant::now();
    let snapshot = model.snapshot_dto();
    let snapshot_ms = elapsed_ms(snapshot_started);
    if outcome == DispatchOutcome::SnapshotChanged {
        let watcher_started = Instant::now();
        if let Ok(mut watcher) = lock_filesystem_watcher(state) {
            let _ = watcher.sync_project_root(app, snapshot.project_root.clone());
        }
        let watcher_ms = elapsed_ms(watcher_started);
        let audio_clear_started = Instant::now();
        if clear_audio_runtime {
            if let Ok(runtime) = lock_audio_runtime(state) {
                runtime.clear();
            }
        }
        let audio_clear_ms = elapsed_ms(audio_clear_started);
        let audio_load_started = Instant::now();
        if let Some(clock) = sync_active_audio_load(state, &snapshot.preview) {
            let audio_load_ms = elapsed_ms(audio_load_started);
            let audio_apply_started = Instant::now();
            let analysis = model.analysis.clone();
            apply_audio_clock_to_model(&mut model, &clock, analysis.as_ref());
            let audio_apply_ms = elapsed_ms(audio_apply_started);
            let second_snapshot_started = Instant::now();
            let snapshot = model.snapshot_dto();
            let second_snapshot_ms = elapsed_ms(second_snapshot_started);
            let emit_started = Instant::now();
            app.emit("app_snapshot_changed", &snapshot)
                .map_err(|error| error.to_string())?;
            emit_preview_state_dto(app, &snapshot)?;
            let emit_ms = elapsed_ms(emit_started);
            eprintln!(
                "[dispatch] changed with_audio lock_ms={:.3} dispatch_ms={:.3} snapshot_ms={:.3} watcher_ms={:.3} audio_clear_ms={:.3} audio_load_ms={:.3} audio_apply_ms={:.3} second_snapshot_ms={:.3} emit_ms={:.3} total_ms={:.3}",
                lock_ms,
                dispatch_ms,
                snapshot_ms,
                watcher_ms,
                audio_clear_ms,
                audio_load_ms,
                audio_apply_ms,
                second_snapshot_ms,
                emit_ms,
                elapsed_ms(total_started),
            );
            return Ok(snapshot);
        }
        let audio_load_ms = elapsed_ms(audio_load_started);
        let emit_started = Instant::now();
        app.emit("app_snapshot_changed", &snapshot)
            .map_err(|error| error.to_string())?;
        emit_preview_state_dto(app, &snapshot)?;
        let emit_ms = elapsed_ms(emit_started);
        eprintln!(
            "[dispatch] changed lock_ms={:.3} dispatch_ms={:.3} snapshot_ms={:.3} watcher_ms={:.3} audio_clear_ms={:.3} audio_load_ms={:.3} emit_ms={:.3} total_ms={:.3}",
            lock_ms,
            dispatch_ms,
            snapshot_ms,
            watcher_ms,
            audio_clear_ms,
            audio_load_ms,
            emit_ms,
            elapsed_ms(total_started),
        );
    } else {
        eprintln!(
            "[dispatch] unchanged lock_ms={:.3} dispatch_ms={:.3} snapshot_ms={:.3} total_ms={:.3}",
            lock_ms,
            dispatch_ms,
            snapshot_ms,
            elapsed_ms(total_started),
        );
    }
    Ok(snapshot)
}

fn should_clear_audio_runtime_for_action(action: &AppAction) -> bool {
    matches!(action, AppAction::OpenProject(_))
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
        model
            .preview
            .set_timing_status("nativeAudio", AudioPlaybackStatus::Error);
        model.status = format!("Audio error: {error}");
        return;
    }
    if clock.ended {
        model
            .preview
            .render_at_native_audio_clock(clock.position_seconds, true, analysis);
        model
            .preview
            .set_timing_status("nativeAudio", AudioPlaybackStatus::Ended);
        model.status = "Preview complete".to_string();
        return;
    }
    match clock.status {
        AudioPlaybackStatus::Loading => {
            model.preview.pause_at(clock.position_seconds, analysis);
            model
                .preview
                .set_timing_status("nativeAudio", AudioPlaybackStatus::Loading);
            model.status = "Loading audio".to_string();
        }
        AudioPlaybackStatus::LoadingToPlay => {
            model.preview.pause_at(clock.position_seconds, analysis);
            model
                .preview
                .set_timing_status("nativeAudio", AudioPlaybackStatus::LoadingToPlay);
            model.status = "Loading audio - will play".to_string();
        }
        AudioPlaybackStatus::Playing => {
            model
                .preview
                .play_from_native_audio_clock(clock.position_seconds, analysis);
            model
                .preview
                .set_timing_status("nativeAudio", AudioPlaybackStatus::Playing);
            model.status = "Preview playing".to_string();
        }
        AudioPlaybackStatus::Ended => {
            model
                .preview
                .render_at_native_audio_clock(clock.position_seconds, true, analysis);
            model
                .preview
                .set_timing_status("nativeAudio", AudioPlaybackStatus::Ended);
            model.status = "Preview complete".to_string();
        }
        AudioPlaybackStatus::Missing => {
            model.preview.pause_at(clock.position_seconds, analysis);
            model
                .preview
                .set_timing_status("silent", AudioPlaybackStatus::Missing);
            model.status = "Audio missing".to_string();
        }
        AudioPlaybackStatus::None => {
            model.preview.pause_at(clock.position_seconds, analysis);
            model
                .preview
                .set_timing_status("silent", AudioPlaybackStatus::None);
            model.status = "Preview ready".to_string();
        }
        AudioPlaybackStatus::Ready | AudioPlaybackStatus::Error => {
            model.preview.pause_at(clock.position_seconds, analysis);
            model.preview.set_timing_status("nativeAudio", clock.status);
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
            return runtime.preload(&audio).ok();
        }
    }
    None
}

pub(crate) fn emit_model_snapshot(
    app: &AppHandle,
    model: &AppModel,
) -> CommandResult<AppSnapshotDto> {
    let total_started = Instant::now();
    let snapshot_started = Instant::now();
    let snapshot = model.snapshot_dto();
    let snapshot_ms = elapsed_ms(snapshot_started);
    let emit_started = Instant::now();
    app.emit("app_snapshot_changed", &snapshot)
        .map_err(|error| error.to_string())?;
    emit_preview_state_dto(app, &snapshot)?;
    eprintln!(
        "[dispatch] emit_model_snapshot snapshot_ms={:.3} emit_ms={:.3} total_ms={:.3}",
        snapshot_ms,
        elapsed_ms(emit_started),
        elapsed_ms(total_started),
    );
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
            effect_preview_active: snapshot.preview.effect_preview_active,
            position_seconds: snapshot.preview.position_seconds,
            home_seconds: snapshot.preview.home_seconds,
            duration_seconds: snapshot.preview.duration_seconds,
            audio: snapshot.preview.audio.clone(),
            clock_source: snapshot.preview.clock_source.clone(),
            audio_playback_status: snapshot.preview.audio_playback_status,
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

pub(crate) fn valid_sequence_audio(snapshot: &PreviewSnapshot) -> Option<SequenceAudioDocument> {
    snapshot
        .audio
        .as_ref()
        .filter(|audio| audio.exists)
        .cloned()
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}
