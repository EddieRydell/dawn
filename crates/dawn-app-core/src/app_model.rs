use dawn_project::analysis::{ProjectAnalysis, ProjectDiagnostic};
use dawn_project::document::{
    DocumentDescriptor, DocumentViewId, FixtureDocument, LayoutDocument, SequenceAuthoredEditInput,
    SequenceDocument, SequenceDocumentEdit, SequenceEffectMoveDocumentEdit,
    SequenceEffectResizeDocumentEdit, SequenceMarkMoveDocumentEdit, SequenceMarkPasteDocumentEdit,
    SequenceMarkRefDocumentEdit,
};
use dawn_project::fs::{WorkspaceEntry, WorkspaceFs};
use dawn_project::model::{Authored, DawnImport, DawnObject, Geometry, Sequence, SequenceEffect};
use dawn_project::parse::parse_dawn_file_with_source_map;
use dawn_project::path::Utf8PathBuf;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use crate::actions::AppAction;
use crate::dto::AppSnapshotDto;
use crate::dto::{
    FixtureGuiEditDto, LayoutGuiEditDto, SequenceGuiEditDto, SequenceMarkRefDto,
    SequencePasteAnchorDto, SequenceResizeEdgeDto, SequenceSelectionDto, SequenceSelectionEditDto,
    SequenceSelectionEditResultDto,
};
use crate::editor_session::{BufferExternalState, EditorBuffer, EditorSession, EditorViewMode};
use crate::layout_persistence::{
    load_workbench_layout, save_workbench_layout, WindowLayout, WorkbenchLayout,
};
use crate::preview_session::{
    PreviewRenderRequest, PreviewRenderResult, PreviewSession, PreviewSnapshot, PreviewSyncMode,
    SequenceKey,
};
use crate::workspace::WorkspaceService;

const MIN_EFFECT_DURATION_SECONDS: f64 = 0.000000001;
const SEQUENCE_AUTOSAVE_DEBOUNCE: Duration = Duration::from_millis(250);

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

