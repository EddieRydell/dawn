use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_project_io::{
    IoDiagnostic, IoDiagnosticSeverity, ProjectCheckReport, ProjectSession, SourceDocument,
    SourceObjectKind, check_document_text, check_project, check_project_document_text,
    is_project_owned_path, save_project, source_document_text as generated_source_document_text,
};
use indexmap::IndexSet;

use crate::dto::{
    AppSettings, AppSnapshot, AudioTransportSnapshot, AudioTransportState, BufferExternalState,
    DiagnosticSeverity, DocumentDefaultObjectKey, DocumentDescriptor, DocumentObjectDescriptor,
    DocumentViewId, EditorBuffer, EditorViewMode, GuiDocument, GuiDocumentRequest, GuiEditCommand,
    GuiEditResult, LiveOutputSnapshot, NewSequenceRequest, ObjectKind, ProjectDiagnostic,
    ProjectTreeMode, SequenceAudio, SequenceSelectionEdit, SequenceSelectionEditResult,
    WorkspaceEntry, WorkspaceEntryKind, WorkspaceLayoutState,
};
use crate::persistence::{
    PersistedEditorViewStateUpdate, PersistedProjectSession, PersistedSequenceViewportStateUpdate,
    PersistenceService, ProjectRestoreState,
};
use crate::project_templates::{new_project_files, write_new_project_files};
use crate::state_tasks::{
    GuiHistory, GuiHistoryEntry, GuiSavePayload, GuiSaveResult, GuiSaveScheduler,
    RenderRefreshPayload, RenderRefreshResult, RenderRefreshScheduler, gui_save_scheduler,
    render_refresh_scheduler,
};

pub struct DesktopState {
    snapshot: Mutex<AppSnapshot>,
    project: Mutex<Option<Arc<ProjectSession>>>,
    gui_history: Mutex<GuiHistory>,
    gui_save: Mutex<GuiSaveScheduler>,
    render_refresh: Mutex<RenderRefreshScheduler>,
    audio: Mutex<crate::audio::AudioEngine>,
    show_render: Mutex<crate::show_render::ShowRenderService>,
    sequence_clip_raster: Mutex<crate::sequence_clip_raster::SequenceClipRasterService>,
    sequence_clipboard: Mutex<Option<crate::gui::SequenceClipboard>>,
    persistence: PersistenceService,
}

impl DesktopState {
    pub fn new() -> Self {
        Self {
            snapshot: Mutex::new(empty_snapshot()),
            project: Mutex::new(None),
            gui_history: Mutex::new(GuiHistory::new(100)),
            gui_save: Mutex::new(gui_save_scheduler()),
            render_refresh: Mutex::new(render_refresh_scheduler()),
            audio: Mutex::new(crate::audio::AudioEngine::new()),
            show_render: Mutex::new(crate::show_render::ShowRenderService::new()),
            sequence_clip_raster: Mutex::new(
                crate::sequence_clip_raster::SequenceClipRasterService::new(),
            ),
            sequence_clipboard: Mutex::new(None),
            persistence: PersistenceService::new(),
        }
    }

    pub fn persistence(&self) -> &PersistenceService {
        &self.persistence
    }

    pub fn apply_persisted_settings(&self) -> AppSnapshot {
        let settings = sanitize_app_settings(self.persistence.settings());
        let workspace_layout = sanitize_workspace_layout(self.persistence.workspace_layout());
        self.update_snapshot(|snapshot| {
            snapshot.settings = settings;
            snapshot.workspace_layout = workspace_layout;
        })
    }

