use std::time::Instant;

use dawn_app_core::actions::AppAction;
use dawn_app_core::app_model::{AppModel, CommandTiming, DispatchOutcome};
use dawn_app_core::document::SequenceAudioDocument;
use dawn_app_core::dto::{AppSnapshotDto, EditorViewModeDto, SequenceTransportSnapshotDto};
use dawn_app_core::sequence_transport_session::{
    AudioPlaybackStatus, NativeAudioClockProjection, SequenceTransportSnapshot,
};
use tauri::{AppHandle, Emitter, State};

use crate::audio_runtime::AudioClock;
use crate::sequence_runtime::{SequenceRenderEventDto, SequenceRenderTimingDto};
use crate::state::{
    lock_audio_runtime, lock_filesystem_watcher, lock_model, lock_project_autosave_runtime,
    AppState, CommandResult,
};

pub(crate) fn dispatch(
    app: &AppHandle,
    state: &State<'_, AppState>,
    action: AppAction,
) -> CommandResult<AppSnapshotDto> {
    let total_started = Instant::now();
    if should_flush_project_autosave_before_action(&action) {
        flush_project_autosave_worker(app, state)?;
    } else {
        drain_project_autosave_completions(app, state)?;
    }
    let clear_audio_runtime = should_clear_audio_runtime_for_action(&action);
    let model_lock_started = Instant::now();
    let mut model = lock_model(state)?;
    let model_lock_wait_ms = elapsed_ms(model_lock_started);
    let dispatch_started = Instant::now();
    let outcome = model.dispatch(action)?;
    let dispatch_ms = elapsed_ms(dispatch_started);
    let snapshot_started = Instant::now();
    let snapshot = model.snapshot_dto();
    let snapshot_ms = elapsed_ms(snapshot_started);
    let mut timing = CommandTiming {
        total_ms: 0.0,
        model_lock_wait_ms,
        dispatch_ms,
        snapshot_ms,
        app_snapshot_emit_ms: 0.0,
    };
    let snapshot_changed = outcome == DispatchOutcome::SnapshotChanged;
    if snapshot_changed {
        if let Ok(mut watcher) = lock_filesystem_watcher(state) {
            let _ = watcher.sync_project_root(app, snapshot.project_root.clone());
        }
        if clear_audio_runtime {
            if let Ok(runtime) = lock_audio_runtime(state) {
                runtime.clear();
            }
        }
        preload_active_sequence_audio(state, &snapshot.sequence_transport);
        let app_snapshot_emit_started = Instant::now();
        app.emit("app_snapshot_changed", &snapshot)
            .map_err(|error| error.to_string())?;
        timing.app_snapshot_emit_ms = elapsed_ms(app_snapshot_emit_started);
        timing.total_ms = elapsed_ms(total_started);
        model.set_last_command_timing(timing);
    } else {
        timing.total_ms = elapsed_ms(total_started);
        model.set_last_command_timing(timing);
    }
    drop(model);
    if snapshot_changed {
        schedule_project_autosave(app, state)?;
    }
    Ok(snapshot)
}

fn should_flush_project_autosave_before_action(action: &AppAction) -> bool {
    matches!(
        action,
        AppAction::OpenProject(_)
            | AppAction::ReloadProject
            | AppAction::FlushAutosave
            | AppAction::CreateFile { .. }
            | AppAction::CreateDirectory { .. }
            | AppAction::RenamePath { .. }
            | AppAction::DeletePath(_)
            | AppAction::SetActiveViewMode(EditorViewModeDto::Text)
    )
}

fn should_clear_audio_runtime_for_action(action: &AppAction) -> bool {
    matches!(action, AppAction::OpenProject(_))
}

