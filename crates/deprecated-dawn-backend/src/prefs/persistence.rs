use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const DEFAULT_LEFT_PANE_WIDTH: f64 = 280.0;
const DEFAULT_RIGHT_PANE_WIDTH: f64 = 360.0;

fn default_preview_window_open() -> bool {
    true
}

fn default_main_window_layout() -> WindowLayout {
    WindowLayout::main_default()
}

fn default_preview_window_layout() -> WindowLayout {
    WindowLayout::preview_default()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WorkbenchLayout {
    pub(super) project_tree_visible: bool,
    pub(super) inspector_visible: bool,
    pub(super) project_tree_width: f64,
    pub(super) inspector_width: f64,
    #[serde(default)]
    pub(super) active_inspector_tab: InspectorTab,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) last_project_root: Option<PathBuf>,
    #[serde(default = "default_preview_window_open")]
    pub(super) preview_window_open: bool,
    #[serde(default = "default_main_window_layout")]
    pub(super) main_window: WindowLayout,
    #[serde(default = "default_preview_window_layout")]
    pub(super) preview_window: WindowLayout,
    #[serde(default)]
    pub(super) effect_preview_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) enum InspectorTab {
    #[default]
    Diagnostics,
    Preview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowLayout {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub maximized: bool,
}

impl WindowLayout {
    pub(crate) fn main_default() -> Self {
        Self {
            x: 40.0,
            y: 40.0,
            width: 1280.0,
            height: 820.0,
            maximized: false,
        }
    }

    pub(crate) fn preview_default() -> Self {
        Self {
            x: 80.0,
            y: 80.0,
            width: 720.0,
            height: 480.0,
            maximized: false,
        }
    }
}

impl Default for WindowLayout {
    fn default() -> Self {
        Self::preview_default()
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
            preview_window_open: true,
            main_window: WindowLayout::main_default(),
            preview_window: WindowLayout::preview_default(),
            effect_preview_enabled: false,
        }
    }
}

pub(super) fn load_workbench_layout() -> WorkbenchLayout {
    let Some(path) = config_path() else {
        return WorkbenchLayout::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

pub(super) fn save_workbench_layout(layout: &WorkbenchLayout) -> Result<(), String> {
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
