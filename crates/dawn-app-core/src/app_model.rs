use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use dawn_project::{
    AssetPath, Curve, CurveDefinitionKey, CurvePoint, CurveUse, CurveValue, CurveValueType,
    DawnProject, DistanceSpan, EffectDefinitionKey, EffectParam, EffectParamArrayValue,
    EffectTarget, Fixture, FixtureDefinitionKey, FixtureId, Flags, Geometry, LayoutDefinitionKey,
    PathStringExt, ProjectDiagnostic, ResolvedAssetPath, ResolvedInlineOrRef, ResolvedSymbolRef,
    Sequence, SequenceDefinitionKey, SequenceEffect, SequenceEffectId, SequenceMarkCollection,
    SymbolRef, Time, TimeSpan, Utf8PathBuf, WorkspaceEntry,
};

use crate::actions::AppAction;
use crate::document::{
    DocumentDescriptor, DocumentViewId, FixtureDocument, LayoutDocument, SequenceDocument,
};
use crate::dto::AppSnapshotDto;
use crate::dto::{
    FixtureGuiEditDto, LayoutGuiEditDto, SequenceGuiEditDto, SequenceMarkRefDto,
    SequencePasteAnchorDto, SequenceResizeEdgeDto, SequenceSelectionDto, SequenceSelectionEditDto,
    SequenceSelectionEditResultDto,
};
use crate::editor_session::{
    BufferExternalState, EditorBuffer, EditorSession, EditorViewMode, OpenFileOutcome,
};
use crate::layout_persistence::{
    load_workbench_layout, save_workbench_layout, WindowLayout, WorkbenchLayout,
};
use crate::preview_session::{
    PreviewRenderRequest, PreviewRenderResult, PreviewSession, PreviewSnapshot, PreviewSyncMode,
    SequenceKey,
};
use crate::workspace::WorkspaceService;

const MIN_EFFECT_DURATION_SECONDS: f64 = 0.000000001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchOutcome {
    SnapshotChanged,
    NoSnapshotChange,
}

impl DispatchOutcome {
    pub fn snapshot_changed(self) -> bool {
        matches!(self, Self::SnapshotChanged)
    }
}

#[derive(Debug)]
pub struct AppModel {
    pub workspace: WorkspaceService,
    pub editors: EditorSession,
    pub workbench_layout: WorkbenchLayout,
    pub preview: PreviewSession,
    pub live_output: LiveOutputSnapshot,
    pub project_root: Option<String>,
    pub project_entries: Vec<WorkspaceEntry>,
    pub project: Option<Arc<DawnProject>>,
    pub diagnostics: Vec<ProjectDiagnostic>,
    pub status: String,
    pub sequence_clipboard: Option<SequenceClipboard>,
    active_sequence_gui_document: Option<CachedActiveGuiDocument>,
    gui_undo_stack: Vec<DawnProject>,
    gui_redo_stack: Vec<DawnProject>,
    last_command_timing: CommandTiming,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CommandTiming {
    pub total_ms: f64,
    pub model_lock_wait_ms: f64,
    pub dispatch_ms: f64,
    pub snapshot_ms: f64,
    pub app_snapshot_emit_ms: f64,
}

#[derive(Debug, Clone)]
pub enum SequenceClipboard {
    Effects(Vec<SequenceEffect<dawn_project::Resolved>>),
    Marks(Vec<SequenceMarkClipboardItem>),
}

#[derive(Debug, Clone)]
pub struct SequenceMarkClipboardItem {
    pub collection_key: String,
    pub time_seconds: f64,
}

#[derive(Debug, Clone)]
struct CachedActiveGuiDocument {
    path: Utf8PathBuf,
    object_key: String,
    view_mode: EditorViewMode,
    document: SequenceDocument,
}

#[derive(Debug, Clone)]
pub struct AppSnapshot {
    pub project_root: Option<String>,
    pub project_entries: Vec<WorkspaceEntry>,
    pub diagnostics: Vec<ProjectDiagnostic>,
    pub workbench_layout: WorkbenchLayout,
    pub preview: PreviewSnapshot,
    pub live_output: LiveOutputSnapshot,
    pub tabs: Vec<EditorBuffer>,
    pub active_file: Option<Utf8PathBuf>,
    pub active_buffer: Option<EditorBuffer>,
    pub active_document_descriptor: Option<DocumentDescriptor>,
    pub active_gui_document: Option<ActiveGuiDocument>,
    pub status: String,
}

#[derive(Debug, Clone)]
pub enum ActiveGuiDocument {
    Sequence(SequenceDocument),
    Layout(LayoutDocument),
    Fixture(FixtureDocument),
    Blocked {
        reason: String,
        diagnostics: Vec<ProjectDiagnostic>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveOutputSnapshot {
    pub enabled: bool,
    pub status: LiveOutputStatus,
    pub active_universe_count: usize,
    pub last_error: Option<String>,
}

impl Default for LiveOutputSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            status: LiveOutputStatus::Disabled,
            active_universe_count: 0,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveOutputStatus {
    Disabled,
    Ready,
    Sending,
    Error,
}

impl LiveOutputStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::Ready => "Ready",
            Self::Sending => "Sending",
            Self::Error => "Error",
        }
    }
}

impl Default for AppModel {
    fn default() -> Self {
        let workbench_layout = load_workbench_layout();
        let last_project_root = workbench_layout.last_project_root.clone();
        let mut model = Self {
            workspace: WorkspaceService::default(),
            editors: EditorSession::default(),
            workbench_layout,
            preview: PreviewSession::default(),
            live_output: LiveOutputSnapshot::default(),
            project_root: None,
            project_entries: Vec::new(),
            project: None,
            diagnostics: Vec::new(),
            status: "No project open".to_string(),
            sequence_clipboard: None,
            active_sequence_gui_document: None,
            gui_undo_stack: Vec::new(),
            gui_redo_stack: Vec::new(),
            last_command_timing: CommandTiming::default(),
        };
        if let Some(path) = last_project_root {
            match model.open_project(path, false, true) {
                Ok(()) => model.status = "Project restored".to_string(),
                Err(error) => model.status = format!("Could not restore last project: {error}"),
            }
        }
        model
    }
}

impl AppModel {
    pub fn snapshot(&self) -> AppSnapshot {
        let active_document_descriptor = self.active_document_descriptor();
        let active_gui_document = self.active_gui_document(active_document_descriptor.as_ref());
        AppSnapshot {
            project_root: self.project_root.clone(),
            project_entries: self.project_entries.clone(),
            diagnostics: self.diagnostics.clone(),
            workbench_layout: self.workbench_layout.clone(),
            preview: self.preview.snapshot(),
            live_output: self.live_output.clone(),
            tabs: self.editors.tabs(),
            active_file: self.editors.active_file().cloned(),
            active_buffer: self.editors.active_buffer().cloned(),
            active_document_descriptor,
            active_gui_document,
            status: self.status.clone(),
        }
    }

    pub fn snapshot_dto(&self) -> AppSnapshotDto {
        self.snapshot().into()
    }

    pub fn set_last_command_timing(&mut self, timing: CommandTiming) {
        self.last_command_timing = timing;
    }

    pub fn last_command_timing(&self) -> CommandTiming {
        self.last_command_timing
    }

