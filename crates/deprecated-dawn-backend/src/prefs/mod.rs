mod persistence;

use std::path::PathBuf;

use crate::runtime::contracts::Revision;
use persistence::{load_workbench_layout, save_workbench_layout, WorkbenchLayout};

pub use persistence::WindowLayout;

#[derive(Debug, Clone)]
pub(crate) struct AppPrefs {
    layout: WorkbenchLayout,
    pub(crate) revision: Revision,
}

impl Default for AppPrefs {
    fn default() -> Self {
        Self {
            layout: load_workbench_layout(),
            revision: Revision::INITIAL,
        }
    }
}

impl AppPrefs {
    pub(crate) fn project_tree_visible(&self) -> bool {
        self.layout.project_tree_visible
    }

    pub(crate) fn preview_window_open(&self) -> bool {
        self.layout.preview_window_open
    }

    pub(crate) fn effect_preview_enabled(&self) -> bool {
        self.layout.effect_preview_enabled
    }

    pub(crate) fn main_window_layout(&self) -> WindowLayout {
        self.layout.main_window.clone()
    }

    pub(crate) fn preview_window_layout(&self) -> WindowLayout {
        self.layout.preview_window.clone()
    }

    pub(crate) fn last_project_root(&self) -> Option<PathBuf> {
        self.layout.last_project_root.clone()
    }

    pub(crate) fn remember_project_root(&mut self, path: PathBuf) -> Result<(), String> {
        self.layout.last_project_root = Some(path);
        self.persist()
    }

    pub(crate) fn toggle_project_tree(&mut self) -> Result<(), String> {
        self.layout.project_tree_visible = !self.layout.project_tree_visible;
        self.persist()
    }

    pub(crate) fn set_preview_window_open(&mut self, open: bool) -> Result<(), String> {
        if self.layout.preview_window_open != open {
            self.layout.preview_window_open = open;
            self.persist()?;
        }
        Ok(())
    }

    pub(crate) fn set_effect_preview_enabled(&mut self, enabled: bool) -> Result<(), String> {
        if self.layout.effect_preview_enabled != enabled {
            self.layout.effect_preview_enabled = enabled;
            self.persist()?;
        }
        Ok(())
    }

    pub(crate) fn set_main_window_layout(&mut self, layout: WindowLayout) -> Result<(), String> {
        self.layout.main_window = layout;
        self.persist()
    }

    pub(crate) fn set_preview_window_layout(&mut self, layout: WindowLayout) -> Result<(), String> {
        self.layout.preview_window = layout;
        self.persist()
    }

    fn persist(&mut self) -> Result<(), String> {
        save_workbench_layout(&self.layout)?;
        self.revision = self.revision.next();
        Ok(())
    }
}
