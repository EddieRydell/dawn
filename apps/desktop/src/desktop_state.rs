use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::dto::{
    AppSettings, AppSnapshot, AudioTransportSnapshot, AudioTransportState, PackageReadiness,
    PackageStatus, ProjectHealth, WorkspaceExplorerState, WorkspaceLayoutState,
};
use crate::persistence::PersistenceService;
use crate::state_tasks::{
    GuiHistory, GuiSaveScheduler, ProjectAnalysisScheduler, RenderRefreshScheduler,
    gui_save_scheduler, project_analysis_scheduler, render_refresh_scheduler,
};
use camino::{Utf8Path, Utf8PathBuf};
use dawn_project_io::{ProjectRecovery, ProjectSession};

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
pub(super) use editor_projection::{
    descriptor_for_path, editor_buffer, editor_buffer_for_path, generated_source_texts,
    recovery_descriptor_for_path, recovery_editor_buffer, refresh_clean_buffers,
    restored_active_buffers, upsert_tab,
};
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
pub(super) use workspace_projection::{FsEntryKind, recovery_workspace_entries, workspace_entries};

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
