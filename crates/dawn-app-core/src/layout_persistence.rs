use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::editor_session::EditorSessionState;

const DEFAULT_LEFT_PANE_WIDTH: f64 = 280.0;
const DEFAULT_RIGHT_PANE_WIDTH: f64 = 360.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchLayout {
    pub project_tree_visible: bool,
    pub inspector_visible: bool,
    pub project_tree_width: f64,
    pub inspector_width: f64,
    #[serde(default)]
    pub active_inspector_tab: InspectorTab,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_project_root: Option<PathBuf>,
    #[serde(default)]
    pub editor_session: EditorSessionState,
    #[serde(default)]
    pub preview_window: PreviewWindowLayout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum InspectorTab {
    #[default]
    Diagnostics,
    Preview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewWindowLayout {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Default for PreviewWindowLayout {
    fn default() -> Self {
        Self {
            x: 80.0,
            y: 80.0,
            width: 720.0,
            height: 480.0,
        }
    }
}

impl Default for WorkbenchLayout {
    fn default() -> Self {
        Self {
            project_tree_visible: true,
            inspector_visible: false,
            project_tree_width: DEFAULT_LEFT_PANE_WIDTH,
            inspector_width: DEFAULT_RIGHT_PANE_WIDTH,
            active_inspector_tab: InspectorTab::Diagnostics,
            last_project_root: None,
            editor_session: EditorSessionState::default(),
            preview_window: PreviewWindowLayout::default(),
        }
    }
}

impl WorkbenchLayout {
    pub fn reset(&mut self) {
        let last_project_root = self.last_project_root.clone();
        let editor_session = self.editor_session.clone();
        *self = Self {
            last_project_root,
            editor_session,
            preview_window: self.preview_window.clone(),
            ..Self::default()
        };
    }
}

pub fn load_workbench_layout() -> WorkbenchLayout {
    let Some(path) = config_path() else {
        return WorkbenchLayout::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

pub fn save_workbench_layout(layout: &WorkbenchLayout) -> Result<(), String> {
    let path = config_path().ok_or_else(|| "could not resolve config directory".to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let content = serde_json::to_string_pretty(layout).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("dawn").join("workbench.json"))
}
