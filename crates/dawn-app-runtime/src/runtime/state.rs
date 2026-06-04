use dawn_language::document::{
    DocumentDescriptor, DocumentViewId, SequenceDocument, SequenceDocumentEdit,
    SequenceEffectMoveDocumentEdit, SequenceEffectResizeDocumentEdit, SequenceMarkMoveDocumentEdit,
    SequenceMarkPasteDocumentEdit, SequenceMarkRefDocumentEdit,
};
use dawn_language::model::{Authored, DawnObject, Geometry};
use dawn_language::parse::parse_dawn_file_with_source_map;
use dawn_language::path::Utf8PathBuf;

use crate::dto::{
    FixtureGuiEditDto, LayoutGuiEditDto, SequenceGuiEditDto, SequenceMarkRefDto,
    SequencePasteAnchorDto, SequenceResizeEdgeDto, SequenceSelectionDto, SequenceSelectionEditDto,
    SequenceSelectionEditResultDto,
};
use crate::editor_session::{EditorSession, SessionBufferState};
use crate::preview::session::{
    AudioPlaybackStatus, PreviewController, PreviewRenderRequest, PreviewRenderResult,
    PreviewSnapshot, PreviewSyncMode, SequenceKey,
};
use crate::read_model::ActiveGuiDocument;
use crate::runtime::contracts::RuntimeStatus;
use crate::runtime::coordinator::SequenceClipboard;
use crate::services::editor_state::{BufferTab, EditorViewMode};
use crate::services::live_output::LiveOutputReadout;
use crate::workspace_session::{CreatedRuntimeFile, WorkspaceSession};

const MIN_EFFECT_DURATION_SECONDS: f64 = 0.000000001;

fn sequence_delete_edit(selection: SequenceSelectionDto) -> SequenceDocumentEdit {
    match selection {
        SequenceSelectionDto::Effects { ids } => SequenceDocumentEdit::DeleteEffects { ids },
        SequenceSelectionDto::Marks { marks } => SequenceDocumentEdit::DeleteMarks {
            marks: marks
                .into_iter()
                .map(|mark| SequenceMarkRefDocumentEdit {
                    collection_key: mark.collection_key,
                    index: mark.index as usize,
                })
                .collect(),
        },
    }
}

fn selection_empty_like(selection: &SequenceSelectionDto) -> SequenceSelectionDto {
    match selection {
        SequenceSelectionDto::Effects { .. } => SequenceSelectionDto::Effects { ids: Vec::new() },
        SequenceSelectionDto::Marks { .. } => SequenceSelectionDto::Marks { marks: Vec::new() },
    }
}

fn selection_count(selection: &SequenceSelectionDto) -> u32 {
    match selection {
        SequenceSelectionDto::Effects { ids } => ids.len().min(u32::MAX as usize) as u32,
        SequenceSelectionDto::Marks { marks } => marks.len().min(u32::MAX as usize) as u32,
    }
}

fn effect_move_edits(
    document: &SequenceDocument,
    ids: Vec<u32>,
    time_delta_seconds: f64,
    lane_delta: i32,
) -> Vec<SequenceEffectMoveDocumentEdit> {
    ids.into_iter()
        .filter_map(|id| {
            let effect = document
                .effects
                .iter()
                .find(|candidate| candidate.id == id)?;
            let current_lane = document
                .lanes
                .iter()
                .position(|lane| lane.target == effect.target)
                .unwrap_or(0);
            let lane_index = (current_lane as i32 + lane_delta)
                .clamp(0, document.lanes.len().saturating_sub(1) as i32)
                as usize;
            Some(SequenceEffectMoveDocumentEdit {
                id,
                start_seconds: (effect.start_seconds + time_delta_seconds).clamp(
                    0.0,
                    (document.duration_seconds - effect.duration_seconds).max(0.0),
                ),
                target: document
                    .lanes
                    .get(lane_index)
                    .map(|lane| lane.target.clone()),
            })
        })
        .collect()
}

fn effect_resize_edits(
    document: &SequenceDocument,
    ids: Vec<u32>,
    edge: SequenceResizeEdgeDto,
    time_delta_seconds: f64,
) -> Vec<SequenceEffectResizeDocumentEdit> {
    ids.into_iter()
        .filter_map(|id| {
            let effect = document
                .effects
                .iter()
                .find(|candidate| candidate.id == id)?;
            let (start_seconds, duration_seconds) = match edge {
                SequenceResizeEdgeDto::Left => {
                    let end_seconds = effect.start_seconds + effect.duration_seconds;
                    let start_seconds = (effect.start_seconds + time_delta_seconds)
                        .clamp(0.0, end_seconds - MIN_EFFECT_DURATION_SECONDS);
                    (start_seconds, end_seconds - start_seconds)
                }
                SequenceResizeEdgeDto::Right => {
                    let duration_seconds = (effect.duration_seconds + time_delta_seconds).clamp(
                        MIN_EFFECT_DURATION_SECONDS,
                        document.duration_seconds - effect.start_seconds,
                    );
                    (effect.start_seconds, duration_seconds)
                }
            };
            Some(SequenceEffectResizeDocumentEdit {
                id,
                start_seconds,
                duration_seconds,
            })
        })
        .collect()
}

fn mark_move_edits(
    document: &SequenceDocument,
    marks: Vec<SequenceMarkRefDto>,
    time_delta_seconds: f64,
) -> Vec<SequenceMarkMoveDocumentEdit> {
    marks
        .into_iter()
        .filter_map(|mark| {
            let collection = document
                .mark_collections
                .iter()
                .find(|collection| collection.key == mark.collection_key)?;
            let time_seconds = collection.marks_seconds.get(mark.index as usize)?;
            Some(SequenceMarkMoveDocumentEdit {
                collection_key: mark.collection_key,
                index: mark.index as usize,
                time_seconds: (time_seconds + time_delta_seconds)
                    .clamp(0.0, document.duration_seconds),
            })
        })
        .collect()
}

