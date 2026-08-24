use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::dto::{
    AppSettings, AppSnapshot, AudioTransportSnapshot, AudioTransportState, BufferExternalState,
    DocumentDefaultObjectKey, DocumentDescriptor, DocumentObjectDescriptor, DocumentViewId,
    EditorBuffer, ObjectKind, PackageReadiness, PackageStatus, ProjectHealth, WorkspaceEntry,
    WorkspaceEntryKind, WorkspaceEntryOwnership, WorkspaceEntryRole, WorkspaceExplorerState,
    WorkspaceLayoutState, WorkspaceOperation,
};
use crate::persistence::{PersistedProjectSession, PersistenceService};
use crate::state_tasks::{
    GuiHistory, GuiSaveScheduler, ProjectAnalysisScheduler, RenderRefreshScheduler,
    gui_save_scheduler, project_analysis_scheduler, render_refresh_scheduler,
};
use camino::{Utf8Path, Utf8PathBuf};
use dawn_project_io::{ProjectRecovery, ProjectSession, SourceDocument, SourceObjectKind};

#[derive(Clone)]
pub(crate) enum LoadedProject {
    Closed,
    Ready(Arc<ProjectSession>),
    Recovery(Arc<ProjectRecovery>),
}

pub(crate) struct DesktopState {
    snapshot: Mutex<AppSnapshot>,
    project: Mutex<LoadedProject>,
    gui_history: Mutex<GuiHistory>,
    gui_save: Mutex<GuiSaveScheduler>,
    project_analysis: Mutex<ProjectAnalysisScheduler>,
    render_refresh: Mutex<RenderRefreshScheduler>,
    audio: Arc<Mutex<crate::audio::AudioEngine>>,
    show_render: Arc<Mutex<crate::show_render::ShowRenderService>>,
    live_output: Mutex<crate::live_output::LiveOutputService>,
    sequence_clip_raster: Mutex<crate::sequence_clip_raster::SequenceClipRasterService>,
    sequence_clipboard: Mutex<Option<crate::gui::SequenceClipboard>>,
    pending_operator_rewrite: Mutex<Option<PendingOperatorRewriteState>>,
    next_operator_rewrite_token: Mutex<u32>,
    filesystem: Arc<Mutex<()>>,
    persistence: PersistenceService,
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl DesktopState {
    pub(crate) fn new() -> Self {
        let audio = Arc::new(Mutex::new(crate::audio::AudioEngine::new()));
        let show_render = Arc::new(Mutex::new(crate::show_render::ShowRenderService::new()));
        let live_output =
            crate::live_output::LiveOutputService::new(audio.clone(), show_render.clone());
        Self {
            snapshot: Mutex::new(empty_snapshot()),
            project: Mutex::new(LoadedProject::Closed),
            gui_history: Mutex::new(GuiHistory::new(100)),
            gui_save: Mutex::new(gui_save_scheduler()),
            project_analysis: Mutex::new(project_analysis_scheduler()),
            render_refresh: Mutex::new(render_refresh_scheduler()),
            audio,
            show_render,
            live_output: Mutex::new(live_output),
            sequence_clip_raster: Mutex::new(
                crate::sequence_clip_raster::SequenceClipRasterService::new(),
            ),
            sequence_clipboard: Mutex::new(None),
            pending_operator_rewrite: Mutex::new(None),
            next_operator_rewrite_token: Mutex::new(1),
            filesystem: Arc::new(Mutex::new(())),
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
        self.drain_project_analysis_results();
        self.drain_render_refresh_results();
        let live_output = lock_unpoisoned(&self.live_output).snapshot();
        let mut snapshot = lock_unpoisoned(&self.snapshot).clone();
        snapshot.live_output = live_output;
        snapshot.audio_transport = self.merged_audio_snapshot(&snapshot.audio_transport);
        snapshot
    }

    pub fn audio_snapshot(&self) -> AudioTransportSnapshot {
        lock_unpoisoned(&self.audio).snapshot()
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
        let snapshot = {
            let mut snapshot = lock_unpoisoned(&self.snapshot);
            update(&mut snapshot);
            snapshot.audio_transport = self.merged_audio_snapshot(&snapshot.audio_transport);
            snapshot.clone()
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
            snapshot.settings = settings;
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
        let current = lock_unpoisoned(&self.snapshot).render_error.clone();
        if current.as_deref() != Some(message.as_str()) {
            self.update_snapshot(|snapshot| {
                snapshot.render_error = Some(message);
            });
        }
    }

    pub fn clear_render_error_if_set(&self) {
        let current = lock_unpoisoned(&self.snapshot).render_error.is_some();
        if current {
            self.update_snapshot(|snapshot| {
                snapshot.render_error = None;
            });
        }
    }

    pub fn set_live_output_active(&self, active: bool) -> AppSnapshot {
        let live_output = if active {
            let Some(project) = self.project_session() else {
                return self.update_snapshot(|snapshot| {
                    snapshot.live_output.state = crate::dto::LiveOutputState::Error;
                    snapshot.live_output.last_error = Some("No project is loaded.".to_string());
                });
            };
            let active = project
                .project
                .setups
                .get(&project.project.root.setup)
                .map(|setup| setup.controllers.clone());
            let render_ready = lock_unpoisoned(&self.show_render).active_target().is_some();
            let Some(active) = active.filter(|active| render_ready && !active.is_empty()) else {
                return self.update_snapshot(|snapshot| {
                    snapshot.live_output.state = crate::dto::LiveOutputState::Error;
                    snapshot.live_output.last_error = Some(
                        "Live output requires a prepared sequence and at least one active controller."
                            .to_string(),
                    );
                });
            };
            lock_unpoisoned(&self.live_output).enable(project.project.controllers.clone(), active)
        } else {
            lock_unpoisoned(&self.live_output).disable()
        };
        self.update_snapshot(|snapshot| snapshot.live_output = live_output)
    }

    pub(super) fn suspend_live_output(&self) {
        let live_output = lock_unpoisoned(&self.live_output).suspend();
        lock_unpoisoned(&self.snapshot).live_output = live_output;
    }

    pub(super) fn disable_live_output(&self) {
        let live_output = lock_unpoisoned(&self.live_output).disable();
        lock_unpoisoned(&self.snapshot).live_output = live_output;
    }

    pub(super) fn resume_live_output_after_prepare(&self) {
        if lock_unpoisoned(&self.live_output).take_resume_after_prepare() {
            let _ = self.set_live_output_active(true);
        }
    }

    pub(crate) fn shutdown_live_output(&self) {
        lock_unpoisoned(&self.live_output).shutdown();
    }
}

mod audio;
mod diagnostics;
pub(super) use diagnostics::{project_diagnostic, project_diagnostics};
mod editor_projection;
pub(super) use editor_projection::{generated_source_texts, refresh_clean_buffers, upsert_tab};
mod filesystem;
mod gui_editing;
mod operator_rewrite;
mod packages;
pub(crate) use packages::{decorate_deprecation_status, package_status};
mod project_lifecycle;
mod rendering;
mod search;
mod settings;
pub(super) use settings::{sanitize_app_settings, sanitize_workspace_layout};
mod workspace;
mod workspace_projection;
pub(super) use workspace_projection::{
    FsEntryKind, canonical_relative_path, collect_workspace_paths, insert_path_with_parents,
};

fn empty_snapshot() -> AppSnapshot {
    AppSnapshot {
        settings: AppSettings::default(),
        workspace_layout: WorkspaceLayoutState::default(),
        workspace_explorer: WorkspaceExplorerState::default(),
        project_root: None,
        project_health: ProjectHealth::Closed,
        project_revision: 0,
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
        live_output: crate::live_output::disabled_snapshot(0),
        pending_operator_rewrite: None,
        package: PackageStatus {
            readiness: PackageReadiness::NoProject,
            root: None,
            manifest_valid: false,
            lock_present: false,
            lock_current: false,
            registry: None,
            update_checked: false,
            dependencies: Vec::new(),
            modules: Vec::new(),
            warnings: Vec::new(),
            message: None,
        },
    }
}

pub(crate) struct PendingOperatorRewriteState {
    pub token: u32,
    pub project_revision: u32,
    pub target_documents: BTreeSet<dawn_language::identity::DocumentId>,
    pub kind: PendingOperatorRewriteKind,
}

pub(crate) enum PendingOperatorRewriteKind {
    Document {
        path: Utf8PathBuf,
        compiled: Box<dawn_project_io::CompiledOperatorDocument>,
    },
    PackageUpdate {
        root: Utf8PathBuf,
        candidate: Box<dawn_package::PreparedPackageCandidate>,
        session: Box<ProjectSession>,
    },
}

fn workspace_entries(session: &ProjectSession) -> Vec<WorkspaceEntry> {
    let mut paths = BTreeMap::new();
    collect_workspace_paths(session.source.project_root(), Utf8Path::new(""), &mut paths);
    for document in session.source.documents.keys() {
        if session.source.is_project_owned(document) {
            insert_path_with_parents(&mut paths, document.path());
        }
    }
    paths
        .into_iter()
        .map(|(path, kind)| workspace_entry(session, path, kind))
        .collect()
}

fn recovery_workspace_entries(recovery: &ProjectRecovery) -> Vec<WorkspaceEntry> {
    let mut paths = BTreeMap::new();
    collect_workspace_paths(&recovery.root, Utf8Path::new(""), &mut paths);
    paths
        .into_iter()
        .map(|(path, kind)| {
            let name = path
                .file_name()
                .map(ToString::to_string)
                .unwrap_or_else(|| path.to_string());
            let parent = path.parent().map(Utf8Path::to_string).unwrap_or_default();
            let role = recovery_workspace_role(recovery, &path, kind);
            let fixed = matches!(
                role,
                WorkspaceEntryRole::Manifest | WorkspaceEntryRole::Lockfile
            );
            WorkspaceEntry {
                path: canonical_relative_path(&path).to_string(),
                kind: match kind {
                    FsEntryKind::Directory => WorkspaceEntryKind::Directory,
                    FsEntryKind::File => WorkspaceEntryKind::File,
                },
                name,
                parent,
                role,
                ownership: WorkspaceEntryOwnership::Project,
                operations: match kind {
                    FsEntryKind::File => vec![WorkspaceOperation::Open],
                    FsEntryKind::Directory => Vec::new(),
                },
                operation_explanation: Some(if fixed {
                    "Package manifests and lockfiles remain fixed at the project root.".to_string()
                } else {
                    "Project-model operations are disabled until project errors are fixed."
                        .to_string()
                }),
            }
        })
        .collect()
}

fn recovery_workspace_role(
    recovery: &ProjectRecovery,
    path: &Utf8Path,
    kind: FsEntryKind,
) -> WorkspaceEntryRole {
    if matches!(kind, FsEntryKind::Directory) {
        return WorkspaceEntryRole::Directory;
    }
    if path == Utf8Path::new(dawn_package::MANIFEST_FILE) {
        return WorkspaceEntryRole::Manifest;
    }
    if path == Utf8Path::new(dawn_package::LOCK_FILE) {
        return WorkspaceEntryRole::Lockfile;
    }
    let Some(document) = recovery.documents.get(path) else {
        return if recovery
            .manifest
            .as_ref()
            .is_some_and(|manifest| manifest.assets.contains_key(path.as_str()))
        {
            WorkspaceEntryRole::Asset
        } else {
            WorkspaceEntryRole::File
        };
    };
    match document.kind {
        dawn_project_io::RecoveryDocumentKind::Effect => WorkspaceEntryRole::Effect,
        dawn_project_io::RecoveryDocumentKind::Operator => WorkspaceEntryRole::Operator,
        dawn_project_io::RecoveryDocumentKind::Other => WorkspaceEntryRole::File,
        dawn_project_io::RecoveryDocumentKind::Dawn => document
            .objects
            .iter()
            .map(|object| crate::dto::workspace_role_for_source_object(&object.kind))
            .next()
            .unwrap_or(WorkspaceEntryRole::File),
    }
}

fn workspace_entry(
    session: &ProjectSession,
    path: Utf8PathBuf,
    kind: FsEntryKind,
) -> WorkspaceEntry {
    let name = path
        .file_name()
        .map(ToString::to_string)
        .unwrap_or_else(|| path.to_string());
    let parent = path.parent().map(Utf8Path::to_string).unwrap_or_default();
    let (ownership, module_id, module_relative) = workspace_ownership(session, &path);
    let role = workspace_role(session, &path, kind, module_id, module_relative.as_deref());
    let fixed = matches!(
        role,
        WorkspaceEntryRole::Manifest | WorkspaceEntryRole::Lockfile
    );
    let structural = session.source.is_structural_workspace_path(&path);
    let operations = match kind {
        FsEntryKind::Directory => {
            let mut operations = vec![WorkspaceOperation::Create];
            if !fixed {
                operations.extend([WorkspaceOperation::Rename, WorkspaceOperation::Move]);
                if !structural {
                    operations.push(WorkspaceOperation::Delete);
                }
            }
            operations
        }
        FsEntryKind::File => {
            let mut operations = vec![WorkspaceOperation::Open];
            if !fixed {
                operations.extend([WorkspaceOperation::Rename, WorkspaceOperation::Move]);
                if !structural {
                    operations.push(WorkspaceOperation::Delete);
                }
            }
            operations
        }
    };
    WorkspaceEntry {
        path: canonical_relative_path(&path).to_string(),
        kind: match kind {
            FsEntryKind::Directory => WorkspaceEntryKind::Directory,
            FsEntryKind::File => WorkspaceEntryKind::File,
        },
        name,
        parent,
        role,
        ownership,
        operations,
        operation_explanation: if fixed {
            Some("Package manifests and lockfiles remain fixed at their module root.".to_string())
        } else if structural {
            Some(
                "Imported documents cannot be deleted; rename or move them through the typed path workflow."
                    .to_string(),
            )
        } else {
            None
        },
    }
}

fn workspace_ownership(
    session: &ProjectSession,
    path: &Utf8Path,
) -> (
    WorkspaceEntryOwnership,
    Option<uuid::Uuid>,
    Option<Utf8PathBuf>,
) {
    let absolute = session.source.project_root().join(path);
    let mut modules = session
        .source
        .source_graph
        .modules()
        .iter()
        .filter(|(_, module)| {
            !matches!(
                module.origin,
                dawn_package::ResolvedModuleOrigin::RegistryDependency { .. }
            ) && absolute.starts_with(&module.root)
        })
        .collect::<Vec<_>>();
    modules.sort_by_key(|(_, module)| module.root.components().count());
    let Some((module_id, module)) = modules.last() else {
        return (WorkspaceEntryOwnership::Project, None, None);
    };
    let ownership = match module.origin {
        dawn_package::ResolvedModuleOrigin::Project => WorkspaceEntryOwnership::Project,
        dawn_package::ResolvedModuleOrigin::PathDependency { .. } => {
            WorkspaceEntryOwnership::PathDependency
        }
        dawn_package::ResolvedModuleOrigin::RegistryDependency { .. } => {
            WorkspaceEntryOwnership::Registry
        }
    };
    (
        ownership,
        Some(**module_id),
        absolute
            .strip_prefix(&module.root)
            .ok()
            .map(Utf8Path::to_path_buf),
    )
}

fn workspace_role(
    session: &ProjectSession,
    path: &Utf8Path,
    kind: FsEntryKind,
    module_id: Option<uuid::Uuid>,
    module_relative: Option<&Utf8Path>,
) -> WorkspaceEntryRole {
    if matches!(kind, FsEntryKind::Directory) {
        if session
            .source
            .source_graph
            .modules()
            .values()
            .any(|module| {
                matches!(
                    module.origin,
                    dawn_package::ResolvedModuleOrigin::PathDependency { .. }
                ) && module.root == session.source.project_root().join(path)
            })
        {
            return WorkspaceEntryRole::PathDependency;
        }
        return WorkspaceEntryRole::Directory;
    }
    let Some(module_id) = module_id else {
        return WorkspaceEntryRole::File;
    };
    let Some(relative) = module_relative else {
        return WorkspaceEntryRole::File;
    };
    if relative == Utf8Path::new(dawn_package::MANIFEST_FILE) {
        return WorkspaceEntryRole::Manifest;
    }
    if relative == Utf8Path::new(dawn_package::LOCK_FILE) {
        return WorkspaceEntryRole::Lockfile;
    }
    let document_id = dawn_language::identity::DocumentId::new(module_id, relative.to_path_buf());
    if session.source.entrypoint.as_ref() == Some(&document_id) {
        return WorkspaceEntryRole::Entrypoint;
    }
    if let Some(document) = session.source.documents.get(&document_id) {
        return match document.kind() {
            dawn_project_io::SourceDocumentKind::Effect { .. } => WorkspaceEntryRole::Effect,
            dawn_project_io::SourceDocumentKind::Operator { .. } => WorkspaceEntryRole::Operator,
            dawn_project_io::SourceDocumentKind::Dawn { .. } => document
                .objects()
                .iter()
                .map(|object| crate::dto::workspace_role_for_source_object(object.kind()))
                .next()
                .unwrap_or(WorkspaceEntryRole::File),
        };
    }
    let module = session.source.source_graph.module(module_id).ok();
    if module.is_some_and(|module| module.manifest.assets.contains_key(relative.as_str()))
        || session
            .source
            .referenced_assets
            .iter()
            .any(|asset| asset.module_id == module_id && asset.relative_path == relative)
    {
        WorkspaceEntryRole::Asset
    } else {
        WorkspaceEntryRole::File
    }
}

fn editor_buffer(session: &ProjectSession, relative_path: &Utf8Path) -> Option<EditorBuffer> {
    editor_buffer_at_root(session.source.project_root(), relative_path)
}

fn editor_buffer_at_root(root: &Utf8Path, relative_path: &Utf8Path) -> Option<EditorBuffer> {
    let disk_path = root.join(relative_path);
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

fn recovery_editor_buffer(
    recovery: &ProjectRecovery,
    relative_path: &Utf8Path,
) -> Option<EditorBuffer> {
    let path = absolute_root_path(&recovery.root, relative_path)?;
    path.is_file()
        .then(|| editor_buffer_at_root(&recovery.root, relative_path))
        .flatten()
}

fn editor_buffer_for_path(
    session: &ProjectSession,
    relative_path: &Utf8Path,
) -> Option<EditorBuffer> {
    let path = absolute_project_path(session, relative_path)?;
    if !path.is_file() {
        return None;
    }
    editor_buffer(session, relative_path)
}

fn restored_active_buffers(
    session: &ProjectSession,
    restore: Option<&PersistedProjectSession>,
) -> Option<(Vec<EditorBuffer>, String)> {
    let restore = restore?;
    let mut buffers = Vec::new();
    for path in &restore.tabs {
        let relative_path = Utf8Path::new(path);
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
    absolute_project_path(session, relative_path)
        .and_then(|_| session.source.document_for_workspace_path(relative_path))
        .and_then(|document_id| session.source.documents.get(&document_id))
        .map(|document| document_descriptor(relative_path, document))
        .or_else(|| {
            absolute_project_path(session, relative_path)
                .is_some_and(|path| path.is_file())
                .then(|| empty_document_descriptor(relative_path))
        })
}

fn recovery_descriptor_for_path(
    recovery: &ProjectRecovery,
    relative_path: &Utf8Path,
) -> Option<DocumentDescriptor> {
    recovery
        .documents
        .get(relative_path)
        .map(|document| {
            let objects = document
                .objects
                .iter()
                .filter(|object| {
                    object.kind != SourceObjectKind::Sequence || object.sequence.is_some()
                })
                .map(|object| DocumentObjectDescriptor {
                    key: object.key.clone(),
                    kind: ObjectKind::from(&object.kind),
                })
                .collect::<Vec<_>>();
            DocumentDescriptor {
                path: relative_path.to_string(),
                available_views: available_views(&objects),
                default_object_keys: default_object_keys(&objects),
                objects,
            }
        })
        .or_else(|| {
            absolute_root_path(&recovery.root, relative_path)
                .is_some_and(|path| path.is_file())
                .then(|| empty_document_descriptor(relative_path))
        })
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
        let view = object.kind.document_view();
        if let Some(view) = view
            && !views.contains(&view)
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
            let view = object.kind.document_view()?;
            Some(DocumentDefaultObjectKey {
                view,
                object_key: object.key.clone(),
            })
        })
        .collect()
}

fn absolute_project_path(
    session: &ProjectSession,
    relative_path: &Utf8Path,
) -> Option<Utf8PathBuf> {
    absolute_root_path(session.source.project_root(), relative_path)
}

fn absolute_root_path(root: &Utf8Path, relative_path: &Utf8Path) -> Option<Utf8PathBuf> {
    if relative_path.as_str().is_empty()
        || relative_path.is_absolute()
        || relative_path.as_str().contains('\\')
        || !relative_path
            .components()
            .all(|component| matches!(component, camino::Utf8Component::Normal(_)))
    {
        return None;
    }
    Some(root.join(relative_path))
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
