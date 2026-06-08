use std::time::Instant;

use dawn_app_core::actions::AppAction;
use dawn_app_core::app_model::{AppModel, CommandTiming, DispatchOutcome};
use dawn_app_core::dto::{AppSnapshotDto, PreviewSnapshotDto};
use dawn_app_core::preview_session::{AudioPlaybackStatus, PreviewSnapshot};
use dawn_project::document::SequenceAudioDocument;
use tauri::{AppHandle, Emitter, State};

use crate::audio_runtime::AudioClock;
use crate::preview::{PreviewStateEventDto, PreviewTimingDto};
use crate::state::{
    lock_audio_runtime, lock_filesystem_watcher, lock_model, lock_terminal_runtime, AppState,
    CommandResult,
};

pub(crate) fn dispatch(
    app: &AppHandle,
    state: &State<'_, AppState>,
    action: AppAction,
) -> CommandResult<AppSnapshotDto> {
    let total_started = Instant::now();
    let clear_audio_runtime = should_clear_audio_runtime_for_action(&action);
    let clear_terminal_runtime = should_clear_terminal_runtime_for_action(&action);
    if clear_terminal_runtime {
        lock_terminal_runtime(state)?.kill_all();
    }
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
    if outcome == DispatchOutcome::SnapshotChanged {
        if let Ok(mut watcher) = lock_filesystem_watcher(state) {
            let _ = watcher.sync_project_root(app, snapshot.project_root.clone());
        }
        if clear_audio_runtime {
            if let Ok(runtime) = lock_audio_runtime(state) {
                runtime.clear();
            }
        }
        preload_active_preview_audio(state, &snapshot.preview);
        let app_snapshot_emit_started = Instant::now();
        app.emit("app_snapshot_changed", &snapshot)
            .map_err(|error| error.to_string())?;
        timing.app_snapshot_emit_ms = elapsed_ms(app_snapshot_emit_started);
        timing.total_ms = elapsed_ms(total_started);
        model.set_last_command_timing(timing);
        emit_preview_state_dto_with_timing(app, &snapshot, PreviewTimingDto::empty(0.0))?;
    } else {
        timing.total_ms = elapsed_ms(total_started);
        model.set_last_command_timing(timing);
    }
    Ok(snapshot)
}

fn should_clear_audio_runtime_for_action(action: &AppAction) -> bool {
    matches!(action, AppAction::OpenProject(_))
}

fn should_clear_terminal_runtime_for_action(action: &AppAction) -> bool {
    matches!(action, AppAction::OpenProject(_))
}

pub(crate) fn update_preview_from_audio_status(
    app: &AppHandle,
    state: &State<'_, AppState>,
    clock: AudioClock,
) -> CommandResult<AppSnapshotDto> {
    let mut model = lock_model(state)?;
    let analysis = model.analysis.clone();
    apply_audio_clock_to_model(&mut model, &clock, analysis.as_deref());
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
            model
                .preview
                .set_timing_status("nativeAudio", AudioPlaybackStatus::Loading);
            model.status = "Loading audio".to_string();
        }
        AudioPlaybackStatus::LoadingToPlay => {
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
            model
                .preview
                .set_timing_status("silent", AudioPlaybackStatus::Missing);
            model.status = "Audio missing".to_string();
        }
        AudioPlaybackStatus::None => {
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
    if !preview.transport_state.is_active_playback()
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
    emit_preview_state_dto_with_timing(app, snapshot, PreviewTimingDto::empty(0.0))
}

pub(crate) fn emit_preview_state_dto_with_timing(
    app: &AppHandle,
    snapshot: &AppSnapshotDto,
    timing: PreviewTimingDto,
) -> CommandResult<()> {
    app.emit(
        "preview_state_changed",
        PreviewStateEventDto {
            source_label: snapshot.preview.source_label.clone(),
            transport_state: snapshot.preview.transport_state,
            preview_updating: snapshot.preview.preview_updating,
            position_seconds: snapshot.preview.position_seconds,
            home_seconds: snapshot.preview.home_seconds,
            duration_seconds: snapshot.preview.duration_seconds,
            audio: snapshot.preview.audio.clone(),
            clock_source: snapshot.preview.clock_source.clone(),
            audio_playback_status: snapshot.preview.audio_playback_status,
            frame_topology_identity: snapshot.preview.frame_topology_identity.clone(),
            status: snapshot.preview.status.clone(),
            timing,
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
            transport_state: snapshot.transport_state,
            preview_updating: snapshot.preview_updating,
            position_seconds: snapshot.position_seconds,
            home_seconds: snapshot.home_seconds,
            duration_seconds: snapshot.duration_seconds,
            audio: snapshot.audio.clone().map(Into::into),
            clock_source: snapshot.clock_source.clone(),
            audio_playback_status: snapshot.audio_playback_status,
            frame_topology_identity: snapshot.frame.topology_identity.stable_key(),
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use dawn_app_core::app_model::AppModel;
    use dawn_app_core::preview_session::{PreviewSyncMode, PreviewTransportState, SequenceKey};
    use dawn_project::analysis::analyze_project;
    use dawn_project::document::get_sequence_document;
    use dawn_project::WorkspaceFs;
    use dawn_project::{utf8_path, Utf8PathBuf};

    use super::*;

    fn thirty_output_controller_project_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/thirty-output-controller/project.dawn")
    }

    fn thirty_output_controller_context() -> (WorkspaceFs, Utf8PathBuf, Utf8PathBuf) {
        let project_path = thirty_output_controller_project_path();
        let root = project_path
            .parent()
            .expect("thirty output controller project should have a parent");
        let fs = WorkspaceFs::open(root).expect("thirty output controller root should open");
        let project_path = utf8_path(
            project_path
                .strip_prefix(root)
                .expect("project path should be under root"),
        )
        .expect("project path should be valid UTF-8");
        let sequence_path = utf8_path(Path::new("sequences/empty.sequence.dawn"))
            .expect("sequence path should be valid UTF-8");
        (fs, project_path, sequence_path)
    }

    #[test]
    fn loading_to_play_audio_clock_does_not_reschedule_preview_render() {
        let (fs, project_path, sequence_path) = thirty_output_controller_context();
        let analysis = analyze_project(&fs, project_path.clone(), "thirty_output_controller");
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let document = get_sequence_document(&fs, sequence_path, "empty", project_path, Vec::new())
            .expect("thirty output controller sequence should load");
        let key = SequenceKey {
            path: document.path.clone().into(),
            object_key: document.object_key.clone(),
        };
        let mut model = AppModel::default();
        model.analysis = Some(std::sync::Arc::new(analysis.clone()));
        model.preview.sync_source(
            Some((key, document)),
            Some(&analysis),
            PreviewSyncMode::RenderNow,
        );
        let request = model
            .begin_deferred_preview_render()
            .expect("source sync should schedule preview render");

        let clock = AudioClock {
            position_seconds: 1.25,
            ended: false,
            status: AudioPlaybackStatus::LoadingToPlay,
            error: None,
        };
        apply_audio_clock_to_model(&mut model, &clock, Some(&analysis));
        apply_audio_clock_to_model(&mut model, &clock, Some(&analysis));

        assert!(!request.cancellation.is_cancelled());
        assert_eq!(
            model.preview.snapshot().transport_state,
            PreviewTransportState::LoadingToPlay
        );
        assert!(
            model.begin_deferred_preview_render().is_none(),
            "loading_to_play status updates should not create a new render request"
        );
    }
}
