use crate::prefs::WindowLayout;

use super::AppBackend;

impl AppBackend {
    pub(super) fn last_project_root(&self) -> Option<std::path::PathBuf> {
        self.app_prefs.last_project_root()
    }

    pub(super) fn remember_project_root(&mut self, path: std::path::PathBuf) -> Result<(), String> {
        self.app_prefs.remember_project_root(path)
    }

    pub(super) fn toggle_project_tree_command(&mut self) -> Result<(), String> {
        self.app_prefs.toggle_project_tree()
    }

    pub(super) fn effect_preview_enabled(&self) -> bool {
        self.app_prefs.effect_preview_enabled()
    }

    pub(super) fn set_effect_preview_enabled_command(
        &mut self,
        enabled: bool,
    ) -> Result<(), String> {
        self.app_prefs.set_effect_preview_enabled(enabled)?;
        self.sync_effect_preview_enabled(enabled)
    }

    pub(super) fn set_effect_preview_effects_command(&mut self, ids: Vec<u32>) {
        self.sync_effect_preview_effects(ids, self.app_prefs.effect_preview_enabled());
    }

    pub(super) fn preview_window_should_open_command(&self) -> bool {
        self.app_prefs.preview_window_open()
    }

    pub(super) fn preview_window_layout_command(&self) -> WindowLayout {
        self.app_prefs.preview_window_layout()
    }

    pub(super) fn main_window_layout_command(&self) -> WindowLayout {
        self.app_prefs.main_window_layout()
    }

    pub(super) fn set_main_window_layout_command(
        &mut self,
        layout: WindowLayout,
    ) -> Result<(), String> {
        self.app_prefs.set_main_window_layout(layout)
    }

    pub(super) fn set_preview_window_layout_command(
        &mut self,
        layout: WindowLayout,
    ) -> Result<(), String> {
        self.app_prefs.set_preview_window_layout(layout)
    }

    pub(super) fn set_preview_window_open_command(&mut self, open: bool) -> Result<(), String> {
        self.app_prefs.set_preview_window_open(open)
    }
}