fn sequence_gui_edit_to_document_edit(edit: SequenceGuiEditDto) -> SequenceDocumentEdit {
    match edit {
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
        SequenceGuiEditDto::RetargetEffect { id, target } => SequenceDocumentEdit::RetargetEffect {
            id,
            target: target.into(),
        },
        SequenceGuiEditDto::SetEffectScope { id, scope } => SequenceDocumentEdit::SetEffectScope {
            id,
            scope: scope.into(),
        },
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
    pub analysis: Option<ProjectAnalysis>,
    pub diagnostics: Vec<ProjectDiagnostic>,
    pub status: String,
    pub sequence_clipboard: Option<SequenceClipboard>,
    active_sequence_gui_document: Option<CachedActiveGuiDocument>,
    sequence_gui_sessions: HashMap<SequenceKey, SequenceGuiSession>,
    sequence_save_worker: SequenceSaveWorker,
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
    Effects(Vec<SequenceEffect<Authored>>),
    Marks(Vec<SequenceMarkPasteDocumentEdit>),
}

#[derive(Debug, Clone)]
struct CachedActiveGuiDocument {
    path: Utf8PathBuf,
    object_key: String,
    view_mode: EditorViewMode,
    document: SequenceDocument,
}

#[derive(Debug, Clone)]
struct SequenceGuiSession {
    path: Utf8PathBuf,
    sequence: Sequence<Authored>,
    document: SequenceDocument,
    base_content: String,
    imports: Vec<DawnImport>,
    pending_imports: Vec<DawnImport>,
    edit_revision: u64,
    saved_revision: u64,
    undo_stack: Vec<SequenceGuiSnapshot>,
    redo_stack: Vec<SequenceGuiSnapshot>,
}

#[derive(Debug, Clone)]
struct SequenceGuiSnapshot {
    sequence: Sequence<Authored>,
    document: SequenceDocument,
    imports: Vec<DawnImport>,
    pending_imports: Vec<DawnImport>,
}

#[derive(Debug)]
struct SequenceSaveWorker {
    requests: Sender<SequenceSaveWorkerRequest>,
    completions: Receiver<SequenceSaveCompletion>,
}

#[derive(Debug)]
enum SequenceSaveWorkerRequest {
    Save(Box<SequenceSaveRequest>),
    Flush(Sender<()>),
}

#[derive(Debug, Clone)]
struct SequenceSaveRequest {
    fs: WorkspaceFs,
    key: SequenceKey,
    revision: u64,
    sequence: Sequence<Authored>,
    base_content: String,
    imports_to_add: Vec<DawnImport>,
    analysis: ProjectAnalysis,
}

#[derive(Debug)]
struct SequenceSaveCompletion {
    key: SequenceKey,
    revision: u64,
    result: Result<String, String>,
}

#[derive(Debug, Clone)]
pub struct AppSnapshot {
    pub project_root: Option<String>,
    pub project_entries: Vec<WorkspaceEntry>,
    pub analysis: Option<ProjectAnalysis>,
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

impl SequenceSaveWorker {
    fn new() -> Self {
        let (request_tx, request_rx) = mpsc::channel();
        let (completion_tx, completion_rx) = mpsc::channel();
        thread::spawn(move || sequence_save_worker_loop(request_rx, completion_tx));
        Self {
            requests: request_tx,
            completions: completion_rx,
        }
    }

    fn queue(&self, request: SequenceSaveRequest) -> Result<(), String> {
        self.requests
            .send(SequenceSaveWorkerRequest::Save(Box::new(request)))
            .map_err(|_| "sequence autosave worker is not available".to_string())
    }

    fn flush(&self) -> Result<(), String> {
        let (done_tx, done_rx) = mpsc::channel();
        self.requests
            .send(SequenceSaveWorkerRequest::Flush(done_tx))
            .map_err(|_| "sequence autosave worker is not available".to_string())?;
        done_rx
            .recv()
            .map_err(|_| "sequence autosave worker did not flush".to_string())
    }
}

fn sequence_save_worker_loop(
    requests: Receiver<SequenceSaveWorkerRequest>,
    completions: Sender<SequenceSaveCompletion>,
) {
    let mut pending = HashMap::new();
    while let Ok(request) = requests.recv() {
        match request {
            SequenceSaveWorkerRequest::Save(request) => {
                let request = *request;
                pending.insert(request.key.clone(), request);
                loop {
                    match requests.recv_timeout(SEQUENCE_AUTOSAVE_DEBOUNCE) {
                        Ok(SequenceSaveWorkerRequest::Save(request)) => {
                            let request = *request;
                            pending.insert(request.key.clone(), request);
                        }
                        Ok(SequenceSaveWorkerRequest::Flush(done)) => {
                            process_sequence_save_requests(&mut pending, &completions);
                            let _ = done.send(());
                            break;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            process_sequence_save_requests(&mut pending, &completions);
                            break;
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            process_sequence_save_requests(&mut pending, &completions);
                            return;
                        }
                    }
                }
            }
            SequenceSaveWorkerRequest::Flush(done) => {
                process_sequence_save_requests(&mut pending, &completions);
                let _ = done.send(());
            }
        }
    }
}

fn process_sequence_save_requests(
    pending: &mut HashMap<SequenceKey, SequenceSaveRequest>,
    completions: &Sender<SequenceSaveCompletion>,
) {
    let requests = std::mem::take(pending);
    for request in requests.into_values() {
        let result = dawn_project::document::serialize_sequence_document(
            &request.fs,
            request.key.path.clone(),
            &request.key.object_key,
            request.sequence,
            request.base_content,
            request.imports_to_add,
            &request.analysis,
        )
        .and_then(|outcome| {
            request
                .fs
                .write(&request.key.path, outcome.serialized_content.as_bytes())
                .map_err(|error| error.to_string())?;
            Ok(outcome.serialized_content)
        });
        let _ = completions.send(SequenceSaveCompletion {
            key: request.key,
            revision: request.revision,
            result,
        });
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
            analysis: None,
            diagnostics: Vec::new(),
            status: "No project open".to_string(),
            sequence_clipboard: None,
            active_sequence_gui_document: None,
            sequence_gui_sessions: HashMap::new(),
            sequence_save_worker: SequenceSaveWorker::new(),
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
            analysis: self.analysis.clone(),
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
        self.drain_sequence_save_completions()?;
        match action {
            AppAction::OpenProject(path) => {
                self.flush_autosave()?;
                self.open_project(path, true, false)?;
                self.status = "Project opened".to_string();
            }
            AppAction::ReloadProject => {
                self.flush_pending_sequence_saves()?;
                let paths = self
                    .editors
                    .buffers()
                    .into_iter()
                    .map(|buffer| buffer.path)
                    .collect();
                self.reconcile_filesystem_changes(paths)?;
                self.status = "Project checked".to_string();
            }
            AppAction::OpenFile(path) => {
                let (text, disk_version) = self.workspace.read_file_with_version(path.clone())?;
                self.editors.open_file(path, text, disk_version);
                self.refresh_analysis()?;
                self.sync_preview_source(PreviewSyncMode::RenderNow);
                self.persist_workbench_layout()?;
            }
            AppAction::CloseFile(path) => {
                self.flush_pending_sequence_saves_for_path(&path)?;
                self.editors.close_file(&path);
                self.sequence_gui_sessions.retain(|key, _| key.path != path);
                self.refresh_analysis()?;
                self.sync_preview_source(PreviewSyncMode::RenderNow);
                self.persist_workbench_layout()?;
            }
            AppAction::SetActiveFile(path) => {
                let active_changed = self.editors.active_file() != Some(&path);
                self.editors.set_active_file(path);
                if active_changed {
                    self.invalidate_active_gui_document_cache();
                    self.preview.pause(self.analysis.as_ref());
                    self.sync_preview_source(PreviewSyncMode::RenderNow);
                    self.persist_workbench_layout()?;
                }
            }
            AppAction::SetActiveViewMode(mode) => {
                let Some(path) = self.editors.active_file().cloned() else {
                    return Ok(DispatchOutcome::NoSnapshotChange);
                };
                let mode = mode.into();
                if mode == EditorViewMode::Text {
                    self.flush_pending_sequence_saves_for_path(&path)?;
                }
                self.editors.set_view_mode(&path, mode);
                self.invalidate_active_gui_document_cache();
                self.sync_preview_source(PreviewSyncMode::RenderNow);
                self.persist_workbench_layout()?;
            }
            AppAction::UpdateActiveText(text) => {
                let path = self.active_path_for_gui_edit()?;
                self.flush_pending_sequence_saves_for_path(&path)?;
                self.ensure_active_buffer_not_conflicted()?;
                self.editors.update_active_text(text);
                self.sequence_gui_sessions.retain(|key, _| key.path != path);
                self.refresh_analysis_after_memory_edit();
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
                self.status = "Edited".to_string();
            }
            AppAction::ApplyLayoutGuiEdit(edit) => {
                self.apply_layout_gui_edit(edit)?;
                self.flush_autosave()?;
                self.status = "Autosaved".to_string();
            }
            AppAction::ApplyFixtureGuiEdit(edit) => {
                self.apply_fixture_gui_edit(edit)?;
                self.flush_autosave()?;
                self.status = "Autosaved".to_string();
            }
            AppAction::FlushAutosave => {
                self.flush_autosave()?;
                self.status = "Saved".to_string();
            }
            AppAction::FilesystemChanged(paths) => {
                self.flush_pending_sequence_saves()?;
                self.reconcile_filesystem_changes(paths)?;
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
                self.refresh_analysis()?;
                self.sync_preview_source(PreviewSyncMode::RenderNow);
                self.persist_workbench_layout()?;
            }
            AppAction::CreateDirectory { parent, name } => {
                self.flush_autosave()?;
                self.workspace.create_directory(parent, &name)?;
                self.refresh_project_entries()?;
                self.refresh_analysis()?;
                self.sync_preview_source(PreviewSyncMode::RenderNow);
            }
            AppAction::RenamePath { path, new_name } => {
                self.flush_autosave()?;
                let moves = self.workspace.rename_path(path.clone(), &new_name)?;
                self.refresh_project_entries()?;
                self.editors.reconcile_moved_paths(&moves);
                self.refresh_analysis()?;
                self.sync_preview_source(PreviewSyncMode::RenderNow);
                self.persist_workbench_layout()?;
            }
            AppAction::DeletePath(path) => {
                self.flush_autosave()?;
                self.workspace.delete_path(path.clone())?;
                self.refresh_project_entries()?;
                self.editors.reconcile_deleted_path(&path);
                self.refresh_analysis()?;
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
                    self.preview.clear_effect_preview(self.analysis.as_ref());
                } else {
                    self.preview.render_current_frame(self.analysis.as_ref());
                }
            }
            AppAction::SetEffectPreviewEffects(ids) => {
                let ids = if self.workbench_layout.effect_preview_enabled {
                    ids
                } else {
                    Vec::new()
                };
                self.preview
                    .set_effect_preview_ids(ids, self.analysis.as_ref());
            }
            AppAction::PreviewPlay => {
                if self.workbench_layout.effect_preview_enabled {
                    self.workbench_layout.effect_preview_enabled = false;
                    save_workbench_layout(&self.workbench_layout)?;
                    self.preview.clear_effect_preview(self.analysis.as_ref());
                }
                self.preview.play(self.analysis.as_ref());
                self.status = "Preview playing".to_string();
            }
            AppAction::PreviewPause => {
                self.preview.pause(self.analysis.as_ref());
                self.status = "Preview paused".to_string();
            }
            AppAction::PreviewStop => {
                self.preview.stop(self.analysis.as_ref());
                self.status = "Preview stopped".to_string();
            }
            AppAction::PreviewRewindToZero => {
                self.preview
                    .go_to_sequence_beginning(self.analysis.as_ref());
                self.status = "Preview rewound".to_string();
            }
            AppAction::PreviewSeek(position_seconds) => {
                self.preview.seek(position_seconds, self.analysis.as_ref());
                self.status = "Preview seeked".to_string();
            }
        }
        self.drain_sequence_save_completions()?;
        Ok(DispatchOutcome::SnapshotChanged)
    }

    pub fn tick_preview(&mut self) {
        self.preview.tick(self.analysis.as_ref());
    }

    pub fn tick_preview_clock(&mut self) {
        self.preview.tick_clock();
    }

    pub fn render_preview_frame(&mut self) {
        self.preview.render_current_frame(self.analysis.as_ref());
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
        self.refresh_analysis()?;
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

    pub fn refresh_analysis(&mut self) -> Result<(), String> {
        self.invalidate_active_gui_document_cache();
        let overlays = self.editors.dirty_overlays();
        let analysis = self.workspace.analyze(overlays)?;
        self.diagnostics = analysis.diagnostics.clone();
        self.analysis = Some(analysis);
        Ok(())
    }

    fn refresh_analysis_after_memory_edit(&mut self) {
        match self.refresh_analysis() {
            Ok(()) => {
                self.sync_preview_source(PreviewSyncMode::RenderNow);
            }
            Err(error) => {
                self.status = error;
                self.sync_preview_source(PreviewSyncMode::RenderNow);
            }
        }
    }

    pub fn flush_autosave(&mut self) -> Result<(), String> {
        self.flush_autosave_with_preview_sync(true)
    }

    fn flush_autosave_with_preview_sync(&mut self, sync_preview: bool) -> Result<(), String> {
        self.flush_pending_sequence_saves()?;
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
            self.refresh_analysis()?;
            if sync_preview {
                self.sync_preview_source(PreviewSyncMode::RenderNow);
            }
        }
        Ok(())
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
        self.refresh_analysis()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        self.persist_workbench_layout()?;
        Ok(())
    }

    fn reload_active_buffer_from_disk(&mut self) -> Result<(), String> {
        let Some(buffer) = self.editors.active_buffer().cloned() else {
            return Ok(());
        };
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
        self.refresh_analysis()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    fn keep_active_buffer(&mut self) -> Result<(), String> {
        let Some(buffer) = self.editors.active_buffer().cloned() else {
            return Ok(());
        };
        let version = self
            .workspace
            .write_text_file_with_version(buffer.path.clone(), &buffer.text)?;
        self.editors.record_saved_version(&buffer.path, version);
        self.refresh_project_entries()?;
        self.refresh_analysis()?;
        self.sync_preview_source(PreviewSyncMode::RenderNow);
        Ok(())
    }

    fn sync_preview_source(&mut self, mode: PreviewSyncMode) {
        let source = self.active_sequence_source();
        self.cache_active_sequence_source(source.as_ref());
        self.preview
            .sync_source(source, self.analysis.as_ref(), mode);
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
        self.cache_active_sequence_source(source.as_ref());
        self.preview
            .sync_source(source, self.analysis.as_ref(), mode);
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
        self.workspace
            .inspect_document(path, self.editors.dirty_overlays())
            .ok()
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
            .filter(|diagnostic| diagnostic.path == buffer.path)
            .cloned()
            .collect::<Vec<_>>();
        let Some(descriptor) = descriptor else {
            return Some(ActiveGuiDocument::Blocked {
                reason: "Text could not be parsed as a Dawn document.".to_string(),
                diagnostics,
            });
        };
        let overlays = self.editors.dirty_overlays();
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
                    .sequence_document(buffer.path.clone(), object_key, overlays)
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
                    .layout_document(buffer.path.clone(), object_key, overlays)
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
                    .fixture_document(buffer.path.clone(), None, overlays)
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
        let Some(buffer) = self.editors.active_buffer() else {
            return Err("sequence preview request does not match active sequence".to_string());
        };
        let path_matches =
            cached.path == *path || cached.document.path == path.as_str() || buffer.path == *path;
        if buffer.path == cached.path
            && buffer.view_mode == cached.view_mode
            && cached.view_mode == EditorViewMode::Gui
            && cached.object_key == object_key
            && path_matches
        {
            Ok(cached.document.clone())
        } else {
            Err("sequence preview request does not match active sequence".to_string())
        }
    }

    fn apply_sequence_gui_edit(&mut self, edit: SequenceGuiEditDto) -> Result<(), String> {
        self.ensure_active_buffer_not_conflicted()?;
        let path = self.active_path_for_gui_edit()?;
        let descriptor_overlays = self.editors.dirty_overlays();
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), descriptor_overlays)?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
            .ok_or_else(|| "active document is not a sequence".to_string())?
            .clone();
        self.apply_sequence_document_edit_for_key(
            SequenceKey { path, object_key },
            sequence_gui_edit_to_document_edit(edit),
        )
    }

    pub fn apply_sequence_selection_edit(
        &mut self,
        edit: SequenceSelectionEditDto,
    ) -> Result<SequenceSelectionEditResultDto, String> {
        let before = self.active_sequence_authored()?;
        let before_document = self.active_sequence_document()?;
        let resulting_selection;
        let mut copied_count = 0;
        let mut skipped_count = 0;

        let document_edit = match edit {
            SequenceSelectionEditDto::Copy { selection } => {
                copied_count =
                    self.copy_sequence_selection(&before, &before_document, &selection)?;
                self.status = format!("Copied {copied_count}");
                return Ok(SequenceSelectionEditResultDto {
                    snapshot: self.snapshot_dto(),
                    selection: Some(selection),
                    copied_count,
                    skipped_count,
                });
            }
            SequenceSelectionEditDto::Cut { selection } => {
                copied_count =
                    self.copy_sequence_selection(&before, &before_document, &selection)?;
                let edit = sequence_delete_edit(selection.clone());
                self.status = format!("Cut {copied_count}");
                resulting_selection = Some(selection_empty_like(&selection));
                edit
            }
            SequenceSelectionEditDto::Delete { selection } => {
                let edit = sequence_delete_edit(selection.clone());
                self.status = "Deleted selection".to_string();
                resulting_selection = Some(selection_empty_like(&selection));
                edit
            }
            SequenceSelectionEditDto::Paste { anchor } => {
                let (edit, selection, skipped) =
                    self.sequence_paste_edit(&before_document, anchor)?;
                skipped_count = skipped;
                copied_count = selection_count(&selection);
                self.status = if skipped_count == 0 {
                    format!("Pasted {copied_count}")
                } else {
                    format!("Pasted {copied_count}, skipped {skipped_count}")
                };
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
        let snapshot = self.snapshot_dto();
        Ok(SequenceSelectionEditResultDto {
            snapshot,
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
            .inspect_document(path.clone(), self.editors.dirty_overlays())?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
            .ok_or_else(|| "active document is not a sequence".to_string())?
            .clone();
        self.apply_sequence_document_edit_for_key(SequenceKey { path, object_key }, edit)
    }

    fn apply_sequence_document_edit_for_key(
        &mut self,
        key: SequenceKey,
        edit: SequenceDocumentEdit,
    ) -> Result<(), String> {
        let analysis = self
            .analysis
            .as_ref()
            .ok_or_else(|| "project analysis is not available".to_string())?
            .clone();
        self.ensure_sequence_gui_session(&key)?;
        let session = self
            .sequence_gui_sessions
            .get(&key)
            .ok_or_else(|| "sequence GUI session is not available".to_string())?
            .clone();
        let outcome = self.workspace.apply_sequence_edit_to_sequence(
            SequenceAuthoredEditInput {
                path: key.path.clone(),
                object_key: key.object_key.clone(),
                sequence: session.sequence.clone(),
                imports: session.imports.clone(),
                edit,
                base_content: session.base_content.clone(),
                overlays: self.editors.dirty_overlays(),
            },
            &analysis,
        )?;
        let next_revision = session.edit_revision.saturating_add(1);
        let next_snapshot = SequenceGuiSnapshot {
            sequence: outcome.sequence.clone(),
            document: outcome.refreshed_document.clone(),
            imports: {
                let mut imports = session.imports.clone();
                if let Some(import) = outcome.import_to_add.clone() {
                    imports.push(import);
                }
                imports
            },
            pending_imports: {
                let mut imports = session.pending_imports.clone();
                if let Some(import) = outcome.import_to_add.clone() {
                    imports.push(import);
                }
                imports
            },
        };
        let session = self
            .sequence_gui_sessions
            .get_mut(&key)
            .ok_or_else(|| "sequence GUI session is not available".to_string())?;
        session.undo_stack.push(SequenceGuiSnapshot {
            sequence: session.sequence.clone(),
            document: session.document.clone(),
            imports: session.imports.clone(),
            pending_imports: session.pending_imports.clone(),
        });
        session.redo_stack.clear();
        session.sequence = next_snapshot.sequence;
        session.document = next_snapshot.document.clone();
        session.imports = next_snapshot.imports;
        session.pending_imports = next_snapshot.pending_imports;
        session.edit_revision = next_revision;
        self.editors.mark_gui_edit_dirty(&key.path, next_revision);
        self.queue_sequence_session_save(&key)?;
        self.sync_preview_source_from_document(
            key.path.clone(),
            next_snapshot.document,
            PreviewSyncMode::DeferRender,
        );
        Ok(())
    }

    fn ensure_sequence_gui_session(&mut self, key: &SequenceKey) -> Result<(), String> {
        if self.sequence_gui_sessions.contains_key(key) {
            return Ok(());
        }
        let base_content = self.active_buffer_text()?;
        let parsed =
            parse_dawn_file_with_source_map(&base_content).map_err(|error| error.to_string())?;
        let sequence = match parsed.file.get(&key.object_key) {
            Some(DawnObject::Sequence(sequence)) => sequence.clone(),
            _ => {
                return Err(format!(
                    "sequence object `{}` was not found",
                    key.object_key
                ))
            }
        };
        let document = if let Some(document) =
            self.cached_active_sequence_document(&key.path, &key.object_key, EditorViewMode::Gui)
        {
            document
        } else {
            self.workspace.sequence_document(
                key.path.clone(),
                &key.object_key,
                self.editors.dirty_overlays(),
            )?
        };
        self.sequence_gui_sessions.insert(
            key.clone(),
            SequenceGuiSession {
                path: key.path.clone(),
                sequence,
                document,
                base_content,
                imports: parsed.file.imports,
                pending_imports: Vec::new(),
                edit_revision: 0,
                saved_revision: 0,
                undo_stack: Vec::new(),
                redo_stack: Vec::new(),
            },
        );
        Ok(())
    }

    fn queue_sequence_session_save(&self, key: &SequenceKey) -> Result<(), String> {
        let session = self
            .sequence_gui_sessions
            .get(key)
            .ok_or_else(|| "sequence GUI session is not available".to_string())?;
        let analysis = self
            .analysis
            .as_ref()
            .ok_or_else(|| "project analysis is not available".to_string())?
            .clone();
        self.sequence_save_worker.queue(SequenceSaveRequest {
            fs: self.workspace.project_fs_clone()?,
            key: key.clone(),
            revision: session.edit_revision,
            sequence: session.sequence.clone(),
            base_content: session.base_content.clone(),
            imports_to_add: session.pending_imports.clone(),
            analysis,
        })
    }

    fn drain_sequence_save_completions(&mut self) -> Result<(), String> {
        let mut changed = false;
        while let Ok(completion) = self.sequence_save_worker.completions.try_recv() {
            let current_revision = self
                .sequence_gui_sessions
                .get(&completion.key)
                .map(|session| session.edit_revision);
            if current_revision != Some(completion.revision) {
                continue;
            }
            match completion.result {
                Ok(text) => {
                    let version = self
                        .workspace
                        .file_version(&completion.key.path, &text)?
                        .ok_or_else(|| "saved sequence file does not exist".to_string())?;
                    if self.editors.complete_gui_edit_save(
                        &completion.key.path,
                        completion.revision,
                        text.clone(),
                        version,
                    ) {
                        if let Some(session) = self.sequence_gui_sessions.get_mut(&completion.key) {
                            if session.edit_revision == completion.revision {
                                session.base_content = text;
                                session.pending_imports.clear();
                                session.saved_revision = completion.revision;
                            }
                        }
                        changed = true;
                    }
                }
                Err(error) => return Err(error),
            }
        }
        if changed {
            self.refresh_analysis()?;
        }
        Ok(())
    }

    fn flush_pending_sequence_saves(&mut self) -> Result<(), String> {
        self.sequence_save_worker.flush()?;
        self.drain_sequence_save_completions()
    }

    fn flush_pending_sequence_saves_for_path(&mut self, path: &Utf8PathBuf) -> Result<(), String> {
        if self
            .sequence_gui_sessions
            .values()
            .any(|session| &session.path == path && session.edit_revision != session.saved_revision)
        {
            self.flush_pending_sequence_saves()?;
        } else {
            self.drain_sequence_save_completions()?;
        }
        Ok(())
    }

    fn undo_active_edit(&mut self) -> Result<bool, String> {
        if self
            .editors
            .active_buffer()
            .is_some_and(|buffer| buffer.view_mode == EditorViewMode::Gui)
        {
            return self.undo_active_sequence_gui_edit();
        }
        let path = self.active_path_for_gui_edit()?;
        self.flush_pending_sequence_saves_for_path(&path)?;
        if self.editors.undo_active_text_edit() {
            self.refresh_analysis_after_memory_edit();
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
            return self.redo_active_sequence_gui_edit();
        }
        let path = self.active_path_for_gui_edit()?;
        self.flush_pending_sequence_saves_for_path(&path)?;
        if self.editors.redo_active_text_edit() {
            self.refresh_analysis_after_memory_edit();
            self.status = "Redo".to_string();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn undo_active_sequence_gui_edit(&mut self) -> Result<bool, String> {
        let key = self.active_sequence_key()?;
        self.ensure_sequence_gui_session(&key)?;
        let Some(session) = self.sequence_gui_sessions.get_mut(&key) else {
            return Ok(false);
        };
        let Some(previous) = session.undo_stack.pop() else {
            return Ok(false);
        };
        session.redo_stack.push(SequenceGuiSnapshot {
            sequence: session.sequence.clone(),
            document: session.document.clone(),
            imports: session.imports.clone(),
            pending_imports: session.pending_imports.clone(),
        });
        session.sequence = previous.sequence;
        session.document = previous.document.clone();
        session.imports = previous.imports;
        session.pending_imports = previous.pending_imports;
        session.edit_revision = session.edit_revision.saturating_add(1);
        let revision = session.edit_revision;
        self.editors.mark_gui_edit_dirty(&key.path, revision);
        self.queue_sequence_session_save(&key)?;
        self.sync_preview_source_from_document(
            key.path,
            previous.document,
            PreviewSyncMode::DeferRender,
        );
        self.status = "Undo".to_string();
        Ok(true)
    }

    fn redo_active_sequence_gui_edit(&mut self) -> Result<bool, String> {
        let key = self.active_sequence_key()?;
        self.ensure_sequence_gui_session(&key)?;
        let Some(session) = self.sequence_gui_sessions.get_mut(&key) else {
            return Ok(false);
        };
        let Some(next) = session.redo_stack.pop() else {
            return Ok(false);
        };
        session.undo_stack.push(SequenceGuiSnapshot {
            sequence: session.sequence.clone(),
            document: session.document.clone(),
            imports: session.imports.clone(),
            pending_imports: session.pending_imports.clone(),
        });
        session.sequence = next.sequence;
        session.document = next.document.clone();
        session.imports = next.imports;
        session.pending_imports = next.pending_imports;
        session.edit_revision = session.edit_revision.saturating_add(1);
        let revision = session.edit_revision;
        self.editors.mark_gui_edit_dirty(&key.path, revision);
        self.queue_sequence_session_save(&key)?;
        self.sync_preview_source_from_document(
            key.path,
            next.document,
            PreviewSyncMode::DeferRender,
        );
        self.status = "Redo".to_string();
        Ok(true)
    }

    fn active_sequence_key(&self) -> Result<SequenceKey, String> {
        let path = self.active_path_for_gui_edit()?;
        Ok(SequenceKey {
            object_key: self.active_sequence_object_key()?,
            path,
        })
    }

    fn active_sequence_authored(&self) -> Result<dawn_project::model::Sequence<Authored>, String> {
        let object_key = self.active_sequence_object_key()?;
        let path = self.active_path_for_gui_edit()?;
        if let Some(session) = self.sequence_gui_sessions.get(&SequenceKey {
            path: path.clone(),
            object_key: object_key.clone(),
        }) {
            return Ok(session.sequence.clone());
        }
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
        let view_mode = self
            .editors
            .active_buffer()
            .map(|buffer| buffer.view_mode)
            .unwrap_or(EditorViewMode::Text);
        if let Some(document) = self.cached_active_sequence_document(&path, &object_key, view_mode)
        {
            return Ok(document);
        }
        Err("active sequence GUI document is not cached".to_string())
    }

    fn active_sequence_object_key(&self) -> Result<String, String> {
        let path = self.active_path_for_gui_edit()?;
        let descriptor = self
            .workspace
            .inspect_document(path, self.editors.dirty_overlays())?;
        descriptor
            .default_object_keys
            .get(&DocumentViewId::Sequence)
            .cloned()
            .ok_or_else(|| "active document is not a sequence".to_string())
    }

    fn copy_sequence_selection(
        &mut self,
        sequence: &dawn_project::model::Sequence<Authored>,
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
                            .map(|time_seconds| SequenceMarkPasteDocumentEdit {
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

    fn sequence_paste_edit(
        &self,
        document: &SequenceDocument,
        anchor: SequencePasteAnchorDto,
    ) -> Result<(SequenceDocumentEdit, SequenceSelectionDto, u32), String> {
        match self.sequence_clipboard.clone() {
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
            .inspect_document(path.clone(), self.editors.dirty_overlays())?;
        let object_key = descriptor
            .default_object_keys
            .get(&DocumentViewId::Layout)
            .ok_or_else(|| "active document is not a layout".to_string())?
            .clone();
        let mut document = self.workspace.layout_document(
            path.clone(),
            &object_key,
            self.editors.dirty_overlays(),
        )?;
        match edit {
            LayoutGuiEditDto::UpdatePlacementTransform { id, transform } => {
                let id = dawn_project::model::FixtureId(id);
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
            self.editors.dirty_overlays(),
        )?;
        self.commit_active_gui_text(outcome.serialized_content)
    }

    fn apply_fixture_gui_edit(&mut self, edit: FixtureGuiEditDto) -> Result<(), String> {
        self.ensure_active_buffer_not_conflicted()?;
        let path = self.active_path_for_gui_edit()?;
        let mut document =
            self.workspace
                .fixture_document(path.clone(), None, self.editors.dirty_overlays())?;
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
                    dawn_project::model::DistanceSpan::try_from_meters_f64_truncated(
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
            self.editors.dirty_overlays(),
        )?;
        self.commit_active_gui_text(outcome.serialized_content)
    }

    fn active_path_for_gui_edit(&self) -> Result<Utf8PathBuf, String> {
        self.editors
            .active_file()
            .cloned()
            .ok_or_else(|| "no active document".to_string())
    }

    fn active_buffer_text(&self) -> Result<String, String> {
        self.editors
            .active_buffer()
            .map(|buffer| buffer.text.clone())
            .ok_or_else(|| "no active document".to_string())
    }

    fn commit_active_gui_text(&mut self, text: String) -> Result<(), String> {
        self.editors.replace_active_text_from_edit(text);
        self.refresh_analysis_after_memory_edit();
        Ok(())
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

    fn active_sequence_source(
        &self,
    ) -> Option<(SequenceKey, dawn_project::document::SequenceDocument)> {
        let path = self.editors.active_file()?.clone();
        let overlays = self.editors.dirty_overlays();
        let descriptor = self
            .workspace
            .inspect_document(path.clone(), overlays.clone())
            .ok()?;
        let object_key = descriptor
            .default_object_keys
            .get(&dawn_project::document::DocumentViewId::Sequence)?;
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

fn buffer_matches_any_path(path: &Utf8PathBuf, changed_paths: &[Utf8PathBuf]) -> bool {
    changed_paths
        .iter()
        .any(|changed_path| path == changed_path || path.starts_with(changed_path))
}
