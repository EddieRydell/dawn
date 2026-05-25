use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ui::theme;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelLayout {
    pub left_visible: bool,
    pub right_visible: bool,
    pub left_width: f64,
    pub right_width: f64,
    #[serde(default)]
    pub active_right_tab: RightPaneTab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum RightPaneTab {
    #[default]
    Diagnostics,
    Preview,
}

impl Default for PanelLayout {
    fn default() -> Self {
        Self {
            left_visible: true,
            right_visible: true,
            left_width: theme::DEFAULT_LEFT_PANE_WIDTH,
            right_width: theme::DEFAULT_RIGHT_PANE_WIDTH,
            active_right_tab: RightPaneTab::Diagnostics,
        }
    }
}

impl PanelLayout {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

pub fn load_panel_layout() -> PanelLayout {
    let Some(path) = config_path() else {
        return PanelLayout::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

pub fn save_panel_layout(layout: &PanelLayout) -> Result<(), String> {
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