    pub fn snapshot(&self) -> AppSnapshot {
        self.drain_gui_save_results();
        self.drain_render_refresh_results();
        let mut snapshot = match self.snapshot.lock() {
            Ok(snapshot) => snapshot.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        snapshot.audio_transport = self.merged_audio_snapshot(&snapshot.audio_transport);
        snapshot
    }

    pub fn audio_snapshot(&self) -> AudioTransportSnapshot {
        match self.audio.lock() {
            Ok(mut audio) => audio.snapshot(),
            Err(poisoned) => poisoned.into_inner().snapshot(),
        }
    }

    fn merged_audio_snapshot(&self, previous: &AudioTransportSnapshot) -> AudioTransportSnapshot {
        let mut audio_transport = self.audio_snapshot();
        if matches!(audio_transport.state, AudioTransportState::Unloaded) {
            audio_transport.position_seconds = previous.position_seconds;
            audio_transport.home_seconds = previous.home_seconds;
        }
        audio_transport
    }

    pub fn update_snapshot(&self, update: impl FnOnce(&mut AppSnapshot)) -> AppSnapshot {
        let snapshot = match self.snapshot.lock() {
            Ok(mut snapshot) => {
                update(&mut snapshot);
                snapshot.audio_transport = self.merged_audio_snapshot(&snapshot.audio_transport);
                snapshot.clone()
            }
            Err(poisoned) => {
                let mut snapshot = poisoned.into_inner();
                update(&mut snapshot);
                snapshot.audio_transport = self.merged_audio_snapshot(&snapshot.audio_transport);
                snapshot.clone()
            }
        };
        self.record_persistent_snapshot(&snapshot);
        snapshot
    }

    pub fn set_persistence_error(&self, message: String) -> AppSnapshot {
        self.update_snapshot(|snapshot| {
            snapshot.status = message;
        })
    }

    pub fn update_app_settings(&self, settings: AppSettings) -> AppSnapshot {
        let settings = sanitize_app_settings(settings);
        if let Err(error) = self.persistence.record_settings(settings.clone()) {
            return self.set_persistence_error(format!("Settings were not saved: {error}"));
        }
        self.update_snapshot(|snapshot| {
            let tree_mode = settings.project_tree_mode.clone();
            snapshot.settings = settings;
            match tree_mode {
                ProjectTreeMode::Show => snapshot.project_tree_visible = true,
                ProjectTreeMode::Hide => snapshot.project_tree_visible = false,
                ProjectTreeMode::Remember => {}
            }
        })
    }

    pub fn save_workspace_layout_state(&self, state: WorkspaceLayoutState) -> AppSnapshot {
        let state = sanitize_workspace_layout(state);
        match self.persistence.record_workspace_layout(state.clone()) {
            Ok(()) => self.update_snapshot(|snapshot| {
                snapshot.workspace_layout = state;
            }),
            Err(error) => {
                self.set_persistence_error(format!("Workspace layout was not saved: {error}"))
            }
        }
    }

    pub fn set_render_error_if_changed(&self, message: String) {
        let current = match self.snapshot.lock() {
            Ok(snapshot) => snapshot.render_error.clone(),
            Err(poisoned) => poisoned.into_inner().render_error.clone(),
        };
        if current.as_deref() != Some(message.as_str()) {
            self.update_snapshot(|snapshot| {
                snapshot.render_error = Some(message);
            });
        }
    }

    pub fn clear_render_error_if_set(&self) {
        let current = match self.snapshot.lock() {
            Ok(snapshot) => snapshot.render_error.is_some(),
            Err(poisoned) => poisoned.into_inner().render_error.is_some(),
        };
        if current {
            self.update_snapshot(|snapshot| {
                snapshot.render_error = None;
            });
        }
    }
}

mod audio;
mod filesystem;
mod gui_editing;
mod project_lifecycle;
mod rendering;
mod workspace;

fn project_path_is_structural(project: &ProjectSession, path: &Utf8Path) -> bool {
    path == project.source.entrypoint
        || project.source.documents.values().any(|document| {
            document.imports().iter().any(|edge| {
                edge.targets()
                    .iter()
                    .any(|target| target == path || target.starts_with(path))
            })
        })
}

impl Default for DesktopState {
    fn default() -> Self {
        Self::new()
    }
}

fn empty_snapshot() -> AppSnapshot {
    AppSnapshot {
        settings: AppSettings::default(),
        workspace_layout: WorkspaceLayoutState::default(),
        project_root: None,
        project_revision: 0,
        project_tree_visible: true,
        project_entries: Vec::new(),
        tabs: Vec::new(),
        active_file: None,
        active_buffer: None,
        active_document_descriptor: None,
        diagnostics: Vec::new(),
        status: "Ready".to_string(),
        render_error: None,
        preview_error: None,
        preview_open: false,
        audio_transport: crate::audio::AudioEngine::empty_snapshot(),
        live_output: LiveOutputSnapshot {
            enabled: false,
            status: "Disabled".to_string(),
            active_universe_count: 0,
            last_error: None,
        },
    }
}

fn sanitize_workspace_layout(state: WorkspaceLayoutState) -> WorkspaceLayoutState {
    WorkspaceLayoutState {
        project_tree_width_px: clamp_f64(state.project_tree_width_px, 220.0, 520.0),
        inspector_width_px: clamp_f64(state.inspector_width_px, 240.0, 560.0),
        project_tree_collapsed: state.project_tree_collapsed,
        inspector_collapsed: state.inspector_collapsed,
    }
}

fn clamp_f64(value: f64, min: f64, max: f64) -> f64 {
    if !value.is_finite() {
        return min;
    }
    value.clamp(min, max)
}

fn normalize_project_entrypoint(path: &str) -> Utf8PathBuf {
    let path = Utf8PathBuf::from(path);
    if path.is_dir() {
        path.join("project.dawn")
    } else {
        path
    }
}

fn project_diagnostic(diagnostic: &IoDiagnostic) -> ProjectDiagnostic {
    ProjectDiagnostic {
        path: diagnostic.path.to_string(),
        range: diagnostic
            .range
            .as_ref()
            .map(|range| crate::dto::TextRange {
                start: crate::dto::TextPosition {
                    line: range.start.line,
                    character: range.start.character,
                },
                end: crate::dto::TextPosition {
                    line: range.end.line,
                    character: range.end.character,
                },
            }),
        severity: match diagnostic.severity {
            IoDiagnosticSeverity::Error => DiagnosticSeverity::Error,
            IoDiagnosticSeverity::Warning => DiagnosticSeverity::Warning,
        },
        code: diagnostic.code.as_str().to_string(),
        message: diagnostic.message.clone(),
    }
}

fn project_diagnostics(report: &ProjectCheckReport) -> Vec<ProjectDiagnostic> {
    report
        .diagnostics
        .iter()
        .map(project_diagnostic)
        .collect::<Vec<_>>()
}

fn workspace_entries(session: &ProjectSession) -> Vec<WorkspaceEntry> {
    let mut paths = IndexSet::new();
    collect_workspace_paths(&session.source.source_root, Utf8Path::new(""), &mut paths);
    for path in session.source.documents.keys() {
        if is_project_owned_path(path) {
            insert_path_with_parents(&mut paths, path);
        }
    }
    paths.sort();
    paths.into_iter().map(workspace_entry).collect()
}

fn collect_workspace_paths(
    root: &Utf8Path,
    relative: &Utf8Path,
    paths: &mut IndexSet<Utf8PathBuf>,
) {
    let absolute = root.join(relative);
    let Ok(entries) = fs::read_dir(absolute) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let path = if relative.as_str().is_empty() {
            Utf8PathBuf::from(name)
        } else {
            relative.join(name)
        };
        paths.insert(path.clone());
        if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            collect_workspace_paths(root, &path, paths);
        }
    }
}

