use std::fmt;
use std::path::PathBuf;

use dawn_language::document::SequenceAudioDocument;
use dawn_language::path::Utf8PathBuf;

use crate::audio_runtime::{AudioClock, AudioRuntime};
use crate::editor::document_store::DocumentStoreCore;
use crate::editor::EditorViewMode;
use crate::gui_edits::selection::SequenceClipboard;
use crate::gui_edits::types::{
    FixtureGuiEdit, LayoutGuiEdit, SequenceGuiEdit, SequenceSelectionEdit,
    SequenceSelectionEditResult,
};
use crate::output::live_output::LiveOutputCore;
use crate::prefs::{AppPrefs, WindowLayout};
use crate::preview::session::{PreviewController, PreviewRenderTiming, PreviewSnapshot};
use crate::runtime::contracts::{RuntimeNotice, RuntimeStatus};
use crate::runtime::rendered_frame::RenderedFrame;
use crate::runtime::workers::{
    AsyncWorkers, RenderWorkerResult, SequenceEffectPreviewRequestEffect,
    SequenceEffectPreviewResult, WorkerResult,
};
use crate::workspace::WorkspaceSession;
use crate::AppView;

pub type BackendResult<T> = Result<T, BackendError>;

#[derive(Debug, Clone)]
pub struct BackendError {
    message: String,
}