    pub fn dispatch(&mut self, action: AppAction) -> Result<DispatchOutcome, String> {
        match action {
            AppAction::OpenProject(path) => {
                self.flush_autosave()?;
                self.open_project(path, true, false)?;
                self.status = "Project opened".to_string();
            }
            AppAction::ReloadProject => {
                self.flush_autosave()?;
                let paths = self
                    .editors
                    .buffers()
                    .into_iter()
                    .map(|buffer| buffer.path)
                    .collect();
                self.reconcile_filesystem_changes(paths)?;
                self.reload_project_model();
                self.status = "Project checked".to_string();
            }
            AppAction::OpenFile(path) => {
                let (text, disk_version) = self.workspace.read_file_with_version(path.clone())?;
                match self
                    .editors
                    .open_or_reconcile_file(path.clone(), text, disk_version)
                {
                    OpenFileOutcome::Opened | OpenFileOutcome::Activated => {}
                    OpenFileOutcome::ReloadedFromDisk | OpenFileOutcome::MarkedChangedOnDisk => {
                        self.invalidate_sequence_gui_state_for_path(&path);
                    }
                }
                self.reload_project_model();
                self.sync_preview_source(PreviewSyncMode::RenderNow);
                self.persist_workbench_layout()?;
            }
            AppAction::CloseFile(path) => {
                self.editors.close_file(&path);
                self.invalidate_sequence_gui_state_for_path(&path);
                self.reload_project_model();
                self.sync_preview_source(PreviewSyncMode::RenderNow);
                self.persist_workbench_layout()?;
            }
            AppAction::SetActiveFile(path) => {
                let active_changed = self.editors.active_file() != Some(&path);
                self.editors.set_active_file(path.clone());
                let reconciled = self.reconcile_open_buffer_from_disk(&path)?;
                if active_changed || reconciled {
                    self.invalidate_active_gui_document_cache();
                    self.preview.pause(self.project.as_deref());
                    if reconciled {
                        self.reload_project_model();
                    }
                    self.sync_preview_source(PreviewSyncMode::RenderNow);
                    self.persist_workbench_layout()?;
                }
            }
            AppAction::SetActiveViewMode(mode) => {
                let Some(path) = self.editors.active_file().cloned() else {
                    return Ok(DispatchOutcome::NoSnapshotChange);
                };
                self.ensure_dirty_text_saved_for_gui()?;
                self.editors.set_view_mode(&path, mode.into());
                self.invalidate_active_gui_document_cache();
                self.sync_preview_source(PreviewSyncMode::RenderNow);
                self.persist_workbench_layout()?;
            }
            AppAction::UpdateActiveText(text) => {
                let path = self.active_path_for_gui_edit()?;
                self.ensure_active_buffer_not_conflicted()?;
                self.editors.update_active_text(text);
                self.invalidate_sequence_gui_state_for_path(&path);
                self.reload_project_model();
                self.sync_preview_source(PreviewSyncMode::RenderNow);
                self.status = "Edited".to_string();
            }
            AppAction::UndoActiveEdit => {
                if !self.undo_active_edit()? {
                    return Ok(DispatchOutcome::NoSnapshotChange);
                }
            }
            AppAction::RedoActiveEdit => {
                if !self.redo_active_edit()? {
                    return Ok(DispatchOutcome::NoSnapshotChange);
                }
            }
            AppAction::ApplySequenceGuiEdit(edit) => {
                self.apply_sequence_gui_edit(edit)?;
                self.status = "Autosaved".to_string();
            }
            AppAction::ApplyLayoutGuiEdit(edit) => {
                self.apply_layout_gui_edit(edit)?;
                self.status = "Autosaved".to_string();
            }
            AppAction::ApplyFixtureGuiEdit(edit) => {
                self.apply_fixture_gui_edit(edit)?;
                self.status = "Autosaved".to_string();
            }
            AppAction::FlushAutosave => {
                self.flush_autosave()?;
                self.status = "Saved".to_string();
            }
            AppAction::FilesystemChanged(paths) => {
                self.reconcile_filesystem_changes(paths)?;
                self.reload_project_model();
                self.status = "Filesystem refreshed".to_string();
            }
            AppAction::ReloadActiveBufferFromDisk => {
                self.reload_active_buffer_from_disk()?;
                self.status = "Reloaded from disk".to_string();
            }
            AppAction::KeepActiveBuffer => {
                self.keep_active_buffer()?;
                self.status = "Kept IDE changes".to_string();
            }
            AppAction::CreateFile { parent, name } => {
                self.flush_autosave()?;
                let path = self.workspace.create_file(parent, &name)?;
                self.refresh_project_entries()?;
                let (text, disk_version) = self.workspace.read_file_with_version(path.clone())?;
                self.editors.open_file(path, text, disk_version);
                self.reload_project_model();
                self.sync_preview_source(PreviewSyncMode::RenderNow);
                self.persist_workbench_layout()?;
            }
            AppAction::CreateDirectory { parent, name } => {
                self.flush_autosave()?;
                self.workspace.create_directory(parent, &name)?;
                self.refresh_project_entries()?;
                self.reload_project_model();
                self.sync_preview_source(PreviewSyncMode::RenderNow);
            }
            AppAction::RenamePath { path, new_name } => {
                self.flush_autosave()?;
                let moves = self.workspace.rename_path(path.clone(), &new_name)?;
                self.refresh_project_entries()?;
                self.editors.reconcile_moved_paths(&moves);
                self.reload_project_model();
                self.sync_preview_source(PreviewSyncMode::RenderNow);
                self.persist_workbench_layout()?;
            }
            AppAction::DeletePath(path) => {
                self.flush_autosave()?;
                self.workspace.delete_path(path.clone())?;
                self.refresh_project_entries()?;
                self.editors.reconcile_deleted_path(&path);
                self.reload_project_model();
                self.sync_preview_source(PreviewSyncMode::RenderNow);
                self.persist_workbench_layout()?;
            }
            AppAction::ToggleProjectTree => {
                self.workbench_layout.project_tree_visible =
                    !self.workbench_layout.project_tree_visible;
                save_workbench_layout(&self.workbench_layout)?;
            }
            AppAction::SetEffectPreviewEnabled(enabled) => {
                self.workbench_layout.effect_preview_enabled = enabled;
                save_workbench_layout(&self.workbench_layout)?;
                if !enabled {
                    self.preview.clear_effect_preview(self.project.as_deref());
                } else {
                    self.preview.render_current_frame(self.project.as_deref());
                }
            }
            AppAction::SetEffectPreviewEffects(ids) => {
                let ids = if self.workbench_layout.effect_preview_enabled {
                    ids
                } else {
                    Vec::new()
                };
                self.preview
                    .set_effect_preview_ids(ids, self.project.as_deref());
            }
            AppAction::SetTerminalPanelLayout(layout) => {
                if !layout.width_px.is_finite() || layout.width_px < 260.0 {
                    return Err(
                        "terminal panel width must be finite and at least 260px".to_string()
                    );
                }
                self.workbench_layout.terminal_panel = layout.into();
                save_workbench_layout(&self.workbench_layout)?;
            }
            AppAction::PreviewPlay => {
                self.preview.play(self.project.as_deref());
                self.status = "Preview playing".to_string();
            }
            AppAction::PreviewPause => {
                self.preview.pause(self.project.as_deref());
                self.status = "Preview paused".to_string();
            }
            AppAction::PreviewStop => {
                self.preview.stop(self.project.as_deref());
                self.status = "Preview stopped".to_string();
            }
            AppAction::PreviewRewindToZero => {
                self.preview
                    .go_to_sequence_beginning(self.project.as_deref());
                self.status = "Preview rewound".to_string();
            }
            AppAction::PreviewSeek(position_seconds) => {
                self.preview.seek(position_seconds, self.project.as_deref());
                self.status = "Preview seeked".to_string();
            }
        }
        Ok(DispatchOutcome::SnapshotChanged)
    }

