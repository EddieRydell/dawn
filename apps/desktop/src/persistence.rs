use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, Runtime, Window};

use crate::dto::{AppSettings, AppSnapshot, EditorViewMode};

const VERSION: u32 = 1;
const FILE_NAME: &str = "desktop-state-v1.json";
const MAX_RECENT_PROJECTS: usize = 10;
const SAVE_DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedEditorViewState {
    pub cursor_anchor: u32,
    pub cursor_head: u32,
    pub scroll_top: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSequenceViewportState {
    pub px_per_second: f64,
    pub lane_height: f64,
    pub scroll_x_seconds: f64,
    pub scroll_y: f64,
    pub active_mark_collection_key: Option<String>,
    pub visible_mark_collection_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedWindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedPreviewWindowState {
    pub open: bool,
    pub geometry: Option<PersistedWindowState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedEditorViewStateUpdate {
    pub path: String,
    pub state: PersistedEditorViewState,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedSequenceViewportStateUpdate {
    pub path: String,
    pub object_key: String,
    pub state: PersistedSequenceViewportState,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRestoreState {
    pub editor_states: BTreeMap<String, PersistedEditorViewState>,
    pub sequence_viewports: BTreeMap<String, PersistedSequenceViewportState>,
}

#[derive(Debug, Clone)]
pub struct ProjectSessionRestore {
    pub session: PersistedProjectSession,
    pub stale_tabs: Vec<String>,
}

#[derive(Debug, Default)]
pub struct PersistenceService {
    inner: Mutex<PersistenceInner>,
}

impl PersistenceService {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(PersistenceInner::default()),
        }
    }

    pub fn load(&self, app: &AppHandle) -> Result<Option<String>, String> {
        let path = persistence_path(app)?;
        let mut inner = self.inner();
        inner.path = Some(path.clone());
        if !path.exists() {
            inner.write_allowed = true;
            return Ok(None);
        }
        let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        let store =
            serde_json::from_str::<PersistedStore>(&text).map_err(|error| error.to_string())?;
        store.validate()?;
        let last_project = store.last_project.clone();
        inner.store = store;
        inner.write_allowed = true;
        Ok(last_project)
    }

    pub fn is_write_allowed(&self) -> bool {
        self.inner().write_allowed
    }

    pub fn settings(&self) -> AppSettings {
        self.inner().store.settings.clone()
    }

    pub fn record_settings(&self, settings: AppSettings) -> Result<(), String> {
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Ok(());
        }
        inner.store.settings = settings;
        inner.save_now()
    }

    pub fn record_snapshot(&self, snapshot: &AppSnapshot) -> Result<(), String> {
        let Some(project_root) = snapshot.project_root.clone() else {
            return Ok(());
        };
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Ok(());
        }
        let session = inner
            .store
            .projects
            .remove(&project_root)
            .unwrap_or_else(|| PersistedProjectSession::new(project_root.clone()));
        let mut session = session.with_snapshot(snapshot);
        if let Some(existing) = inner.store.projects.get(&project_root) {
            session.editor_states = existing.editor_states.clone();
            session.sequence_viewports = existing.sequence_viewports.clone();
        }
        inner.store.projects.insert(project_root.clone(), session);
        inner.store.last_project = Some(project_root);
        trim_recent_projects(&mut inner.store);
        inner.save_debounced()
    }

    pub fn record_editor_state(
        &self,
        project_root: &str,
        update: PersistedEditorViewStateUpdate,
    ) -> Result<(), String> {
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Ok(());
        }
        let session = inner
            .store
            .projects
            .entry(project_root.to_string())
            .or_insert_with(|| PersistedProjectSession::new(project_root.to_string()));
        session.editor_states.insert(update.path, update.state);
        inner.save_debounced()
    }

    pub fn record_sequence_viewport(
        &self,
        project_root: &str,
        update: PersistedSequenceViewportStateUpdate,
    ) -> Result<(), String> {
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Ok(());
        }
        let session = inner
            .store
            .projects
            .entry(project_root.to_string())
            .or_insert_with(|| PersistedProjectSession::new(project_root.to_string()));
        session.sequence_viewports.insert(
            sequence_viewport_key(&update.path, &update.object_key),
            update.state,
        );
        inner.save_debounced()
    }

    pub fn restore_for_project(
        &self,
        project_root: &str,
        valid_paths: &std::collections::BTreeSet<String>,
    ) -> Option<ProjectSessionRestore> {
        let inner = self.inner();
        let session = inner.store.projects.get(project_root)?.clone();
        let stale_tabs = session
            .tabs
            .iter()
            .filter(|tab| !valid_paths.contains(&tab.path))
            .map(|tab| tab.path.clone())
            .collect::<Vec<_>>();
        Some(ProjectSessionRestore {
            session,
            stale_tabs,
        })
    }

    pub fn restore_view_state(&self, project_root: &str) -> ProjectRestoreState {
        let inner = self.inner();
        let Some(session) = inner.store.projects.get(project_root) else {
            return ProjectRestoreState {
                editor_states: BTreeMap::new(),
                sequence_viewports: BTreeMap::new(),
            };
        };
        ProjectRestoreState {
            editor_states: session.editor_states.clone(),
            sequence_viewports: session.sequence_viewports.clone(),
        }
    }

    pub fn record_main_window(&self, geometry: PersistedWindowState) -> Result<(), String> {
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Ok(());
        }
        inner.store.main_window = Some(geometry);
        inner.save_now()
    }

    pub fn main_window(&self) -> Option<PersistedWindowState> {
        self.inner().store.main_window.clone()
    }

    pub fn record_preview_window(&self, state: PersistedPreviewWindowState) -> Result<(), String> {
        let mut inner = self.inner();
        if !inner.write_allowed {
            return Ok(());
        }
        inner.store.preview_window = state;
        inner.save_now()
    }

    pub fn preview_window(&self) -> PersistedPreviewWindowState {
        self.inner().store.preview_window.clone()
    }

    fn inner(&self) -> MutexGuard<'_, PersistenceInner> {
        match self.inner.lock() {
            Ok(inner) => inner,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PersistenceInner {
    path: Option<PathBuf>,
    store: PersistedStore,
    write_allowed: bool,
    last_save: Option<Instant>,
}

impl PersistenceInner {
    fn save_debounced(&mut self) -> Result<(), String> {
        if self
            .last_save
            .is_some_and(|last_save| last_save.elapsed() < SAVE_DEBOUNCE)
        {
            return Ok(());
        }
        self.save_now()
    }

    fn save_now(&mut self) -> Result<(), String> {
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        let parent = path
            .parent()
            .ok_or_else(|| "Persistence path has no parent directory.".to_string())?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let text = serde_json::to_string_pretty(&self.store).map_err(|error| error.to_string())?;
        fs::write(path, text).map_err(|error| error.to_string())?;
        self.last_save = Some(Instant::now());
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedStore {
    version: u32,
    #[serde(default)]
    settings: AppSettings,
    last_project: Option<String>,
    projects: BTreeMap<String, PersistedProjectSession>,
    main_window: Option<PersistedWindowState>,
    preview_window: PersistedPreviewWindowState,
}

impl PersistedStore {
    fn validate(&self) -> Result<(), String> {
        if self.version != VERSION {
            return Err(format!(
                "Unsupported desktop state version {}",
                self.version
            ));
        }
        Ok(())
    }
}

impl Default for PersistedStore {
    fn default() -> Self {
        Self {
            version: VERSION,
            settings: AppSettings::default(),
            last_project: None,
            projects: BTreeMap::new(),
            main_window: None,
            preview_window: PersistedPreviewWindowState {
                open: false,
                geometry: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedProjectSession {
    pub project_root: String,
    pub tabs: Vec<PersistedTab>,
    pub active_file: Option<String>,
    pub project_tree_visible: bool,
    pub audio_position_seconds: f64,
    pub audio_home_seconds: f64,
    pub live_output_enabled: bool,
    pub editor_states: BTreeMap<String, PersistedEditorViewState>,
    pub sequence_viewports: BTreeMap<String, PersistedSequenceViewportState>,
}

impl PersistedProjectSession {
    fn new(project_root: String) -> Self {
        Self {
            project_root,
            tabs: Vec::new(),
            active_file: None,
            project_tree_visible: true,
            audio_position_seconds: 0.0,
            audio_home_seconds: 0.0,
            live_output_enabled: false,
            editor_states: BTreeMap::new(),
            sequence_viewports: BTreeMap::new(),
        }
    }

    fn with_snapshot(mut self, snapshot: &AppSnapshot) -> Self {
        self.tabs = snapshot
            .tabs
            .iter()
            .map(|tab| PersistedTab {
                path: tab.path.clone(),
                view_mode: tab.view_mode.clone(),
            })
            .collect();
        self.active_file = snapshot.active_file.clone();
        self.project_tree_visible = snapshot.project_tree_visible;
        self.audio_position_seconds = snapshot.audio_transport.position_seconds;
        self.audio_home_seconds = snapshot.audio_transport.home_seconds;
        self.live_output_enabled = snapshot.live_output.enabled;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PersistedTab {
    pub path: String,
    pub view_mode: EditorViewMode,
}

pub fn read_window_state<R: Runtime>(window: &Window<R>) -> Option<PersistedWindowState> {
    let position = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    Some(PersistedWindowState {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
        maximized: window.is_maximized().unwrap_or(false),
    })
}

pub fn apply_window_state<R: Runtime>(window: &Window<R>, state: &PersistedWindowState) {
    use tauri::{LogicalSize, PhysicalPosition};
    let _ = window.set_size(LogicalSize::new(state.width as f64, state.height as f64));
    let _ = window.set_position(PhysicalPosition::new(state.x, state.y));
    if state.maximized {
        let _ = window.maximize();
    }
}

pub fn sequence_viewport_key(path: &str, object_key: &str) -> String {
    format!("{path}::{object_key}")
}

fn persistence_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|path| path.join(FILE_NAME))
        .map_err(|error| error.to_string())
}

fn trim_recent_projects(store: &mut PersistedStore) {
    if store.projects.len() <= MAX_RECENT_PROJECTS {
        return;
    }
    let protected = store.last_project.clone();
    let remove = store
        .projects
        .keys()
        .filter(|project| Some((*project).as_str()) != protected.as_deref())
        .take(store.projects.len().saturating_sub(MAX_RECENT_PROJECTS))
        .cloned()
        .collect::<Vec<_>>();
    for project in remove {
        store.projects.remove(&project);
    }
}

pub fn valid_project_file(path: &Path) -> bool {
    path.is_file()
}