pub(crate) fn schedule_project_autosave(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> CommandResult<()> {
    drain_project_autosave_completions(app, state)?;
    let job = {
        let mut model = lock_model(state)?;
        model.begin_project_autosave_job()
    };
    if let Some(job) = job {
        lock_project_autosave_runtime(state)?.request(job)?;
    }
    Ok(())
}

pub(crate) fn flush_autosave_blocking(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> CommandResult<()> {
    flush_project_autosave_worker(app, state)?;
    let mut model = lock_model(state)?;
    model.flush_autosave()
}

fn flush_project_autosave_worker(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> CommandResult<()> {
    loop {
        drain_project_autosave_completions(app, state)?;
        let job = {
            let mut model = lock_model(state)?;
            if !model.has_pending_project_autosave() {
                return Ok(());
            }
            model.begin_project_autosave_job()
        };
        if let Some(job) = job {
            lock_project_autosave_runtime(state)?.request(job)?;
        }
        let completion = lock_project_autosave_runtime(state)?.complete()?;
        complete_project_autosave(app, state, completion, true)?;
    }
}

fn drain_project_autosave_completions(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> CommandResult<()> {
    loop {
        let completion = lock_project_autosave_runtime(state)?.try_complete()?;
        let Some(completion) = completion else {
            return Ok(());
        };
        complete_project_autosave(app, state, completion, false)?;
    }
}

fn complete_project_autosave(
    app: &AppHandle,
    state: &State<'_, AppState>,
    completion: dawn_app_core::app_model::ProjectAutosaveCompletion,
    propagate_error: bool,
) -> CommandResult<()> {
    let result = {
        let mut model = lock_model(state)?;
        let result = model.complete_project_autosave(completion);
        let _ = emit_model_snapshot(app, &model);
        result
    };
    match result {
        Ok(should_schedule_next) => {
            if should_schedule_next {
                schedule_project_autosave(app, state)?;
            }
            Ok(())
        }
        Err(error) if propagate_error => Err(error),
        Err(_) => Ok(()),
    }
}

pub(crate) fn update_sequence_transport_from_audio_status(
    app: &AppHandle,
    state: &State<'_, AppState>,
    clock: AudioClock,
) -> CommandResult<AppSnapshotDto> {
    let mut model = lock_model(state)?;
    let project = model.project.clone();
    apply_audio_clock_to_model(&mut model, &clock, project.as_deref());
    emit_model_snapshot(app, &model)
}

pub(crate) fn update_sequence_transport_from_audio_seek(
    app: &AppHandle,
    state: &State<'_, AppState>,
    clock: AudioClock,
    home_seconds: f64,
) -> CommandResult<AppSnapshotDto> {
    let mut model = lock_model(state)?;
    let project = model.project.clone();
    model.sequence_transport.set_playhead_home(home_seconds);
    apply_audio_clock_to_model(&mut model, &clock, project.as_deref());
    emit_model_snapshot(app, &model)
}

pub(crate) fn apply_audio_clock_to_model(
    model: &mut AppModel,
    clock: &AudioClock,
    project: Option<&dawn_project::DawnProject>,
) {
    if let Some(error) = &clock.error {
        model.sequence_transport.apply_native_audio_clock(
            NativeAudioClockProjection {
                position_seconds: clock.position_seconds,
                status: AudioPlaybackStatus::Error,
                ended: false,
            },
            project,
        );
        model.status = format!("Audio error: {error}");
        return;
    }
    model.sequence_transport.apply_native_audio_clock(
        NativeAudioClockProjection {
            position_seconds: clock.position_seconds,
            status: clock.status,
            ended: clock.ended,
        },
        project,
    );
    model.status = match clock.status {
        AudioPlaybackStatus::Playing => "Sequence playing",
        AudioPlaybackStatus::Ended => "Sequence complete",
        AudioPlaybackStatus::Missing => "Audio missing",
        AudioPlaybackStatus::Error => "Audio error",
        AudioPlaybackStatus::None | AudioPlaybackStatus::Ready => "Sequence ready",
    }
    .to_string();
}

pub(crate) fn preload_active_sequence_audio(
    state: &State<'_, AppState>,
    transport: &SequenceTransportSnapshotDto,
) {
    let Some(audio) = transport.audio.as_ref() else {
        if transport.source_label != "No sequence source" {
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
    if transport.audio_playback_status != AudioPlaybackStatus::Playing {
        if let Ok(runtime) = lock_audio_runtime(state) {
            let _clock = runtime.preload(&audio);
        }
    }
}

pub(crate) fn emit_model_snapshot(
    app: &AppHandle,
    model: &AppModel,
) -> CommandResult<AppSnapshotDto> {
    let snapshot = model.snapshot_dto();
    app.emit("app_snapshot_changed", &snapshot)
        .map_err(|error| error.to_string())?;
    Ok(snapshot)
}

pub(crate) fn emit_sequence_render_snapshot(
    app: &AppHandle,
    snapshot: &SequenceTransportSnapshot,
    timing: SequenceRenderTimingDto,
) {
    let _ = app.emit(
        "sequence_render_state_changed",
        SequenceRenderEventDto {
            source_label: snapshot.source_label.clone(),
            source_key: snapshot.source_key.clone().map(Into::into),
            render_generation: saturating_u32(snapshot.render_generation),
            render_dirty_revision: saturating_u32(snapshot.render_dirty_revision),
            render_updating: snapshot.render_updating,
            position_seconds: snapshot.frame.time_seconds,
            geometry_identity: snapshot.geometry.geometry_id.clone(),
            timing,
        },
    );
}

pub(crate) fn sequence_transport_audio(
    snapshot: &SequenceTransportSnapshot,
) -> Option<SequenceAudioDocument> {
    snapshot
        .audio
        .as_ref()
        .filter(|audio| audio.exists)
        .cloned()
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn saturating_u32(value: u64) -> u32 {
    value.min(u32::MAX as u64) as u32
}