fn insert_path_with_parents(paths: &mut IndexSet<Utf8PathBuf>, path: &Utf8Path) {
    let mut current = Utf8PathBuf::new();
    for component in path.components() {
        let camino::Utf8Component::Normal(part) = component else {
            continue;
        };
        current.push(part);
        paths.insert(current.clone());
    }
}

fn workspace_entry(path: Utf8PathBuf) -> WorkspaceEntry {
    let name = path
        .file_name()
        .map(ToString::to_string)
        .unwrap_or_else(|| path.to_string());
    let parent = path.parent().map(Utf8Path::to_string).unwrap_or_default();
    let kind = if path.extension().is_some() {
        WorkspaceEntryKind::File
    } else {
        WorkspaceEntryKind::Directory
    };
    WorkspaceEntry {
        path: path.to_string(),
        kind,
        name,
        parent,
    }
}

fn generated_source_texts(
    session: &ProjectSession,
    paths: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>, String> {
    let mut texts = BTreeMap::new();
    for path in paths {
        match generated_source_document_text(session, Utf8Path::new(path)) {
            Ok(Some(text)) => {
                texts.insert(path.clone(), text);
            }
            Ok(None) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(texts)
}

fn refresh_clean_buffers(snapshot: &mut AppSnapshot, generated_text: &BTreeMap<String, String>) {
    for (path, text) in generated_text {
        if let Some(tab) = snapshot.tabs.iter_mut().find(|tab| tab.path == *path) {
            if tab.dirty {
                tab.external_state = BufferExternalState::ChangedOnDisk;
            } else {
                tab.text = text.clone();
                tab.external_state = BufferExternalState::Current;
            }
        }
        if let Some(buffer) = snapshot
            .active_buffer
            .as_mut()
            .filter(|buffer| buffer.path == *path)
        {
            if buffer.dirty {
                buffer.external_state = BufferExternalState::ChangedOnDisk;
            } else {
                buffer.text = text.clone();
                buffer.external_state = BufferExternalState::Current;
            }
        }
    }
}

fn editor_buffer(session: &ProjectSession, relative_path: &Utf8Path) -> Option<EditorBuffer> {
    let disk_path = session.source.source_root.join(relative_path);
    let text = fs::read_to_string(&disk_path).ok()?;
    Some(EditorBuffer {
        path: relative_path.to_string(),
        name: relative_path
            .file_name()
            .map(ToString::to_string)
            .unwrap_or_else(|| relative_path.to_string()),
        text,
        dirty: false,
        external_state: BufferExternalState::Current,
    })
}

fn editor_buffer_for_path(
    session: &ProjectSession,
    relative_path: &Utf8Path,
) -> Option<EditorBuffer> {
    if let Some(_document) = session.source.documents.get(relative_path) {
        return editor_buffer(session, relative_path);
    }
    let path = absolute_project_path(session, relative_path)?;
    if !path.is_file() {
        return None;
    }
    let text = fs::read_to_string(&path).ok()?;
    Some(EditorBuffer {
        path: relative_path.to_string(),
        name: relative_path
            .file_name()
            .map(ToString::to_string)
            .unwrap_or_else(|| relative_path.to_string()),
        text,
        dirty: false,
        external_state: BufferExternalState::Current,
    })
}

fn restored_active_buffers(
    session: &ProjectSession,
    restore: Option<&PersistedProjectSession>,
) -> Option<(Vec<EditorBuffer>, String)> {
    let restore = restore?;
    let mut buffers = Vec::new();
    for tab in &restore.tabs {
        let relative_path = Utf8Path::new(&tab.path);
        if let Some(buffer) = editor_buffer_for_path(session, relative_path) {
            buffers.push(buffer);
        }
    }
    if buffers.is_empty() {
        return None;
    }
    let active_file = restore
        .active_file
        .as_ref()
        .filter(|path| buffers.iter().any(|buffer| &buffer.path == *path))
        .cloned()
        .unwrap_or_else(|| buffers[0].path.clone());
    Some((buffers, active_file))
}

fn descriptor_for_path(
    session: &ProjectSession,
    relative_path: &Utf8Path,
) -> Option<DocumentDescriptor> {
    session
        .source
        .documents
        .get(relative_path)
        .map(|document| document_descriptor(relative_path, document))
        .or_else(|| {
            absolute_project_path(session, relative_path)
                .is_some_and(|path| path.is_file())
                .then(|| empty_document_descriptor(relative_path))
        })
}

fn valid_project_paths(session: &ProjectSession) -> BTreeSet<String> {
    workspace_entries(session)
        .into_iter()
        .filter(|entry| matches!(entry.kind, WorkspaceEntryKind::File))
        .map(|entry| entry.path)
        .collect()
}

fn document_descriptor(path: &Utf8Path, document: &SourceDocument) -> DocumentDescriptor {
    let objects = document
        .objects()
        .iter()
        .map(|object| DocumentObjectDescriptor {
            key: object.id().to_string(),
            kind: ObjectKind::from(object.kind()),
        })
        .collect::<Vec<_>>();
    let available_views = available_views(&objects);
    let default_object_keys = default_object_keys(&objects);
    DocumentDescriptor {
        path: path.to_string(),
        objects,
        available_views,
        default_object_keys,
    }
}

fn empty_document_descriptor(path: &Utf8Path) -> DocumentDescriptor {
    DocumentDescriptor {
        path: path.to_string(),
        objects: Vec::new(),
        available_views: vec![DocumentViewId::Text],
        default_object_keys: Vec::new(),
    }
}

fn available_views(objects: &[DocumentObjectDescriptor]) -> Vec<DocumentViewId> {
    let mut views = vec![DocumentViewId::Text];
    for object in objects {
        let view = match object.kind {
            ObjectKind::Layout => Some(DocumentViewId::Layout),
            ObjectKind::Fixture => Some(DocumentViewId::Fixture),
            ObjectKind::Sequence => Some(DocumentViewId::Sequence),
            _ => None,
        };
        if let Some(view) = view
            && !views.iter().any(|existing| same_view(existing, &view))
        {
            views.push(view);
        }
    }
    views
}

fn default_object_keys(objects: &[DocumentObjectDescriptor]) -> Vec<DocumentDefaultObjectKey> {
    objects
        .iter()
        .filter_map(|object| {
            let view = match object.kind {
                ObjectKind::Layout => DocumentViewId::Layout,
                ObjectKind::Fixture => DocumentViewId::Fixture,
                ObjectKind::Sequence => DocumentViewId::Sequence,
                _ => return None,
            };
            Some(DocumentDefaultObjectKey {
                view,
                object_key: object.key.clone(),
            })
        })
        .collect()
}

fn same_view(left: &DocumentViewId, right: &DocumentViewId) -> bool {
    matches!(
        (left, right),
        (DocumentViewId::Text, DocumentViewId::Text)
            | (DocumentViewId::Layout, DocumentViewId::Layout)
            | (DocumentViewId::Fixture, DocumentViewId::Fixture)
            | (DocumentViewId::Sequence, DocumentViewId::Sequence)
    )
}

fn sanitize_app_settings(mut settings: AppSettings) -> AppSettings {
    if !settings.sequence_initial_px_per_second.is_finite() {
        settings.sequence_initial_px_per_second = 80.0;
    }
    if !settings.sequence_initial_lane_height_px.is_finite() {
        settings.sequence_initial_lane_height_px = 42.0;
    }
    if !settings.effect_raster.render_scale.is_finite() {
        settings.effect_raster.render_scale = 1.0;
    }
    settings.sequence_initial_px_per_second =
        settings.sequence_initial_px_per_second.clamp(20.0, 12000.0);
    settings.sequence_initial_lane_height_px =
        settings.sequence_initial_lane_height_px.clamp(24.0, 120.0);
    settings.effect_raster.render_scale = settings.effect_raster.render_scale.clamp(0.25, 2.0);
    settings.effect_raster.max_columns = settings.effect_raster.max_columns.clamp(16, 1024);
    settings.effect_raster.max_rows = settings.effect_raster.max_rows.clamp(1, 200);
    settings.effect_raster.min_frame_stride = settings.effect_raster.min_frame_stride.clamp(1, 16);
    settings
}

fn upsert_tab(tabs: &mut Vec<EditorBuffer>, buffer: EditorBuffer) {
    if let Some(tab) = tabs.iter_mut().find(|tab| tab.path == buffer.path) {
        *tab = buffer;
    } else {
        tabs.push(buffer);
    }
}

#[derive(Clone, Copy)]
enum FsEntryKind {
    File,
    Directory,
}

fn absolute_project_path(
    session: &ProjectSession,
    relative_path: &Utf8Path,
) -> Option<Utf8PathBuf> {
    if !is_project_owned_path(relative_path) {
        return None;
    }
    Some(session.source.source_root.join(relative_path))
}

fn valid_child_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && name != "." && name != ".."
}

fn path_matches_or_is_child(candidate: &str, parent: &str) -> bool {
    candidate == parent
        || candidate
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.starts_with('/') || suffix.starts_with('\\'))
}