#[derive(Debug)]
pub struct CoordinatorState {
    workspace: WorkspaceSession,
    editor: EditorSession,
    preview: PreviewController,
    status: RuntimeStatus,
}

#[derive(Debug, Clone)]
pub struct CoordinatorSnapshot {
    pub project_root: Option<String>,
    pub project_entries: Vec<dawn_language::fs::WorkspaceEntry>,
    pub analysis: Option<dawn_language::analysis::ProjectAnalysis>,
    pub diagnostics: Vec<dawn_language::analysis::ProjectDiagnostic>,
    pub project_tree_visible: bool,
    pub effect_preview_enabled: bool,
    pub preview: PreviewSnapshot,
    pub live_output: LiveOutputReadout,
    pub tabs: Vec<BufferTab>,
    pub active_file: Option<Utf8PathBuf>,
    pub active_buffer: Option<BufferTab>,
    pub active_document_descriptor: Option<DocumentDescriptor>,
    pub active_gui_document: Option<ActiveGuiDocument>,
    pub status: RuntimeStatus,
}

impl Default for CoordinatorState {
    fn default() -> Self {
        Self {
            workspace: WorkspaceSession::default(),
            editor: EditorSession::default(),
            preview: PreviewController::default(),
            status: RuntimeStatus::NoProjectOpen,
        }
    }
}

impl CoordinatorState {
    pub fn snapshot(
        &self,
        project_tree_visible: bool,
        effect_preview_enabled: bool,
        live_output: LiveOutputReadout,
    ) -> CoordinatorSnapshot {
        let active_document_descriptor = self.active_document_descriptor();
        let active_gui_document = self.workspace.active_gui_document(
            self.editor.active_buffer(),
            active_document_descriptor.as_ref(),
            self.editor.dirty_overlays(),
        );
        CoordinatorSnapshot {
            project_root: self.workspace.project_root(),
            project_entries: self.workspace.project_entries(),
            analysis: self.workspace.analysis_cloned(),
            diagnostics: self.workspace.diagnostics_cloned(),
            project_tree_visible,
            effect_preview_enabled,
            preview: self.preview.snapshot(),
            live_output,
            tabs: self.editor.tabs(),
            active_file: self.editor.active_file().cloned(),
            active_buffer: self.editor.active_buffer().cloned(),
            active_document_descriptor,
            active_gui_document,
            status: self.status.clone(),
        }
    }

    pub fn prepare_for_runtime_project_open(&mut self) -> Result<(), String> {
        self.flush_autosave()
    }

    pub fn sync_project_opened(
        &mut self,
        path: std::path::PathBuf,
        _remember: bool,
        status: impl Into<String>,
    ) -> Result<(), String> {
        self.open_project(path)?;
        self.status = RuntimeStatus::message(status);
        Ok(())
    }