    pub fn tick_preview(&mut self) {
        self.preview.tick(self.project.as_deref());
    }

    pub fn tick_preview_clock(&mut self) {
        self.preview.tick_clock();
    }

    pub fn render_preview_frame(&mut self) {
        self.preview.render_current_frame(self.project.as_deref());
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

    pub fn set_live_output_snapshot(&mut self, snapshot: LiveOutputSnapshot) {
        self.live_output = snapshot;
    }

    pub fn set_main_window_layout(&mut self, layout: WindowLayout) -> Result<(), String> {
        self.workbench_layout.main_window = layout;
        save_workbench_layout(&self.workbench_layout)
    }

    pub fn set_preview_window_layout(&mut self, layout: WindowLayout) -> Result<(), String> {
        self.workbench_layout.preview_window = layout;
        save_workbench_layout(&self.workbench_layout)
    }

    pub fn set_preview_window_open(&mut self, open: bool) -> Result<(), String> {
        self.workbench_layout.preview_window_open = open;
        save_workbench_layout(&self.workbench_layout)
    }

    fn open_project(
        &mut self,
        path: PathBuf,
        remember: bool,
        restore_editor_session: bool,
    ) -> Result<(), String> {
        self.workspace.open_project(&path)?;
        self.refresh_project_entries()?;
        self.editors.clear();
        self.preview.reset();
        if restore_editor_session {
            self.restore_editor_session();
        }
        self.reload_project_model();
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        if remember {
            self.workbench_layout.last_project_root = Some(path);
            self.persist_workbench_layout()?;
        }
        Ok(())
    }

    fn restore_editor_session(&mut self) {
        let tabs = self
            .workbench_layout
            .editor_session
            .tabs
            .clone()
            .into_iter()
            .filter_map(|tab| {
                self.workspace
                    .read_file_with_version(tab.path.clone())
                    .ok()
                    .map(|(text, disk_version)| (tab.path, text, disk_version, tab.view_mode))
            })
            .collect();
        self.editors.restore(
            tabs,
            self.workbench_layout.editor_session.active_file.clone(),
        );
    }

    fn persist_workbench_layout(&mut self) -> Result<(), String> {
        self.workbench_layout.editor_session = self.editors.state();
        save_workbench_layout(&self.workbench_layout)
    }

    pub fn refresh_project_entries(&mut self) -> Result<(), String> {
        self.project_root = self
            .workspace
            .project_root_display()
            .map(ToString::to_string);
        self.project_entries = self.workspace.project_entries()?;
        Ok(())
    }

    fn reload_project_model(&mut self) {
        self.invalidate_active_gui_document_cache();
        let result = self.workspace.load_project();
        self.diagnostics = result.diagnostics;
        self.project = result.project.map(Arc::new);
    }

    pub fn flush_autosave(&mut self) -> Result<(), String> {
        let dirty_buffers = self.editors.dirty_autosave_buffers();
        let had_dirty_buffers = dirty_buffers
            .iter()
            .any(|buffer| buffer.text != buffer.saved_text);
        for buffer in dirty_buffers {
            if buffer.text == buffer.saved_text {
                continue;
            }
            let version = self
                .workspace
                .write_text_file_with_version(buffer.path.clone(), &buffer.text)?;
            self.editors.record_saved_version(&buffer.path, version);
        }
        if had_dirty_buffers {
            self.reload_project_model();
            self.sync_preview_source(PreviewSyncMode::RenderNow);
        }
        Ok(())
    }

    fn ensure_dirty_text_saved_for_gui(&mut self) -> Result<(), String> {
        self.flush_autosave()?;
        if self.project.is_none() {
            self.reload_project_model();
        }
        if self.project.is_none() {
            return Err("project did not load; GUI edits are unavailable".to_string());
        }
        Ok(())
    }

    fn save_project_and_refresh_buffers(&mut self) -> Result<(), String> {
        let Some(project) = self.project.as_ref() else {
            return Err("project did not load; GUI edits are unavailable".to_string());
        };
        let result = self.workspace.save_project(project)?;
        if !result.diagnostics.is_empty() {
            self.diagnostics = result.diagnostics;
            return Err("project save failed".to_string());
        }
        for path in result.written_files {
            let text = self.workspace.read_file(path.clone())?;
            let version = self
                .workspace
                .file_version(&path, &text)?
                .ok_or_else(|| format!("saved file `{path}` does not exist"))?;
            self.editors.mark_saved(&path, text, version);
        }
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    fn reconcile_open_buffer_from_disk(&mut self, path: &Utf8PathBuf) -> Result<bool, String> {
        let Some(buffer) = self
            .editors
            .buffers()
            .into_iter()
            .find(|buffer| &buffer.path == path)
        else {
            return Ok(false);
        };

        match self.workspace.read_file_with_version(buffer.path.clone()) {
            Ok((disk_text, disk_version)) => {
                if buffer.disk_version.as_ref() == Some(&disk_version) {
                    return Ok(false);
                }
                self.invalidate_sequence_gui_state_for_path(&buffer.path);
                if buffer.is_dirty() {
                    self.editors
                        .mark_external_state(&buffer.path, BufferExternalState::ChangedOnDisk);
                } else {
                    self.editors
                        .replace_from_disk(&buffer.path, disk_text, disk_version, false);
                }
            }
            Err(_) => {
                self.invalidate_sequence_gui_state_for_path(&buffer.path);
                if buffer.is_dirty() {
                    self.editors
                        .mark_external_state(&buffer.path, BufferExternalState::DeletedOnDisk);
                } else {
                    self.editors.close_file(&buffer.path);
                }
            }
        }
        Ok(true)
    }

    fn reconcile_filesystem_changes(&mut self, paths: Vec<Utf8PathBuf>) -> Result<(), String> {
        let watched_paths = if paths.is_empty() {
            self.editors
                .buffers()
                .into_iter()
                .map(|buffer| buffer.path)
                .collect()
        } else {
            paths
        };
        let buffers = self.editors.buffers();
        for buffer in buffers {
            if !buffer_matches_any_path(&buffer.path, &watched_paths) {
                continue;
            }
            match self.workspace.read_file_with_version(buffer.path.clone()) {
                Ok((disk_text, disk_version)) => {
                    if buffer.disk_version.as_ref() == Some(&disk_version) {
                        continue;
                    }
                    self.invalidate_sequence_gui_state_for_path(&buffer.path);
                    if buffer.is_dirty() {
                        self.editors
                            .mark_external_state(&buffer.path, BufferExternalState::ChangedOnDisk);
                    } else {
                        self.editors.replace_from_disk(
                            &buffer.path,
                            disk_text,
                            disk_version,
                            false,
                        );
                    }
                }
                Err(_) => {
                    self.invalidate_sequence_gui_state_for_path(&buffer.path);
                    if buffer.is_dirty() {
                        self.editors
                            .mark_external_state(&buffer.path, BufferExternalState::DeletedOnDisk);
                    } else {
                        self.editors.close_file(&buffer.path);
                    }
                }
            }
        }
        self.refresh_project_entries()?;
        self.persist_workbench_layout()?;
        Ok(())
    }

    fn invalidate_sequence_gui_state_for_path(&mut self, path: &Utf8PathBuf) {
        if self
            .active_sequence_gui_document
            .as_ref()
            .is_some_and(|cached| &cached.path == path)
        {
            self.invalidate_active_gui_document_cache();
        }
    }

    fn reload_active_buffer_from_disk(&mut self) -> Result<(), String> {
        let Some(buffer) = self.editors.active_buffer().cloned() else {
            return Ok(());
        };
        self.invalidate_sequence_gui_state_for_path(&buffer.path);
        match self.workspace.read_file_with_version(buffer.path.clone()) {
            Ok((text, disk_version)) => {
                self.editors
                    .replace_from_disk(&buffer.path, text, disk_version, true);
            }
            Err(_) => {
                self.editors.close_file(&buffer.path);
                self.persist_workbench_layout()?;
            }
        }
        self.refresh_project_entries()?;
        self.reload_project_model();
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    fn keep_active_buffer(&mut self) -> Result<(), String> {
        let Some(buffer) = self.editors.active_buffer().cloned() else {
            return Ok(());
        };
        self.invalidate_sequence_gui_state_for_path(&buffer.path);
        let version = self
            .workspace
            .write_text_file_with_version(buffer.path.clone(), &buffer.text)?;
        self.editors.record_saved_version(&buffer.path, version);
        self.refresh_project_entries()?;
        self.reload_project_model();
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    fn sync_preview_source(&mut self, mode: PreviewSyncMode) {
        let source = self.active_sequence_source();
        self.cache_active_sequence_source(source.as_ref());
        self.preview
            .sync_source(source, self.project.as_deref(), mode);
    }

    fn cache_active_sequence_source(&mut self, source: Option<&(SequenceKey, SequenceDocument)>) {
        let Some((key, document)) = source else {
            self.invalidate_active_gui_document_cache();
            return;
        };
        let Some(buffer) = self.editors.active_buffer() else {
            self.invalidate_active_gui_document_cache();
            return;
        };
        if buffer.path != key.path
            || buffer.view_mode != EditorViewMode::Gui
            || buffer.is_conflicted()
        {
            self.invalidate_active_gui_document_cache();
            return;
        }
        self.active_sequence_gui_document = Some(CachedActiveGuiDocument {
            path: key.path.clone(),
            object_key: key.object_key.clone(),
            view_mode: buffer.view_mode,
            document: document.clone(),
        });
    }

    fn invalidate_active_gui_document_cache(&mut self) {
        self.active_sequence_gui_document = None;
    }

    fn active_document_descriptor(&self) -> Option<DocumentDescriptor> {
        let path = self.editors.active_file()?.clone();
        let project = self.project.as_ref()?;
        self.workspace.inspect_document(project, path).ok()
    }

    fn active_gui_document(
        &self,
        descriptor: Option<&DocumentDescriptor>,
    ) -> Option<ActiveGuiDocument> {
        let buffer = self.editors.active_buffer()?;
        if buffer.view_mode != EditorViewMode::Gui {
            return None;
        }
        if buffer.is_conflicted() {
            return Some(ActiveGuiDocument::Blocked {
                reason: "This document has external disk changes.".to_string(),
                diagnostics: Vec::new(),
            });
        }
        let diagnostics = self
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.file == buffer.path)
            .cloned()
            .collect::<Vec<_>>();
        let Some(project) = self.project.as_ref() else {
            return Some(ActiveGuiDocument::Blocked {
                reason: "Project did not load.".to_string(),
                diagnostics,
            });
        };
        let Some(descriptor) = descriptor else {
            return Some(ActiveGuiDocument::Blocked {
                reason: "Text could not be parsed as a Dawn document.".to_string(),
                diagnostics,
            });
        };
        if let Some(object_key) = descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
        {
            if let Some(document) =
                self.cached_active_sequence_document(&buffer.path, object_key, buffer.view_mode)
            {
                return Some(ActiveGuiDocument::Sequence(document));
            }
            return Some(
                match self
                    .workspace
                    .sequence_document(project, buffer.path.clone(), object_key)
                {
                    Ok(document) => ActiveGuiDocument::Sequence(document),
                    Err(error) => ActiveGuiDocument::Blocked {
                        reason: error,
                        diagnostics,
                    },
                },
            );
        }
        if let Some(object_key) = descriptor.default_object_keys.get(&DocumentViewId::Layout) {
            return Some(
                match self
                    .workspace
                    .layout_document(project, buffer.path.clone(), object_key)
                {
                    Ok(document) => ActiveGuiDocument::Layout(document),
                    Err(error) => ActiveGuiDocument::Blocked {
                        reason: error,
                        diagnostics,
                    },
                },
            );
        }
        if descriptor
            .default_object_keys
            .contains_key(&DocumentViewId::Fixture)
        {
            return Some(
                match self
                    .workspace
                    .fixture_document(project, buffer.path.clone(), None)
                {
                    Ok(document) => ActiveGuiDocument::Fixture(document),
                    Err(error) => ActiveGuiDocument::Blocked {
                        reason: error,
                        diagnostics,
                    },
                },
            );
        }
        Some(ActiveGuiDocument::Blocked {
            reason: "This document has no GUI editor view.".to_string(),
            diagnostics,
        })
    }

    fn cached_active_sequence_document(
        &self,
        path: &Utf8PathBuf,
        object_key: &str,
        view_mode: EditorViewMode,
    ) -> Option<SequenceDocument> {
        let cached = self.active_sequence_gui_document.as_ref()?;
        if cached.path == *path && cached.object_key == object_key && cached.view_mode == view_mode
        {
            Some(cached.document.clone())
        } else {
            None
        }
    }

    pub fn cached_sequence_document_for_preview_request(
        &self,
        path: &Utf8PathBuf,
        object_key: &str,
    ) -> Result<SequenceDocument, String> {
        let cached = self
            .active_sequence_gui_document
            .as_ref()
            .ok_or_else(|| "sequence preview request does not match active sequence".to_string())?;
        if cached.path == *path && cached.object_key == object_key {
            Ok(cached.document.clone())
        } else {
            Err("sequence preview request does not match active sequence".to_string())
        }
    }

    fn apply_sequence_gui_edit(&mut self, edit: SequenceGuiEditDto) -> Result<(), String> {
        self.ensure_active_buffer_not_conflicted()?;
        self.ensure_dirty_text_saved_for_gui()?;
        self.push_gui_undo_snapshot()?;
        let (path, object_key) = self.active_object_key(DocumentViewId::Sequence)?;
        let project = self.project_mut()?;
        let store_path = canonical_store_path(project, &path);
        let project_snapshot = project.clone();
        let sequence = sequence_mut(project, &path, &object_key)?;
        apply_sequence_edit(&project_snapshot, &store_path, sequence, edit)?;
        self.save_project_and_refresh_buffers()
    }

    pub fn apply_sequence_selection_edit(
        &mut self,
        edit: SequenceSelectionEditDto,
    ) -> Result<SequenceSelectionEditResultDto, String> {
        self.ensure_dirty_text_saved_for_gui()?;
        let document = self.active_sequence_document()?;
        let resulting_selection;
        let mut copied_count = 0;
        let skipped_count = 0;
        match edit {
            SequenceSelectionEditDto::Copy { selection } => {
                copied_count = self.copy_sequence_selection(&document, &selection)?;
                self.status = format!("Copied {copied_count}");
                return Ok(SequenceSelectionEditResultDto {
                    snapshot: self.snapshot_dto(),
                    selection: Some(selection),
                    copied_count,
                    skipped_count,
                });
            }
            SequenceSelectionEditDto::Cut { selection }
            | SequenceSelectionEditDto::Delete { selection } => {
                self.push_gui_undo_snapshot()?;
                let (path, object_key) = self.active_object_key(DocumentViewId::Sequence)?;
                let project = self.project_mut()?;
                let sequence = sequence_mut(project, &path, &object_key)?;
                delete_selection(sequence, &selection);
                resulting_selection = Some(selection_empty_like(&selection));
                self.status = "Deleted selection".to_string();
            }
            SequenceSelectionEditDto::Paste { anchor } => {
                self.push_gui_undo_snapshot()?;
                let (path, object_key) = self.active_object_key(DocumentViewId::Sequence)?;
                let clipboard = self.sequence_clipboard.clone();
                let project = self.project_mut()?;
                let sequence = sequence_mut(project, &path, &object_key)?;
                let selection = paste_clipboard(sequence, clipboard, anchor)?;
                copied_count = selection_count(&selection);
                resulting_selection = Some(selection);
                self.status = format!("Pasted {copied_count}");
            }
            SequenceSelectionEditDto::MoveEffects {
                ids,
                time_delta_seconds,
                lane_delta,
            } => {
                self.push_gui_undo_snapshot()?;
                let (path, object_key) = self.active_object_key(DocumentViewId::Sequence)?;
                let project = self.project_mut()?;
                let sequence = sequence_mut(project, &path, &object_key)?;
                move_effects(sequence, &document, &ids, time_delta_seconds, lane_delta)?;
                resulting_selection = Some(SequenceSelectionDto::Effects { ids });
            }
            SequenceSelectionEditDto::ResizeEffects {
                ids,
                edge,
                time_delta_seconds,
            } => {
                self.push_gui_undo_snapshot()?;
                let (path, object_key) = self.active_object_key(DocumentViewId::Sequence)?;
                let project = self.project_mut()?;
                let sequence = sequence_mut(project, &path, &object_key)?;
                resize_effects(sequence, &ids, edge, time_delta_seconds)?;
                resulting_selection = Some(SequenceSelectionDto::Effects { ids });
            }
            SequenceSelectionEditDto::MoveMarks {
                marks,
                time_delta_seconds,
            } => {
                self.push_gui_undo_snapshot()?;
                let (path, object_key) = self.active_object_key(DocumentViewId::Sequence)?;
                let project = self.project_mut()?;
                let sequence = sequence_mut(project, &path, &object_key)?;
                move_marks(sequence, &marks, time_delta_seconds)?;
                resulting_selection = Some(SequenceSelectionDto::Marks { marks });
            }
        }
        self.save_project_and_refresh_buffers()?;
        Ok(SequenceSelectionEditResultDto {
            snapshot: self.snapshot_dto(),
            selection: resulting_selection,
            copied_count,
            skipped_count,
        })
    }

    fn push_gui_undo_snapshot(&mut self) -> Result<(), String> {
        let project = self
            .project
            .as_ref()
            .ok_or_else(|| "project did not load; GUI edits are unavailable".to_string())?;
        self.gui_undo_stack.push((**project).clone());
        self.gui_redo_stack.clear();
        Ok(())
    }

    fn undo_active_edit(&mut self) -> Result<bool, String> {
        if self
            .editors
            .active_buffer()
            .is_some_and(|buffer| buffer.view_mode == EditorViewMode::Gui)
        {
            let Some(previous) = self.gui_undo_stack.pop() else {
                return Ok(false);
            };
            if let Some(current) = self.project.replace(Arc::new(previous)) {
                self.gui_redo_stack.push((*current).clone());
            }
            self.save_project_and_refresh_buffers()?;
            self.status = "Undo".to_string();
            return Ok(true);
        }
        if self.editors.undo_active_text_edit() {
            self.reload_project_model();
            self.sync_preview_source(PreviewSyncMode::RenderNow);
            self.status = "Undo".to_string();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn redo_active_edit(&mut self) -> Result<bool, String> {
        if self
            .editors
            .active_buffer()
            .is_some_and(|buffer| buffer.view_mode == EditorViewMode::Gui)
        {
            let Some(next) = self.gui_redo_stack.pop() else {
                return Ok(false);
            };
            if let Some(current) = self.project.replace(Arc::new(next)) {
                self.gui_undo_stack.push((*current).clone());
            }
            self.save_project_and_refresh_buffers()?;
            self.status = "Redo".to_string();
            return Ok(true);
        }
        if self.editors.redo_active_text_edit() {
            self.reload_project_model();
            self.sync_preview_source(PreviewSyncMode::RenderNow);
            self.status = "Redo".to_string();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn active_sequence_document(&self) -> Result<SequenceDocument, String> {
        let path = self.active_path_for_gui_edit()?;
        let object_key = self.active_sequence_object_key()?;
        let project = self
            .project
            .as_ref()
            .ok_or_else(|| "project did not load".to_string())?;
        self.workspace.sequence_document(project, path, &object_key)
    }

    fn active_sequence_object_key(&self) -> Result<String, String> {
        self.active_object_key(DocumentViewId::Sequence)
            .map(|(_, key)| key)
    }

    fn copy_sequence_selection(
        &mut self,
        document: &SequenceDocument,
        selection: &SequenceSelectionDto,
    ) -> Result<u32, String> {
        match selection {
            SequenceSelectionDto::Effects { ids } => {
                let (path, object_key) = self.active_object_key(DocumentViewId::Sequence)?;
                let project = self
                    .project
                    .as_ref()
                    .ok_or_else(|| "project did not load".to_string())?;
                let sequence = sequence_ref(project, &path, &object_key)?;
                let effects = ids
                    .iter()
                    .filter_map(|id| {
                        sequence
                            .effects
                            .iter()
                            .find(|effect| effect.id.0 == *id)
                            .cloned()
                    })
                    .collect::<Vec<_>>();
                let count = effects.len().min(u32::MAX as usize) as u32;
                self.sequence_clipboard = Some(SequenceClipboard::Effects(effects));
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
                            .map(|time_seconds| SequenceMarkClipboardItem {
                                collection_key: mark.collection_key.clone(),
                                time_seconds: *time_seconds,
                            })
                    })
                    .collect::<Vec<_>>();
                let count = copied.len().min(u32::MAX as usize) as u32;
                self.sequence_clipboard = Some(SequenceClipboard::Marks(copied));
                Ok(count)
            }
        }
    }

    fn apply_layout_gui_edit(&mut self, edit: LayoutGuiEditDto) -> Result<(), String> {
        self.ensure_active_buffer_not_conflicted()?;
        self.ensure_dirty_text_saved_for_gui()?;
        self.push_gui_undo_snapshot()?;
        let (path, object_key) = self.active_object_key(DocumentViewId::Layout)?;
        let project = self.project_mut()?;
        let layout = layout_mut(project, &path, &object_key)?;
        match edit {
            LayoutGuiEditDto::UpdatePlacementTransform { id, transform } => {
                let id = FixtureId(id);
                let placement = layout
                    .fixtures
                    .iter_mut()
                    .find(|fixture| fixture.id == id)
                    .ok_or_else(|| format!("fixture placement `{id}` was not found"))?;
                placement.transform = transform
                    .try_into()
                    .map_err(|error: &'static str| error.to_string())?;
            }
        }
        self.save_project_and_refresh_buffers()
    }

    fn apply_fixture_gui_edit(&mut self, edit: FixtureGuiEditDto) -> Result<(), String> {
        self.ensure_active_buffer_not_conflicted()?;
        self.ensure_dirty_text_saved_for_gui()?;
        self.push_gui_undo_snapshot()?;
        let path = self.active_path_for_gui_edit()?;
        let project = self.project_mut()?;
        match edit {
            FixtureGuiEditDto::UpdateBulbDiameter {
                object_key,
                bulb_diameter_meters,
            } => {
                let fixture = fixture_mut(project, &path, &object_key)?;
                fixture.bulb_diameter =
                    DistanceSpan::try_from_meters_f64_truncated(bulb_diameter_meters)
                        .map_err(str::to_string)?;
            }
            FixtureGuiEditDto::MovePoint {
                object_key,
                point_index,
                point,
            } => {
                let fixture = fixture_mut(project, &path, &object_key)?;
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
        self.save_project_and_refresh_buffers()
    }

    fn active_object_key(&self, view: DocumentViewId) -> Result<(Utf8PathBuf, String), String> {
        let path = self.active_path_for_gui_edit()?;
        let project = self
            .project
            .as_ref()
            .ok_or_else(|| "project did not load".to_string())?;
        let descriptor = self.workspace.inspect_document(project, path.clone())?;
        let object_key = descriptor
            .default_object_keys
            .get(&view)
            .cloned()
            .ok_or_else(|| match view {
                DocumentViewId::Sequence => "active document is not a sequence".to_string(),
                DocumentViewId::Layout => "active document is not a layout".to_string(),
                DocumentViewId::Fixture => "active document is not a fixture".to_string(),
                DocumentViewId::Text => "active document is not text".to_string(),
            })?;
        Ok((path, object_key))
    }

    fn active_path_for_gui_edit(&self) -> Result<Utf8PathBuf, String> {
        self.editors
            .active_file()
            .cloned()
            .ok_or_else(|| "no active document".to_string())
    }

    fn project_mut(&mut self) -> Result<&mut DawnProject, String> {
        self.project
            .as_mut()
            .map(Arc::make_mut)
            .ok_or_else(|| "project did not load; GUI edits are unavailable".to_string())
    }

    fn ensure_active_buffer_not_conflicted(&self) -> Result<(), String> {
        let Some(buffer) = self.editors.active_buffer() else {
            return Ok(());
        };
        if buffer.is_conflicted() {
            return Err("active document has external disk changes".to_string());
        }
        Ok(())
    }

    fn active_sequence_source(&self) -> Option<(SequenceKey, SequenceDocument)> {
        let path = self.editors.active_file()?.clone();
        let project = self.project.as_ref()?;
        let descriptor = self
            .workspace
            .inspect_document(project, path.clone())
            .ok()?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)?;
        let document = self
            .workspace
            .sequence_document(project, path.clone(), object_key)
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

fn sequence_ref<'a>(
    project: &'a DawnProject,
    path: &Utf8PathBuf,
    object_key: &str,
) -> Result<&'a Sequence<dawn_project::Resolved>, String> {
    let key = SequenceDefinitionKey::new(canonical_store_path(project, path), object_key);
    project
        .stores
        .sequences
        .get(&key)
        .map(|sequence| &sequence.value)
        .ok_or_else(|| format!("sequence `{object_key}` was not found"))
}

fn sequence_mut<'a>(
    project: &'a mut DawnProject,
    path: &Utf8PathBuf,
    object_key: &str,
) -> Result<&'a mut Sequence<dawn_project::Resolved>, String> {
    let key = SequenceDefinitionKey::new(canonical_store_path(project, path), object_key);
    project
        .stores
        .sequences
        .get_mut(&key)
        .map(|sequence| &mut sequence.value)
        .ok_or_else(|| format!("sequence `{object_key}` was not found"))
}

fn layout_mut<'a>(
    project: &'a mut DawnProject,
    path: &Utf8PathBuf,
    object_key: &str,
) -> Result<&'a mut dawn_project::Layout<dawn_project::Resolved>, String> {
    let key = LayoutDefinitionKey::new(canonical_store_path(project, path), object_key);
    project
        .stores
        .layouts
        .get_mut(&key)
        .map(|layout| &mut layout.value)
        .ok_or_else(|| format!("layout `{object_key}` was not found"))
}

fn fixture_mut<'a>(
    project: &'a mut DawnProject,
    path: &Utf8PathBuf,
    object_key: &str,
) -> Result<&'a mut Fixture, String> {
    let key = FixtureDefinitionKey::new(canonical_store_path(project, path), object_key);
    project
        .stores
        .fixture_definitions
        .get_mut(&key)
        .map(|fixture| &mut fixture.value)
        .ok_or_else(|| format!("fixture `{object_key}` was not found"))
}

fn canonical_store_path(project: &DawnProject, path: &Utf8PathBuf) -> Utf8PathBuf {
    if project.stores.source_files.contains_key(path) {
        return path.clone();
    }
    project
        .stores
        .source_files
        .keys()
        .find(|candidate| candidate.ends_with(path))
        .cloned()
        .unwrap_or_else(|| path.clone())
}

fn apply_sequence_edit(
    project: &DawnProject,
    sequence_path: &Utf8PathBuf,
    sequence: &mut Sequence<dawn_project::Resolved>,
    edit: SequenceGuiEditDto,
) -> Result<(), String> {
    match edit {
        SequenceGuiEditDto::SetAudio { import } => {
            sequence.audio = import
                .map(|raw| {
                    let source = AssetPath::new(raw.clone())?;
                    Ok::<_, String>(ResolvedAssetPath {
                        path: dawn_project::resolve_import_path(sequence_path, source.path()),
                        source,
                    })
                })
                .transpose()?;
        }
        SequenceGuiEditDto::AddEffect {
            script,
            target,
            scope,
            start_seconds,
            mark_collection_key,
        } => {
            let id = sequence
                .effects
                .iter()
                .map(|effect| effect.id.0)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            let mut params = indexmap::IndexMap::new();
            if let Some(key) = mark_collection_key {
                params.insert(key.clone(), EffectParam::Marks { key });
            }
            sequence.effects.push(SequenceEffect {
                id: SequenceEffectId(id),
                start: time_from_seconds(start_seconds)?,
                duration: TimeSpan::try_from_seconds_f64_rounded(1.0).map_err(str::to_string)?,
                target: layout_target_to_effect_target(target)?,
                scope: scope.into(),
                params,
                script: script_reference(project, script)?,
            });
        }
        SequenceGuiEditDto::MoveEffect {
            id,
            start_seconds,
            target,
        } => {
            let effect = effect_mut(sequence, id)?;
            effect.start = time_from_seconds(start_seconds)?;
            if let Some(target) = target {
                effect.target = layout_target_to_effect_target(target)?;
            }
        }
        SequenceGuiEditDto::ResizeEffect {
            id,
            start_seconds,
            duration_seconds,
        } => {
            let effect = effect_mut(sequence, id)?;
            effect.start = time_from_seconds(start_seconds)?;
            effect.duration = duration_from_seconds(duration_seconds)?;
        }
        SequenceGuiEditDto::ChangeEffectScript { id, script } => {
            effect_mut(sequence, id)?.script = script_reference(project, script)?;
        }
        SequenceGuiEditDto::DeleteEffect { id } => {
            sequence.effects.retain(|effect| effect.id.0 != id);
        }
        SequenceGuiEditDto::RetargetEffect { id, target } => {
            effect_mut(sequence, id)?.target = layout_target_to_effect_target(target)?;
        }
        SequenceGuiEditDto::SetEffectScope { id, scope } => {
            effect_mut(sequence, id)?.scope = scope.into();
        }
        SequenceGuiEditDto::UpdateEffectParam { id, name, value } => {
            effect_mut(sequence, id)?
                .params
                .insert(name, effect_param_from_dto(value)?);
        }
        SequenceGuiEditDto::LinkEffectCurveParam {
            id,
            name,
            curve_path,
            object_key,
        } => {
            let key = CurveDefinitionKey::new(Utf8PathBuf::from(curve_path), object_key.clone());
            effect_mut(sequence, id)?.params.insert(
                name,
                EffectParam::Curve {
                    curve: CurveUse {
                        id: 0,
                        curve: ResolvedInlineOrRef::Ref(ResolvedSymbolRef {
                            key,
                            reference: SymbolRef::new(object_key)?,
                        }),
                    },
                },
            );
        }
        SequenceGuiEditDto::UnlinkEffectCurveParam { id, name } => {
            effect_mut(sequence, id)?.params.shift_remove(&name);
        }
        SequenceGuiEditDto::CreateMarkCollection { key, name, color } => {
            sequence.mark_collections.push(SequenceMarkCollection {
                key,
                name,
                color,
                marks: Vec::new(),
            });
        }
        SequenceGuiEditDto::RenameMarkCollection { key, name } => {
            mark_collection_mut(sequence, &key)?.name = name;
        }
        SequenceGuiEditDto::DeleteMarkCollection { key } => {
            sequence
                .mark_collections
                .retain(|collection| collection.key != key);
        }
        SequenceGuiEditDto::SetMarkCollectionColor { key, color } => {
            mark_collection_mut(sequence, &key)?.color = color;
        }
        SequenceGuiEditDto::AddMark {
            collection_key,
            time_seconds,
        } => {
            mark_collection_mut(sequence, &collection_key)?
                .marks
                .push(time_from_seconds(time_seconds)?);
        }
        SequenceGuiEditDto::MoveMark {
            collection_key,
            index,
            time_seconds,
        } => {
            let collection = mark_collection_mut(sequence, &collection_key)?;
            let mark = collection
                .marks
                .get_mut(index as usize)
                .ok_or_else(|| format!("mark `{index}` was not found"))?;
            *mark = time_from_seconds(time_seconds)?;
        }
        SequenceGuiEditDto::DeleteMark {
            collection_key,
            index,
        } => {
            let collection = mark_collection_mut(sequence, &collection_key)?;
            if (index as usize) < collection.marks.len() {
                collection.marks.remove(index as usize);
            }
        }
    }
    Ok(())
}

fn script_reference(
    project: &DawnProject,
    script: crate::dto::EffectScriptReferenceDto,
) -> Result<ResolvedSymbolRef<EffectDefinitionKey>, String> {
    let key = project
        .stores
        .effect_definitions
        .keys()
        .find(|key| key.path.to_slash_string() == script.path && key.name == script.effect_name)
        .cloned()
        .ok_or_else(|| format!("effect `{}` was not found", script.effect_name))?;
    Ok(ResolvedSymbolRef {
        key,
        reference: SymbolRef::new(script.effect_name)?,
    })
}

fn layout_target_to_effect_target(
    target: crate::dto::LayoutTargetDto,
) -> Result<EffectTarget<dawn_project::Resolved>, String> {
    let id = target
        .name
        .parse::<u32>()
        .map_err(|_| format!("target id `{}` is not numeric", target.name))?;
    Ok(match target.kind {
        crate::dto::LayoutTargetKindDto::Group => EffectTarget::Group {
            id: dawn_project::GroupInstantiationId(id),
        },
        crate::dto::LayoutTargetKindDto::Fixture => EffectTarget::Fixture { id: FixtureId(id) },
    })
}

fn effect_param_from_dto(
    value: crate::dto::SequenceEffectParamValueDto,
) -> Result<EffectParam<dawn_project::Resolved>, String> {
    Ok(match value {
        crate::dto::SequenceEffectParamValueDto::Int { value } => EffectParam::Integer {
            value: u64::from(value),
        },
        crate::dto::SequenceEffectParamValueDto::Float { value } => EffectParam::Float { value },
        crate::dto::SequenceEffectParamValueDto::Bool { value } => EffectParam::Boolean { value },
        crate::dto::SequenceEffectParamValueDto::Color { value } => EffectParam::Color {
            value: dawn_project::Color::parse(&value)?,
        },
        crate::dto::SequenceEffectParamValueDto::Enum { value } => EffectParam::Enum { value },
        crate::dto::SequenceEffectParamValueDto::Flags { value } => EffectParam::Flags {
            value: Flags { values: value },
        },
        crate::dto::SequenceEffectParamValueDto::FloatCurve { points } => EffectParam::Curve {
            curve: CurveUse {
                id: 0,
                curve: ResolvedInlineOrRef::Inline(Curve {
                    value_type: CurveValueType::Float,
                    points: points
                        .into_iter()
                        .map(|point| CurvePoint {
                            time: point.time,
                            value: CurveValue::Float(point.value),
                        })
                        .collect(),
                }),
            },
        },
        crate::dto::SequenceEffectParamValueDto::ColorCurve { points } => EffectParam::Curve {
            curve: CurveUse {
                id: 0,
                curve: ResolvedInlineOrRef::Inline(Curve {
                    value_type: CurveValueType::Color,
                    points: points
                        .into_iter()
                        .map(|point| {
                            Ok(CurvePoint {
                                time: point.time,
                                value: CurveValue::Color(dawn_project::Color::parse(&point.value)?),
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                }),
            },
        },
        crate::dto::SequenceEffectParamValueDto::IntArray { values } => EffectParam::Array {
            element_type: dawn_project::ArrayElementType::Int,
            values: values
                .into_iter()
                .map(|value| EffectParamArrayValue::Integer(u64::from(value)))
                .collect(),
        },
        crate::dto::SequenceEffectParamValueDto::FloatArray { values } => EffectParam::Array {
            element_type: dawn_project::ArrayElementType::Float,
            values: values
                .into_iter()
                .map(EffectParamArrayValue::Float)
                .collect(),
        },
        crate::dto::SequenceEffectParamValueDto::BoolArray { values } => EffectParam::Array {
            element_type: dawn_project::ArrayElementType::Bool,
            values: values
                .into_iter()
                .map(EffectParamArrayValue::Boolean)
                .collect(),
        },
        crate::dto::SequenceEffectParamValueDto::ColorArray { values } => EffectParam::Array {
            element_type: dawn_project::ArrayElementType::Color,
            values: values
                .into_iter()
                .map(|value| dawn_project::Color::parse(&value).map(EffectParamArrayValue::Color))
                .collect::<Result<Vec<_>, String>>()?,
        },
        crate::dto::SequenceEffectParamValueDto::FloatCurveArray { values: _ }
        | crate::dto::SequenceEffectParamValueDto::ColorCurveArray { values: _ } => {
            return Err("curve arrays cannot be edited in this milestone".to_string())
        }
        crate::dto::SequenceEffectParamValueDto::Marks { key } => EffectParam::Marks { key },
    })
}

fn effect_mut(
    sequence: &mut Sequence<dawn_project::Resolved>,
    id: u32,
) -> Result<&mut SequenceEffect<dawn_project::Resolved>, String> {
    sequence
        .effects
        .iter_mut()
        .find(|effect| effect.id.0 == id)
        .ok_or_else(|| format!("effect `{id}` was not found"))
}

fn mark_collection_mut<'a>(
    sequence: &'a mut Sequence<dawn_project::Resolved>,
    key: &str,
) -> Result<&'a mut SequenceMarkCollection, String> {
    sequence
        .mark_collections
        .iter_mut()
        .find(|collection| collection.key == key)
        .ok_or_else(|| format!("mark collection `{key}` was not found"))
}

fn delete_selection(
    sequence: &mut Sequence<dawn_project::Resolved>,
    selection: &SequenceSelectionDto,
) {
    match selection {
        SequenceSelectionDto::Effects { ids } => {
            sequence
                .effects
                .retain(|effect| !ids.contains(&effect.id.0));
        }
        SequenceSelectionDto::Marks { marks } => {
            let mut grouped: HashMap<&str, Vec<usize>> = HashMap::new();
            for mark in marks {
                grouped
                    .entry(mark.collection_key.as_str())
                    .or_default()
                    .push(mark.index as usize);
            }
            for collection in &mut sequence.mark_collections {
                if let Some(indices) = grouped.get_mut(collection.key.as_str()) {
                    indices.sort_unstable_by(|left, right| right.cmp(left));
                    for index in indices {
                        if *index < collection.marks.len() {
                            collection.marks.remove(*index);
                        }
                    }
                }
            }
        }
    }
}

fn paste_clipboard(
    sequence: &mut Sequence<dawn_project::Resolved>,
    clipboard: Option<SequenceClipboard>,
    anchor: SequencePasteAnchorDto,
) -> Result<SequenceSelectionDto, String> {
    match clipboard {
        Some(SequenceClipboard::Effects(mut effects)) => {
            let first_id = sequence
                .effects
                .iter()
                .map(|effect| effect.id.0)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            let anchor_time = anchor.time_seconds.unwrap_or(0.0);
            let ids = effects
                .iter_mut()
                .enumerate()
                .map(|(offset, effect)| {
                    let id = first_id.saturating_add(offset as u32);
                    effect.id = SequenceEffectId(id);
                    effect.start = time_from_seconds(anchor_time)?;
                    Ok(id)
                })
                .collect::<Result<Vec<_>, String>>()?;
            sequence.effects.extend(effects);
            Ok(SequenceSelectionDto::Effects { ids })
        }
        Some(SequenceClipboard::Marks(marks)) => {
            let mut refs = Vec::new();
            let anchor_time = anchor.time_seconds.unwrap_or(0.0);
            for mark in marks {
                if let Some(collection) = sequence
                    .mark_collections
                    .iter_mut()
                    .find(|collection| collection.key == mark.collection_key)
                {
                    let index = collection.marks.len();
                    collection
                        .marks
                        .push(time_from_seconds(anchor_time + mark.time_seconds)?);
                    refs.push(SequenceMarkRefDto {
                        collection_key: mark.collection_key,
                        index: index as u32,
                    });
                }
            }
            Ok(SequenceSelectionDto::Marks { marks: refs })
        }
        None => Err("sequence clipboard is empty".to_string()),
    }
}

fn move_effects(
    sequence: &mut Sequence<dawn_project::Resolved>,
    document: &SequenceDocument,
    ids: &[u32],
    time_delta_seconds: f64,
    lane_delta: i32,
) -> Result<(), String> {
    for id in ids {
        let effect = effect_mut(sequence, *id)?;
        let current_lane = document
            .lanes
            .iter()
            .position(|lane| lane.target.name == target_name(&effect.target))
            .unwrap_or(0);
        let lane_index = (current_lane as i32 + lane_delta)
            .clamp(0, document.lanes.len().saturating_sub(1) as i32)
            as usize;
        effect.start = time_from_seconds(
            (effect.start.as_seconds_f64() + time_delta_seconds).clamp(0.0, f64::MAX),
        )?;
        if let Some(lane) = document.lanes.get(lane_index) {
            effect.target = layout_target_to_effect_target(lane.target.clone().into())?;
        }
    }
    Ok(())
}

fn resize_effects(
    sequence: &mut Sequence<dawn_project::Resolved>,
    ids: &[u32],
    edge: SequenceResizeEdgeDto,
    time_delta_seconds: f64,
) -> Result<(), String> {
    for id in ids {
        let effect = effect_mut(sequence, *id)?;
        match edge {
            SequenceResizeEdgeDto::Left => {
                let end_seconds = effect.start.as_seconds_f64() + effect.duration.as_seconds_f64();
                let start_seconds = (effect.start.as_seconds_f64() + time_delta_seconds)
                    .clamp(0.0, end_seconds - MIN_EFFECT_DURATION_SECONDS);
                effect.start = time_from_seconds(start_seconds)?;
                effect.duration = duration_from_seconds(end_seconds - start_seconds)?;
            }
            SequenceResizeEdgeDto::Right => {
                effect.duration = duration_from_seconds(
                    (effect.duration.as_seconds_f64() + time_delta_seconds)
                        .max(MIN_EFFECT_DURATION_SECONDS),
                )?;
            }
        }
    }
    Ok(())
}

fn move_marks(
    sequence: &mut Sequence<dawn_project::Resolved>,
    marks: &[SequenceMarkRefDto],
    time_delta_seconds: f64,
) -> Result<(), String> {
    for mark in marks {
        let collection = mark_collection_mut(sequence, &mark.collection_key)?;
        let time = collection
            .marks
            .get_mut(mark.index as usize)
            .ok_or_else(|| format!("mark `{}` was not found", mark.index))?;
        *time = time_from_seconds((time.as_seconds_f64() + time_delta_seconds).max(0.0))?;
    }
    Ok(())
}

fn target_name(target: &EffectTarget<dawn_project::Resolved>) -> String {
    match target {
        EffectTarget::Group { id } => id.to_string(),
        EffectTarget::Fixture { id } => id.to_string(),
    }
}

fn time_from_seconds(seconds: f64) -> Result<Time, String> {
    Time::try_from_seconds_f64_rounded(seconds).map_err(str::to_string)
}

fn duration_from_seconds(seconds: f64) -> Result<TimeSpan, String> {
    TimeSpan::try_from_seconds_f64_rounded(seconds).map_err(str::to_string)
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

fn buffer_matches_any_path(path: &Utf8PathBuf, changed_paths: &[Utf8PathBuf]) -> bool {
    changed_paths
        .iter()
        .any(|changed_path| path == changed_path || path.starts_with(changed_path))
}