impl BackendError {
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for BackendError {}

impl From<String> for BackendError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

impl From<&str> for BackendError {
    fn from(message: &str) -> Self {
        Self {
            message: message.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BackendOutput<T> {
    pub view: AppView,
    pub value: T,
}

#[derive(Debug, Clone, Copy)]
pub struct PreviewHostState {
    pub has_preview_sink: bool,
    pub backend_seconds: f32,
}

#[derive(Debug, Clone)]
pub struct PreviewTickOutput {
    pub view: AppView,
    pub snapshot: PreviewSnapshot,
    pub target_fps: u32,
    pub render_timing: PreviewRenderTiming,
    pub publish_frame: Option<RenderedFrame>,
    pub live_output_frame: Option<RenderedFrame>,
}

#[derive(Debug, Clone)]
pub struct SequenceEffectPreviewRequest {
    pub path: Utf8PathBuf,
    pub object_key: String,
    pub request_id: u32,
    pub effects: Vec<SequenceEffectPreviewRequestEffect>,
}

#[derive(Debug, Clone)]
pub struct SequenceEffectPreviewKey {
    pub path: String,
    pub object_key: String,
}

#[derive(Debug, Clone)]
pub struct SequenceAudioDialog {
    pub project_root: PathBuf,
    pub sequence_path: Utf8PathBuf,
    pub audio_directory: PathBuf,
}

pub struct AppBackend {
    pub(super) workspace: WorkspaceSession,
    pub(super) document_store: DocumentStoreCore,
    pub(super) preview: PreviewController,
    pub(super) audio: AudioRuntime,
    pub(super) status: RuntimeStatus,
    pub(super) sequence_clipboard: Option<SequenceClipboard>,
    pub(super) live_output: LiveOutputCore,
    pub(super) workers: AsyncWorkers,
    pub(super) app_prefs: AppPrefs,
}

impl Default for AppBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl AppBackend {
    pub fn new() -> Self {
        Self {
            workspace: WorkspaceSession::default(),
            document_store: DocumentStoreCore::default(),
            preview: PreviewController::default(),
            audio: AudioRuntime::default(),
            status: RuntimeStatus::no_project_open(),
            sequence_clipboard: None,
            live_output: LiveOutputCore::default(),
            workers: AsyncWorkers::new(),
            app_prefs: AppPrefs::default(),
        }
    }

    pub fn view(&self) -> AppView {
        self.snapshot(
            self.app_prefs.project_tree_visible(),
            self.app_prefs.effect_preview_enabled(),
            self.live_output.readout(),
        )
    }

    pub fn restore_last_project(&mut self) -> BackendResult<AppView> {
        self.restore_last_project_command()?;
        Ok(self.view())
    }

    pub fn open_project(&mut self, path: PathBuf) -> BackendResult<AppView> {
        self.open_project_from_path(path, true, RuntimeNotice::ProjectOpened)?;
        self.audio.clear();
        Ok(self.view())
    }

    pub fn create_new_project(&mut self, parent: PathBuf, name: String) -> BackendResult<AppView> {
        let project_root = crate::workspace::create_starter_project(&parent, &name)?;
        self.open_project_from_path(project_root, true, RuntimeNotice::ProjectOpened)?;
        self.open_file_command(crate::workspace::STARTER_SEQUENCE_PATH.into())?;
        self.set_active_view_mode_command(EditorViewMode::Gui)?;
        Ok(self.view())
    }

    pub fn open_file(&mut self, path: Utf8PathBuf) -> BackendResult<AppView> {
        self.open_file_command(path)?;
        Ok(self.view())
    }

    pub fn close_file(&mut self, path: Utf8PathBuf) -> BackendResult<AppView> {
        self.close_file_command(path)?;
        Ok(self.view())
    }

    pub fn set_active_file(&mut self, path: Utf8PathBuf) -> BackendResult<AppView> {
        self.set_active_file_command(path)?;
        Ok(self.view())
    }

    pub fn update_active_text(&mut self, text: String) -> BackendResult<AppView> {
        self.update_active_text_command(text)?;
        Ok(self.view())
    }

    pub fn set_active_view_mode(&mut self, mode: EditorViewMode) -> BackendResult<AppView> {
        self.set_active_view_mode_command(mode)?;
        Ok(self.view())
    }

    pub fn undo_active_edit(&mut self) -> BackendResult<AppView> {
        if self.undo_active_text()?.is_some() {
            self.status = RuntimeStatus::notice(RuntimeNotice::Undo);
        }
        Ok(self.view())
    }

    pub fn redo_active_edit(&mut self) -> BackendResult<AppView> {
        if self.redo_active_text()?.is_some() {
            self.status = RuntimeStatus::notice(RuntimeNotice::Redo);
        }
        Ok(self.view())
    }

    pub fn apply_sequence_gui_edit(&mut self, edit: SequenceGuiEdit) -> BackendResult<AppView> {
        self.apply_sequence_gui_edit_and_autosave(edit)?;
        Ok(self.view())
    }

    pub fn apply_layout_gui_edit(&mut self, edit: LayoutGuiEdit) -> BackendResult<AppView> {
        self.apply_layout_gui_edit_and_autosave(edit)?;
        Ok(self.view())
    }

    pub fn apply_fixture_gui_edit(&mut self, edit: FixtureGuiEdit) -> BackendResult<AppView> {
        self.apply_fixture_gui_edit_and_autosave(edit)?;
        Ok(self.view())
    }

    pub fn apply_sequence_selection_edit(
        &mut self,
        edit: SequenceSelectionEdit,
    ) -> BackendResult<BackendOutput<SequenceSelectionEditResult>> {
        let value = self.apply_sequence_selection_edit_command(edit)?;
        Ok(BackendOutput {
            view: self.view(),
            value,
        })
    }

    pub fn active_sequence_audio_dialog(&self) -> BackendResult<SequenceAudioDialog> {
        let (project_root, sequence_path) = self.active_sequence_audio_context()?;
        let project_root = project_root
            .map(PathBuf::from)
            .ok_or("no project is open")?;
        let sequence_path = if sequence_path.is_absolute() {
            sequence_path
        } else {
            Utf8PathBuf::from_path_buf(project_root.join(sequence_path.as_std_path()))
                .map_err(|path| format!("sequence path is not UTF-8: {}", path.display()))?
        };
        let audio_directory = project_root.join("audio");
        Ok(SequenceAudioDialog {
            project_root,
            sequence_path,
            audio_directory,
        })
    }

    pub fn set_active_sequence_audio(
        &mut self,
        selected_audio_path: PathBuf,
    ) -> BackendResult<AppView> {
        let dialog = self.active_sequence_audio_dialog()?;
        let selected = dawn_language::path::utf8_path(selected_audio_path)?;
        let import = dawn_language::path::serialized_import_path(&dialog.sequence_path, &selected);
        self.apply_sequence_gui_edit(SequenceGuiEdit::SetAudio {
            import: Some(import),
        })
    }

    pub fn clear_active_sequence_audio(&mut self) -> BackendResult<AppView> {
        self.apply_sequence_gui_edit(SequenceGuiEdit::SetAudio { import: None })
    }

    pub fn active_sequence_fseq_default_name(&self) -> BackendResult<String> {
        self.active_sequence_fseq_default_name_command()
            .map_err(Into::into)
    }

    pub fn export_active_sequence_fseq(
        &mut self,
        output_path: PathBuf,
        step_ms: u8,
    ) -> BackendResult<AppView> {
        self.export_active_sequence_fseq_command(&output_path, step_ms)?;
        Ok(self.view())
    }

    pub fn flush_autosave(&mut self) -> BackendResult<AppView> {
        self.flush_autosave_command()?;
        Ok(self.view())
    }

    pub fn reload_active_buffer_from_disk(&mut self) -> BackendResult<AppView> {
        self.reload_active_buffer_from_disk_command()?;
        Ok(self.view())
    }

    pub fn keep_active_buffer(&mut self) -> BackendResult<AppView> {
        self.keep_active_buffer_command()?;
        Ok(self.view())
    }

    pub fn create_file(&mut self, parent: Utf8PathBuf, name: String) -> BackendResult<AppView> {
        self.create_file_command(parent, name)?;
        Ok(self.view())
    }

    pub fn create_directory(
        &mut self,
        parent: Utf8PathBuf,
        name: String,
    ) -> BackendResult<AppView> {
        self.create_directory_command(parent, name)?;
        Ok(self.view())
    }

    pub fn rename_path(&mut self, path: Utf8PathBuf, new_name: String) -> BackendResult<AppView> {
        self.rename_path_command(path, new_name)?;
        Ok(self.view())
    }

    pub fn delete_path(&mut self, path: Utf8PathBuf) -> BackendResult<AppView> {
        self.delete_path_command(path)?;
        Ok(self.view())
    }

    pub fn reload_project(&mut self) -> BackendResult<AppView> {
        self.reload_project_command()?;
        Ok(self.view())
    }

    pub fn toggle_project_tree(&mut self) -> BackendResult<AppView> {
        self.toggle_project_tree_command()?;
        Ok(self.view())
    }

    pub fn set_effect_preview_enabled(&mut self, enabled: bool) -> BackendResult<AppView> {
        self.set_effect_preview_enabled_command(enabled)?;
        Ok(self.view())
    }

    pub fn set_effect_preview_effects(&mut self, ids: Vec<u32>) -> BackendResult<AppView> {
        self.set_effect_preview_effects_command(ids);
        Ok(self.view())
    }

    pub fn preview_play(&mut self) -> BackendResult<AppView> {
        let snapshot = self.preview_snapshot();
        let audio = valid_preview_audio(&snapshot);
        if self.app_prefs.effect_preview_enabled() {
            self.app_prefs.set_effect_preview_enabled(false)?;
            self.sync_effect_preview_enabled(false)?;
        }
        if let Some(audio) = audio {
            let clock = self.audio.play(&audio, snapshot.position_seconds)?;
            self.apply_audio_clock(clock);
        } else {
            self.start_preview_playback();
        }
        Ok(self.view())
    }

    pub fn preview_pause(&mut self) -> BackendResult<AppView> {
        if valid_preview_audio(&self.preview_snapshot()).is_some() {
            let clock = self.audio.pause()?;
            self.preview_pause_at_native_audio(clock.position_seconds, clock.status);
        } else {
            self.preview_pause_command();
        }
        Ok(self.view())
    }

    pub fn preview_stop(&mut self) -> BackendResult<AppView> {
        let snapshot = self.preview_snapshot();
        if valid_preview_audio(&snapshot).is_some() {
            let clock = self.audio.stop(snapshot.home_seconds)?;
            self.preview_stop_native_audio(clock.status);
        } else {
            self.preview_stop_command();
        }
        Ok(self.view())
    }

    pub fn preview_rewind_to_zero(&mut self) -> BackendResult<AppView> {
        if valid_preview_audio(&self.preview_snapshot()).is_some() {
            let clock = self.audio.stop(0.0)?;
            self.preview_rewind_native_audio(clock.status);
        } else {
            self.preview_rewind_to_zero_command();
        }
        Ok(self.view())
    }

    pub fn preview_seek(&mut self, position_seconds: f64) -> BackendResult<AppView> {
        validate_position_seconds(position_seconds)?;
        let snapshot = self.preview_snapshot();
        if let Some(audio) = valid_preview_audio(&snapshot) {
            let clock = self
                .audio
                .seek(&audio, position_seconds, snapshot.is_playing)?;
            self.preview_seek_native_audio(
                clock.position_seconds,
                snapshot.is_playing,
                clock.status,
            );
        } else {
            self.preview_seek_command(position_seconds);
        }
        Ok(self.view())
    }

    pub fn preview_tick(&mut self, host: PreviewHostState) -> BackendResult<PreviewTickOutput> {
        let worker_changed = self.poll_workers();
        let worker_render_timing = if worker_changed {
            Some(self.preview_last_render_timing())
        } else {
            None
        };
        let initial_snapshot = self.preview_snapshot();
        if initial_snapshot.audio.is_some() {
            let clock = self.audio.clock()?;
            if should_apply_audio_clock(&initial_snapshot, &clock) {
                self.apply_audio_clock(clock);
            }
        } else {
            self.tick_preview_clock();
        }
        let mut snapshot = self.preview_snapshot();
        let live_output_enabled = self.live_output_enabled();
        let should_render_frame = (host.has_preview_sink || live_output_enabled)
            && (snapshot.is_playing
                || snapshot.preview_updating
                || snapshot.effect_preview_active
                || live_output_enabled);
        let deferred_requested = if snapshot.preview_updating {
            self.request_deferred_preview_render().unwrap_or(false)
        } else {
            false
        };
        if should_render_frame && !deferred_requested {
            self.render_preview_frame();
            snapshot = self.preview_snapshot();
        }
        let live_output_frame = if live_output_enabled {
            self.send_live_output_frame(&snapshot.frame);
            Some(snapshot.frame.clone())
        } else {
            None
        };
        let publish_frame = if host.has_preview_sink {
            Some(snapshot.frame.clone())
        } else {
            None
        };
        let render_timing =
            worker_render_timing.unwrap_or_else(|| self.preview_last_render_timing());
        Ok(PreviewTickOutput {
            view: self.view(),
            snapshot,
            target_fps: self.preview_target_fps(),
            render_timing,
            publish_frame,
            live_output_frame,
        })
    }

    pub fn request_sequence_effect_previews(
        &mut self,
        request: SequenceEffectPreviewRequest,
    ) -> BackendResult<()> {
        self.request_sequence_effect_previews_command(
            request.path,
            request.object_key,
            request.request_id,
            request.effects,
        )?;
        Ok(())
    }

    pub fn take_sequence_effect_preview_results(
        &mut self,
        key: SequenceEffectPreviewKey,
    ) -> BackendResult<Vec<SequenceEffectPreviewResult>> {
        self.take_sequence_effect_preview_results_command(key.path, key.object_key)
            .map_err(Into::into)
    }

    pub fn set_live_output_enabled(&mut self, enabled: bool) -> BackendResult<AppView> {
        self.set_live_output_enabled_command(enabled);
        Ok(self.view())
    }

    pub fn main_window_layout(&self) -> WindowLayout {
        self.main_window_layout_command()
    }

    pub fn preview_window_layout(&self) -> WindowLayout {
        self.preview_window_layout_command()
    }

    pub fn preview_window_should_open(&self) -> bool {
        self.preview_window_should_open_command()
    }

    pub fn set_main_window_layout(&mut self, layout: WindowLayout) -> BackendResult<AppView> {
        self.set_main_window_layout_command(layout)?;
        Ok(self.view())
    }

    pub fn set_preview_window_layout(&mut self, layout: WindowLayout) -> BackendResult<AppView> {
        self.set_preview_window_layout_command(layout)?;
        Ok(self.view())
    }

    pub fn set_preview_window_open(&mut self, open: bool) -> BackendResult<AppView> {
        self.set_preview_window_open_command(open)?;
        Ok(self.view())
    }

    pub(super) fn poll_workers(&mut self) -> bool {
        let mut changed = false;
        for result in self.workers.drain() {
            changed |= self.apply_worker_result(result);
        }
        changed
    }

    fn apply_audio_clock(&mut self, clock: AudioClock) {
        self.apply_audio_clock_state(
            clock.position_seconds,
            clock.status,
            clock.ended,
            clock.error.as_deref(),
        );
    }

    fn apply_worker_result(&mut self, result: WorkerResult) -> bool {
        match result {
            WorkerResult::Filesystem { paths } => match self.handle_filesystem_changes(paths) {
                Ok(()) => true,
                Err(error) => {
                    self.status = RuntimeStatus::error(error);
                    true
                }
            },
            WorkerResult::Render(RenderWorkerResult::PreviewFrame(result)) => {
                self.preview.complete_deferred_render(result)
            }
            WorkerResult::Analysis | WorkerResult::LiveOutput => false,
        }
    }
}

fn valid_preview_audio(snapshot: &PreviewSnapshot) -> Option<SequenceAudioDocument> {
    snapshot
        .audio
        .as_ref()
        .filter(|audio| audio.exists)
        .cloned()
}

fn should_apply_audio_clock(snapshot: &PreviewSnapshot, clock: &AudioClock) -> bool {
    snapshot.is_playing
        || matches!(
            clock.status,
            crate::AudioPlaybackStatus::LoadingToPlay
                | crate::AudioPlaybackStatus::Playing
                | crate::AudioPlaybackStatus::Ended
        )
        || clock.ended
        || snapshot.audio_playback_status == crate::AudioPlaybackStatus::LoadingToPlay
}

fn validate_position_seconds(position_seconds: f64) -> BackendResult<()> {
    if position_seconds.is_finite() && position_seconds >= 0.0 {
        Ok(())
    } else {
        Err("preview seek seconds must be finite and non-negative".into())
    }
}