    pub fn sync_session_opened(
        &mut self,
        path: std::path::PathBuf,
        buffers: Vec<SessionBufferState>,
        active_file: Option<Utf8PathBuf>,
        status: impl Into<String>,
    ) -> Result<(), String> {
        self.workspace.open_project(&path)?;
        self.editor.restore(buffers, active_file);
        self.preview.reset();
        self.workspace.refresh_analysis_from_editor(&self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        self.status = RuntimeStatus::message(status);
        Ok(())
    }

    pub fn sync_file_opened(
        &mut self,
        path: Utf8PathBuf,
        text: String,
        disk_version: crate::services::editor_state::FileVersion,
        view_mode: EditorViewMode,
    ) -> Result<(), String> {
        self.editor.open_file(path, text, disk_version, view_mode);
        self.workspace.refresh_analysis_from_editor(&self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub fn sync_file_closed(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        self.editor.close_file(&path);
        self.workspace.refresh_analysis_from_editor(&self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub fn sync_active_file(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        let active_changed = self.editor.active_file() != Some(&path);
        self.editor.set_active_file(path);
        if active_changed {
            self.preview.pause(self.workspace.analysis());
            self.sync_preview_source(PreviewSyncMode::RenderNow);
        }
        Ok(())
    }

    pub fn sync_active_view_mode(&mut self, mode: EditorViewMode) -> Result<(), String> {
        self.editor.set_active_view_mode(mode);
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub fn sync_active_text_update(&mut self, text: String) -> Result<(), String> {
        self.ensure_active_buffer_not_conflicted()?;
        self.editor.update_active_text(text);
        self.refresh_analysis_after_memory_edit();
        self.status = RuntimeStatus::message("Edited");
        Ok(())
    }

    pub fn sync_active_history_text(&mut self, text: String, status: impl Into<String>) {
        self.editor.replace_active_text_from_runtime(text);
        self.refresh_analysis_after_memory_edit();
        self.status = RuntimeStatus::message(status);
    }

    pub fn create_file_for_runtime_open(
        &mut self,
        parent: Utf8PathBuf,
        name: String,
    ) -> Result<CreatedRuntimeFile, String> {
        self.flush_autosave()?;
        self.workspace.create_file_for_runtime_open(parent, &name)
    }

    pub fn reload_project(&mut self) -> Result<(), String> {
        let paths = self
            .editor
            .buffers()
            .into_iter()
            .map(|buffer| buffer.path)
            .collect();
        self.reconcile_filesystem_changes(paths)?;
        self.status = RuntimeStatus::message("Project checked");
        Ok(())
    }

    pub fn apply_sequence_gui_edit_and_autosave(
        &mut self,
        edit: SequenceGuiEditDto,
    ) -> Result<(), String> {
        self.apply_sequence_gui_edit(edit)?;
        self.flush_autosave_without_analysis()?;
        self.status = RuntimeStatus::message("Autosaved");
        Ok(())
    }

    pub fn apply_layout_gui_edit_and_autosave(
        &mut self,
        edit: LayoutGuiEditDto,
    ) -> Result<(), String> {
        self.apply_layout_gui_edit(edit)?;
        self.flush_autosave()?;
        self.status = RuntimeStatus::message("Autosaved");
        Ok(())
    }

    pub fn apply_fixture_gui_edit_and_autosave(
        &mut self,
        edit: FixtureGuiEditDto,
    ) -> Result<(), String> {
        self.apply_fixture_gui_edit(edit)?;
        self.flush_autosave()?;
        self.status = RuntimeStatus::message("Autosaved");
        Ok(())
    }

    pub fn flush_autosave_command(&mut self) -> Result<(), String> {
        self.flush_autosave()?;
        self.status = RuntimeStatus::Saved;
        Ok(())
    }

    pub fn handle_filesystem_changes(&mut self, paths: Vec<Utf8PathBuf>) -> Result<(), String> {
        self.reconcile_filesystem_changes(paths)?;
        self.status = RuntimeStatus::message("Filesystem refreshed");
        Ok(())
    }

    pub fn reload_active_buffer_from_disk_command(&mut self) -> Result<(), String> {
        self.reload_active_buffer_from_disk()?;
        self.status = RuntimeStatus::message("Reloaded from disk");
        Ok(())
    }

    pub fn keep_active_buffer_command(&mut self) -> Result<(), String> {
        self.keep_active_buffer()?;
        self.status = RuntimeStatus::message("Kept IDE changes");
        Ok(())
    }

    pub fn create_directory(&mut self, parent: Utf8PathBuf, name: String) -> Result<(), String> {
        self.flush_autosave()?;
        self.workspace.create_directory(parent, &name)?;
        self.workspace.refresh_analysis_from_editor(&self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub fn rename_path(&mut self, path: Utf8PathBuf, new_name: String) -> Result<(), String> {
        self.flush_autosave()?;
        let moves = self.workspace.rename_path(path.clone(), &new_name)?;
        self.editor.reconcile_moved_paths(&moves);
        self.workspace.refresh_analysis_from_editor(&self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub fn delete_path(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        self.flush_autosave()?;
        self.workspace.delete_path(path.clone())?;
        self.editor.reconcile_deleted_path(&path);
        self.workspace.refresh_analysis_from_editor(&self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    pub fn set_effect_preview_enabled(&mut self, enabled: bool) -> Result<(), String> {
        if !enabled {
            self.preview.clear_effect_preview(self.workspace.analysis());
        } else {
            self.preview.render_current_frame(self.workspace.analysis());
        }
        Ok(())
    }

    pub fn set_effect_preview_effects(&mut self, ids: Vec<u32>, effect_preview_enabled: bool) {
        let ids = if effect_preview_enabled {
            ids
        } else {
            Vec::new()
        };
        self.preview
            .set_effect_preview_ids(ids, self.workspace.analysis());
    }

    pub fn preview_play(&mut self) {
        self.preview.play(self.workspace.analysis());
        self.status = RuntimeStatus::message("Preview playing");
    }

    pub fn preview_pause(&mut self) {
        self.preview.pause(self.workspace.analysis());
        self.status = RuntimeStatus::message("Preview paused");
    }

    pub fn preview_stop(&mut self) {
        self.preview.stop(self.workspace.analysis());
        self.status = RuntimeStatus::message("Preview stopped");
    }

    pub fn preview_rewind_to_zero(&mut self) {
        self.preview
            .go_to_sequence_beginning(self.workspace.analysis());
        self.status = RuntimeStatus::message("Preview rewound");
    }

    pub fn preview_seek(&mut self, position_seconds: f64) {
        self.preview
            .seek(position_seconds, self.workspace.analysis());
        self.status = RuntimeStatus::message("Preview seeked");
    }

    pub fn tick_preview(&mut self) {
        self.preview.tick(self.workspace.analysis());
    }

    pub fn tick_preview_clock(&mut self) {
        self.preview.tick_clock();
    }

    pub fn render_preview_frame(&mut self) {
        self.preview.render_current_frame(self.workspace.analysis());
    }

    pub fn begin_deferred_preview_render(&mut self) -> Option<PreviewRenderRequest> {
        self.preview.begin_deferred_render()
    }

    pub fn complete_deferred_preview_render(&mut self, result: PreviewRenderResult) -> bool {
        self.preview.complete_deferred_render(result)
    }

    pub fn preview_target_fps(&self) -> u32 {
        self.preview.target_fps()
    }

    pub fn project_root(&self) -> Option<String> {
        self.workspace.project_root()
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = RuntimeStatus::message(status);
    }

    pub fn read_file_with_version(
        &self,
        path: Utf8PathBuf,
    ) -> Result<(String, crate::services::editor_state::FileVersion), String> {
        self.workspace.read_file_with_version(path)
    }

    pub fn preview_snapshot(&self) -> PreviewSnapshot {
        self.preview.snapshot()
    }

    pub fn preview_last_render_timing(&self) -> crate::preview::session::PreviewRenderTiming {
        self.preview.last_render_timing()
    }

    pub fn current_analysis(&self) -> Option<dawn_language::analysis::ProjectAnalysis> {
        self.workspace.analysis_cloned()
    }

    pub fn preview_pause_at_native_audio(
        &mut self,
        position_seconds: f64,
        status: AudioPlaybackStatus,
    ) {
        self.preview
            .pause_at(position_seconds, self.workspace.analysis());
        self.preview.set_timing_status("nativeAudio", status);
        self.status = RuntimeStatus::message("Preview paused");
    }

    pub fn preview_stop_native_audio(&mut self, status: AudioPlaybackStatus) {
        self.preview.stop_native_audio(self.workspace.analysis());
        self.preview.set_timing_status("nativeAudio", status);
        self.status = RuntimeStatus::message("Preview stopped");
    }

    pub fn preview_rewind_native_audio(&mut self, status: AudioPlaybackStatus) {
        self.preview
            .go_to_sequence_beginning_native_audio(self.workspace.analysis());
        self.preview.set_timing_status("nativeAudio", status);
        self.status = RuntimeStatus::message("Preview rewound");
    }

    pub fn preview_seek_native_audio(
        &mut self,
        position_seconds: f64,
        playing: bool,
        status: AudioPlaybackStatus,
    ) {
        self.preview
            .seek_native_audio(position_seconds, playing, self.workspace.analysis());
        self.preview.set_timing_status("nativeAudio", status);
        self.status = RuntimeStatus::message("Preview seeked");
    }

    pub fn apply_audio_clock_state(
        &mut self,
        position_seconds: f64,
        status: AudioPlaybackStatus,
        ended: bool,
        error: Option<&str>,
    ) {
        if let Some(error) = error {
            self.preview
                .pause_at(position_seconds, self.workspace.analysis());
            self.preview
                .set_timing_status("nativeAudio", AudioPlaybackStatus::Error);
            self.status = RuntimeStatus::message(format!("Audio error: {error}"));
            return;
        }
        if ended {
            self.preview.render_at_native_audio_clock(
                position_seconds,
                true,
                self.workspace.analysis(),
            );
            self.preview
                .set_timing_status("nativeAudio", AudioPlaybackStatus::Ended);
            self.status = RuntimeStatus::message("Preview complete");
            return;
        }
        match status {
            AudioPlaybackStatus::Loading => {
                self.preview
                    .pause_at(position_seconds, self.workspace.analysis());
                self.preview
                    .set_timing_status("nativeAudio", AudioPlaybackStatus::Loading);
                self.status = RuntimeStatus::message("Loading audio");
            }
            AudioPlaybackStatus::LoadingToPlay => {
                self.preview
                    .pause_at(position_seconds, self.workspace.analysis());
                self.preview
                    .set_timing_status("nativeAudio", AudioPlaybackStatus::LoadingToPlay);
                self.status = RuntimeStatus::message("Loading audio - will play");
            }
            AudioPlaybackStatus::Playing => {
                self.preview
                    .play_from_native_audio_clock(position_seconds, self.workspace.analysis());
                self.preview
                    .set_timing_status("nativeAudio", AudioPlaybackStatus::Playing);
                self.status = RuntimeStatus::message("Preview playing");
            }
            AudioPlaybackStatus::Ended => {
                self.preview.render_at_native_audio_clock(
                    position_seconds,
                    true,
                    self.workspace.analysis(),
                );
                self.preview
                    .set_timing_status("nativeAudio", AudioPlaybackStatus::Ended);
                self.status = RuntimeStatus::message("Preview complete");
            }
            AudioPlaybackStatus::Missing => {
                self.preview
                    .pause_at(position_seconds, self.workspace.analysis());
                self.preview
                    .set_timing_status("silent", AudioPlaybackStatus::Missing);
                self.status = RuntimeStatus::message("Audio missing");
            }
            AudioPlaybackStatus::None => {
                self.preview
                    .pause_at(position_seconds, self.workspace.analysis());
                self.preview
                    .set_timing_status("silent", AudioPlaybackStatus::None);
                self.status = RuntimeStatus::message("Preview ready");
            }
            AudioPlaybackStatus::Ready | AudioPlaybackStatus::Error => {
                self.preview
                    .pause_at(position_seconds, self.workspace.analysis());
                self.preview.set_timing_status("nativeAudio", status);
                self.status = RuntimeStatus::message("Preview ready");
            }
        }
    }

    pub fn active_sequence_export_source(
        &self,
    ) -> Result<
        (
            dawn_language::analysis::ProjectAnalysis,
            SequenceDocument,
            String,
        ),
        String,
    > {
        let analysis = self
            .workspace
            .analysis()
            .ok_or_else(|| "project analysis is not available".to_string())?
            .clone();
        if analysis.has_errors() {
            return Err("project has analysis errors".to_string());
        }
        if self
            .active_gui_document()
            .as_ref()
            .is_some_and(|document| document.is_blocked())
        {
            return Err("active document is blocked by diagnostics".to_string());
        }
        let path = self
            .editor
            .active_file()
            .cloned()
            .ok_or_else(|| "no active sequence file is selected".to_string())?;
        let overlays = self.editor.dirty_overlays();
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), overlays.clone())?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
            .cloned()
            .ok_or_else(|| "active document is not a sequence".to_string())?;
        let document = self
            .workspace
            .sequence_document(path, &object_key, overlays)?;
        let default_name = format!("{}.fseq", document.object_key);
        Ok((analysis, document, default_name))
    }

    pub fn active_sequence_audio_context(&self) -> Result<(Option<String>, Utf8PathBuf), String> {
        let Some(sequence_path) = self.editor.active_file().cloned() else {
            return Err("no active sequence file is selected".to_string());
        };
        if !self
            .active_gui_document()
            .as_ref()
            .is_some_and(|document| document.is_sequence())
        {
            return Err("active document is not a sequence".to_string());
        }
        Ok((self.workspace.project_root(), sequence_path))
    }

    fn active_gui_document(&self) -> Option<ActiveGuiDocument> {
        let descriptor = self.active_document_descriptor();
        self.workspace.active_gui_document(
            self.editor.active_buffer(),
            descriptor.as_ref(),
            self.editor.dirty_overlays(),
        )
    }

    pub fn effect_preview_request_source(
        &self,
        path: Utf8PathBuf,
        object_key: &str,
    ) -> Result<(dawn_language::analysis::ProjectAnalysis, SequenceDocument), String> {
        let analysis = self
            .workspace
            .analysis()
            .ok_or_else(|| "project analysis is not available".to_string())?
            .clone();
        let document = self.active_sequence_document_for_preview_request(&path, object_key)?;
        Ok((analysis, document))
    }

    fn open_project(&mut self, path: std::path::PathBuf) -> Result<(), String> {
        self.workspace.open_project(&path)?;
        self.editor.clear();
        self.preview.reset();
        self.workspace.refresh_analysis_from_editor(&self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    fn refresh_analysis_after_memory_edit(&mut self) {
        match self.workspace.refresh_analysis_from_editor(&self.editor) {
            Ok(()) => {
                self.sync_preview_source(PreviewSyncMode::RenderNow);
            }
            Err(error) => {
                self.status = RuntimeStatus::message(error);
                self.sync_preview_source(PreviewSyncMode::RenderNow);
            }
        }
    }

    pub fn flush_autosave(&mut self) -> Result<(), String> {
        self.flush_autosave_with_preview_sync(true)
    }

    fn flush_autosave_without_analysis(&mut self) -> Result<(), String> {
        self.workspace
            .flush_autosave_without_analysis(&mut self.editor)
    }

    fn flush_autosave_with_preview_sync(&mut self, sync_preview: bool) -> Result<(), String> {
        let had_dirty_buffers = self.workspace.flush_autosave(&mut self.editor)?;
        if had_dirty_buffers && sync_preview {
            self.sync_preview_source(PreviewSyncMode::RenderNow);
        }
        Ok(())
    }

    fn reconcile_filesystem_changes(&mut self, paths: Vec<Utf8PathBuf>) -> Result<(), String> {
        self.workspace
            .reconcile_filesystem_changes(&mut self.editor, paths)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    fn reload_active_buffer_from_disk(&mut self) -> Result<(), String> {
        self.workspace
            .reload_active_buffer_from_disk(&mut self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    fn keep_active_buffer(&mut self) -> Result<(), String> {
        self.workspace.keep_active_buffer(&mut self.editor)?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    fn sync_preview_source(&mut self, mode: PreviewSyncMode) {
        let source = self.active_sequence_source();
        self.preview
            .sync_source(source, self.workspace.analysis(), mode);
    }

    fn sync_preview_source_from_document(
        &mut self,
        path: Utf8PathBuf,
        document: SequenceDocument,
        mode: PreviewSyncMode,
    ) {
        let source = Some((
            SequenceKey {
                path,
                object_key: document.object_key.clone(),
            },
            document,
        ));
        self.preview
            .sync_source(source, self.workspace.analysis(), mode);
    }

    fn active_document_descriptor(&self) -> Option<DocumentDescriptor> {
        let path = self.editor.active_file()?.clone();
        self.workspace
            .inspect_document(path, self.editor.dirty_overlays())
            .ok()
    }

    pub fn active_sequence_document_for_preview_request(
        &self,
        path: &Utf8PathBuf,
        object_key: &str,
    ) -> Result<SequenceDocument, String> {
        let Some(buffer) = self.editor.active_buffer() else {
            return Err("sequence preview request does not match active sequence".to_string());
        };
        if buffer.view_mode != EditorViewMode::Gui || buffer.is_conflicted() {
            return Err("sequence preview request does not match active sequence".to_string());
        }
        let document = self.workspace.sequence_document(
            buffer.path.clone(),
            object_key,
            self.editor.dirty_overlays(),
        )?;
        if buffer.path != *path && document.path != path.as_str() {
            return Err("sequence preview request does not match active sequence".to_string());
        }
        Ok(document)
    }

    fn apply_sequence_gui_edit(&mut self, edit: SequenceGuiEditDto) -> Result<(), String> {
        self.ensure_active_buffer_not_conflicted()?;
        let path = self.active_path_for_gui_edit()?;
        let descriptor_overlays = self.editor.dirty_overlays();
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), descriptor_overlays)?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
            .ok_or_else(|| "active document is not a sequence".to_string())?
            .clone();
        let edit = match edit {
            SequenceGuiEditDto::SetAudio { import } => SequenceDocumentEdit::SetAudio { import },
            SequenceGuiEditDto::AddEffect {
                script,
                target,
                scope,
                start_seconds,
                mark_collection_key,
            } => SequenceDocumentEdit::AddEffect {
                script: script.into(),
                target: target.into(),
                scope: scope.into(),
                start_seconds,
                mark_collection_key,
            },
            SequenceGuiEditDto::MoveEffect {
                id,
                start_seconds,
                target,
            } => SequenceDocumentEdit::MoveEffect {
                id,
                start_seconds,
                target: target.map(Into::into),
            },
            SequenceGuiEditDto::ResizeEffect {
                id,
                start_seconds,
                duration_seconds,
            } => SequenceDocumentEdit::ResizeEffect {
                id,
                start_seconds,
                duration_seconds,
            },
            SequenceGuiEditDto::ChangeEffectScript { id, script } => {
                SequenceDocumentEdit::ChangeEffectScript {
                    id,
                    script: script.into(),
                }
            }
            SequenceGuiEditDto::DeleteEffect { id } => SequenceDocumentEdit::DeleteEffect { id },
            SequenceGuiEditDto::RetargetEffect { id, target } => {
                SequenceDocumentEdit::RetargetEffect {
                    id,
                    target: target.into(),
                }
            }
            SequenceGuiEditDto::SetEffectScope { id, scope } => {
                SequenceDocumentEdit::SetEffectScope {
                    id,
                    scope: scope.into(),
                }
            }
            SequenceGuiEditDto::UpdateEffectParam { id, name, value } => {
                SequenceDocumentEdit::UpdateEffectParam {
                    id,
                    name,
                    value: value.into(),
                }
            }
            SequenceGuiEditDto::LinkEffectCurveParam {
                id,
                name,
                curve_path,
                object_key,
            } => SequenceDocumentEdit::LinkEffectCurveParam {
                id,
                name,
                curve_path,
                object_key,
            },
            SequenceGuiEditDto::UnlinkEffectCurveParam { id, name } => {
                SequenceDocumentEdit::UnlinkEffectCurveParam { id, name }
            }
            SequenceGuiEditDto::CreateMarkCollection { key, name, color } => {
                SequenceDocumentEdit::CreateMarkCollection { key, name, color }
            }
            SequenceGuiEditDto::RenameMarkCollection { key, name } => {
                SequenceDocumentEdit::RenameMarkCollection { key, name }
            }
            SequenceGuiEditDto::DeleteMarkCollection { key } => {
                SequenceDocumentEdit::DeleteMarkCollection { key }
            }
            SequenceGuiEditDto::SetMarkCollectionColor { key, color } => {
                SequenceDocumentEdit::SetMarkCollectionColor { key, color }
            }
            SequenceGuiEditDto::AddMark {
                collection_key,
                time_seconds,
            } => SequenceDocumentEdit::AddMark {
                collection_key,
                time_seconds,
            },
            SequenceGuiEditDto::MoveMark {
                collection_key,
                index,
                time_seconds,
            } => SequenceDocumentEdit::MoveMark {
                collection_key,
                index: index as usize,
                time_seconds,
            },
            SequenceGuiEditDto::DeleteMark {
                collection_key,
                index,
            } => SequenceDocumentEdit::DeleteMark {
                collection_key,
                index: index as usize,
            },
        };
        let base_content = self.active_buffer_text()?;
        let edit_overlays = self.editor.dirty_overlays();
        let outcome = self.workspace.apply_sequence_edit(
            path.clone(),
            &object_key,
            edit,
            base_content,
            edit_overlays,
        )?;
        self.save_active_sequence_gui_text(
            path,
            outcome.serialized_content,
            outcome.refreshed_document,
            PreviewSyncMode::DeferRender,
        )?;
        Ok(())
    }

    pub fn apply_sequence_selection_edit(
        &mut self,
        edit: SequenceSelectionEditDto,
        sequence_clipboard: &mut Option<SequenceClipboard>,
    ) -> Result<SequenceSelectionEditResultDto, String> {
        let before = self.active_sequence_authored()?;
        let before_document = self.active_sequence_document()?;
        let resulting_selection;
        let mut copied_count = 0;
        let mut skipped_count = 0;

        let document_edit = match edit {
            SequenceSelectionEditDto::Copy { selection } => {
                copied_count = self.copy_sequence_selection(
                    sequence_clipboard,
                    &before,
                    &before_document,
                    &selection,
                )?;
                self.status = RuntimeStatus::message(format!("Copied {copied_count}"));
                return Ok(SequenceSelectionEditResultDto {
                    selection: Some(selection),
                    copied_count,
                    skipped_count,
                });
            }
            SequenceSelectionEditDto::Cut { selection } => {
                copied_count = self.copy_sequence_selection(
                    sequence_clipboard,
                    &before,
                    &before_document,
                    &selection,
                )?;
                let edit = sequence_delete_edit(selection.clone());
                self.status = RuntimeStatus::message(format!("Cut {copied_count}"));
                resulting_selection = Some(selection_empty_like(&selection));
                edit
            }
            SequenceSelectionEditDto::Delete { selection } => {
                let edit = sequence_delete_edit(selection.clone());
                self.status = RuntimeStatus::message("Deleted selection");
                resulting_selection = Some(selection_empty_like(&selection));
                edit
            }
            SequenceSelectionEditDto::Paste { anchor } => {
                let (edit, selection, skipped) =
                    self.sequence_paste_edit(sequence_clipboard, &before_document, anchor)?;
                skipped_count = skipped;
                copied_count = selection_count(&selection);
                self.status = RuntimeStatus::message(if skipped_count == 0 {
                    format!("Pasted {copied_count}")
                } else {
                    format!("Pasted {copied_count}, skipped {skipped_count}")
                });
                resulting_selection = Some(selection);
                edit
            }
            SequenceSelectionEditDto::MoveEffects {
                ids,
                time_delta_seconds,
                lane_delta,
            } => {
                let edits = effect_move_edits(
                    &before_document,
                    ids.clone(),
                    time_delta_seconds,
                    lane_delta,
                );
                resulting_selection = Some(SequenceSelectionDto::Effects { ids });
                SequenceDocumentEdit::MoveEffects { edits }
            }
            SequenceSelectionEditDto::ResizeEffects {
                ids,
                edge,
                time_delta_seconds,
            } => {
                let edits =
                    effect_resize_edits(&before_document, ids.clone(), edge, time_delta_seconds);
                resulting_selection = Some(SequenceSelectionDto::Effects { ids });
                SequenceDocumentEdit::ResizeEffects { edits }
            }
            SequenceSelectionEditDto::MoveMarks {
                marks,
                time_delta_seconds,
            } => {
                let edits = mark_move_edits(&before_document, marks.clone(), time_delta_seconds);
                resulting_selection = Some(SequenceSelectionDto::Marks { marks });
                SequenceDocumentEdit::MoveMarks { edits }
            }
        };

        self.apply_sequence_document_edit(document_edit)?;
        self.flush_autosave_without_analysis()?;
        Ok(SequenceSelectionEditResultDto {
            selection: resulting_selection,
            copied_count,
            skipped_count,
        })
    }

    fn apply_sequence_document_edit(&mut self, edit: SequenceDocumentEdit) -> Result<(), String> {
        self.ensure_active_buffer_not_conflicted()?;
        let path = self.active_path_for_gui_edit()?;
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), self.editor.dirty_overlays())?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
            .ok_or_else(|| "active document is not a sequence".to_string())?
            .clone();
        let outcome = self.workspace.apply_sequence_edit(
            path.clone(),
            &object_key,
            edit,
            self.active_buffer_text()?,
            self.editor.dirty_overlays(),
        )?;
        self.save_active_sequence_gui_text(
            path,
            outcome.serialized_content,
            outcome.refreshed_document,
            PreviewSyncMode::DeferRender,
        )
    }

    fn active_sequence_authored(&self) -> Result<dawn_language::model::Sequence<Authored>, String> {
        let object_key = self.active_sequence_object_key()?;
        let parsed = parse_dawn_file_with_source_map(&self.active_buffer_text()?)
            .map_err(|error| error.to_string())?;
        match parsed.file.get(&object_key) {
            Some(DawnObject::Sequence(sequence)) => Ok(sequence.clone()),
            _ => Err(format!("sequence object `{object_key}` was not found")),
        }
    }

    fn active_sequence_document(&self) -> Result<SequenceDocument, String> {
        let path = self.active_path_for_gui_edit()?;
        let object_key = self.active_sequence_object_key()?;
        let Some(buffer) = self.editor.active_buffer() else {
            return Err("no active document".to_string());
        };
        if buffer.view_mode != EditorViewMode::Gui || buffer.is_conflicted() {
            return Err("active sequence GUI document is not available".to_string());
        }
        self.workspace
            .sequence_document(path, &object_key, self.editor.dirty_overlays())
    }

    fn active_sequence_object_key(&self) -> Result<String, String> {
        let path = self.active_path_for_gui_edit()?;
        let descriptor = self
            .workspace
            .inspect_document(path, self.editor.dirty_overlays())?;
        descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
            .cloned()
            .ok_or_else(|| "active document is not a sequence".to_string())
    }

    fn copy_sequence_selection(
        &self,
        sequence_clipboard: &mut Option<SequenceClipboard>,
        sequence: &dawn_language::model::Sequence<Authored>,
        document: &SequenceDocument,
        selection: &SequenceSelectionDto,
    ) -> Result<u32, String> {
        match selection {
            SequenceSelectionDto::Effects { ids } => {
                let effects = ids
                    .iter()
                    .filter_map(|id| {
                        sequence
                            .effects
                            .iter()
                            .find(|effect| effect.id == *id)
                            .cloned()
                    })
                    .collect::<Vec<_>>();
                let count = effects.len().min(u32::MAX as usize) as u32;
                *sequence_clipboard = Some(SequenceClipboard::Effects(effects));
                Ok(count)
            }
            SequenceSelectionDto::Marks { marks } => {
                let copied = marks
                    .iter()
                    .filter_map(|mark| {
                        document
                            .mark_collections
                            .iter()
                            .find(|collection| collection.key == mark.collection_key)
                            .and_then(|collection| {
                                collection.marks_seconds.get(mark.index as usize)
                            })
                            .map(|time_seconds| SequenceMarkPasteDocumentEdit {
                                collection_key: mark.collection_key.clone(),
                                time_seconds: *time_seconds,
                            })
                    })
                    .collect::<Vec<_>>();
                let count = copied.len().min(u32::MAX as usize) as u32;
                *sequence_clipboard = Some(SequenceClipboard::Marks(copied));
                Ok(count)
            }
        }
    }

    fn sequence_paste_edit(
        &self,
        sequence_clipboard: &Option<SequenceClipboard>,
        document: &SequenceDocument,
        anchor: SequencePasteAnchorDto,
    ) -> Result<(SequenceDocumentEdit, SequenceSelectionDto, u32), String> {
        match sequence_clipboard.clone() {
            Some(SequenceClipboard::Effects(effects)) => {
                let first_id = document
                    .effects
                    .iter()
                    .map(|effect| effect.id)
                    .max()
                    .unwrap_or(0)
                    + 1;
                let ids = (0..effects.len())
                    .map(|offset| first_id + offset as u32)
                    .collect::<Vec<_>>();
                Ok((
                    SequenceDocumentEdit::PasteEffects {
                        effects,
                        lane_index: anchor.lane_index.map(|value| value as usize),
                        time_seconds: anchor.time_seconds,
                    },
                    SequenceSelectionDto::Effects { ids },
                    0,
                ))
            }
            Some(SequenceClipboard::Marks(marks)) => {
                let existing = document
                    .mark_collections
                    .iter()
                    .map(|collection| (collection.key.clone(), collection.marks_seconds.len()))
                    .collect::<std::collections::HashMap<_, _>>();
                let mut refs = Vec::new();
                for mark in &marks {
                    if let Some(index) = existing.get(&mark.collection_key) {
                        refs.push(SequenceMarkRefDto {
                            collection_key: mark.collection_key.clone(),
                            index: *index as u32,
                        });
                    }
                }
                let skipped = marks
                    .len()
                    .saturating_sub(refs.len())
                    .min(u32::MAX as usize) as u32;
                Ok((
                    SequenceDocumentEdit::PasteMarks {
                        marks,
                        time_seconds: anchor.time_seconds,
                    },
                    SequenceSelectionDto::Marks { marks: refs },
                    skipped,
                ))
            }
            None => Err("sequence clipboard is empty".to_string()),
        }
    }

    fn apply_layout_gui_edit(&mut self, edit: LayoutGuiEditDto) -> Result<(), String> {
        self.ensure_active_buffer_not_conflicted()?;
        let path = self.active_path_for_gui_edit()?;
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), self.editor.dirty_overlays())?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Layout)
            .ok_or_else(|| "active document is not a layout".to_string())?
            .clone();
        let mut document = self.workspace.layout_document(
            path.clone(),
            &object_key,
            self.editor.dirty_overlays(),
        )?;
        match edit {
            LayoutGuiEditDto::UpdatePlacementTransform { id, transform } => {
                let id = dawn_language::model::FixtureId(id);
                let placement = document
                    .fixtures
                    .iter_mut()
                    .find(|fixture| fixture.id == id)
                    .ok_or_else(|| format!("fixture placement `{id}` was not found"))?;
                placement.transform = transform
                    .try_into()
                    .map_err(|error: &'static str| error.to_string())?;
            }
        }
        let outcome = self.workspace.apply_layout_edit(
            path,
            &object_key,
            document,
            self.active_buffer_text()?,
            self.editor.dirty_overlays(),
        )?;
        self.save_active_gui_text(outcome.serialized_content)
    }

    fn apply_fixture_gui_edit(&mut self, edit: FixtureGuiEditDto) -> Result<(), String> {
        self.ensure_active_buffer_not_conflicted()?;
        let path = self.active_path_for_gui_edit()?;
        let mut document =
            self.workspace
                .fixture_document(path.clone(), None, self.editor.dirty_overlays())?;
        match edit {
            FixtureGuiEditDto::UpdateBulbDiameter {
                object_key,
                bulb_diameter_meters,
            } => {
                let fixture = document
                    .fixtures
                    .iter_mut()
                    .find(|fixture| fixture.object_key == object_key)
                    .ok_or_else(|| format!("fixture `{object_key}` was not found"))?;
                fixture.bulb_diameter =
                    dawn_language::model::DistanceSpan::try_from_meters_f64_truncated(
                        bulb_diameter_meters,
                    )
                    .map_err(str::to_string)?;
            }
            FixtureGuiEditDto::MovePoint {
                object_key,
                point_index,
                point,
            } => {
                let fixture = document
                    .fixtures
                    .iter_mut()
                    .find(|fixture| fixture.object_key == object_key)
                    .ok_or_else(|| format!("fixture `{object_key}` was not found"))?;
                let Geometry::Points { points } = &mut fixture.geometry else {
                    return Err("only point geometry can be edited in this milestone".to_string());
                };
                let target = points
                    .get_mut(point_index as usize)
                    .ok_or_else(|| format!("point `{point_index}` was not found"))?;
                *target = point
                    .try_into()
                    .map_err(|error: &'static str| error.to_string())?;
            }
        }
        let outcome = self.workspace.apply_fixture_edit(
            path,
            document,
            self.active_buffer_text()?,
            self.editor.dirty_overlays(),
        )?;
        self.save_active_gui_text(outcome.serialized_content)
    }

    fn active_path_for_gui_edit(&self) -> Result<Utf8PathBuf, String> {
        self.editor
            .active_file()
            .cloned()
            .ok_or_else(|| "no active document".to_string())
    }

    fn active_buffer_text(&self) -> Result<String, String> {
        self.editor
            .active_buffer()
            .map(|buffer| buffer.text.clone())
            .ok_or_else(|| "no active document".to_string())
    }

    fn save_active_gui_text(&mut self, text: String) -> Result<(), String> {
        self.editor.replace_active_text_from_gui(text);
        self.refresh_analysis_after_memory_edit();
        Ok(())
    }

    fn save_active_sequence_gui_text(
        &mut self,
        path: Utf8PathBuf,
        text: String,
        document: SequenceDocument,
        mode: PreviewSyncMode,
    ) -> Result<(), String> {
        self.editor.replace_active_text_from_gui(text);
        self.sync_preview_source_from_document(path, document, mode);
        Ok(())
    }

    fn ensure_active_buffer_not_conflicted(&self) -> Result<(), String> {
        let Some(buffer) = self.editor.active_buffer() else {
            return Ok(());
        };
        if buffer.is_conflicted() {
            return Err("active document has external disk changes".to_string());
        }
        Ok(())
    }

    fn active_sequence_source(
        &self,
    ) -> Option<(SequenceKey, dawn_language::document::SequenceDocument)> {
        let path = self.editor.active_file()?.clone();
        let overlays = self.editor.dirty_overlays();
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), overlays.clone())
            .ok()?;
        let object_key = descriptor
            .default_object_keys
            .get(&dawn_language::document::DocumentViewId::Sequence)?;
        let document = self
            .workspace
            .sequence_document(path.clone(), object_key, overlays)
            .ok()?;
        Some((
            SequenceKey {
                path,
                object_key: document.object_key.clone(),
            },
            document,
        ))
    }
}
