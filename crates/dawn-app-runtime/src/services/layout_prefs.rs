use crate::contracts::Revision;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutPrefsCore {
    pub project_tree_visible: bool,
    pub preview_window_open: bool,
    pub revision: Revision,
}

impl Default for LayoutPrefsCore {
    fn default() -> Self {
        Self {
            project_tree_visible: true,
            preview_window_open: false,
            revision: Revision::INITIAL,
        }
    }
}

impl LayoutPrefsCore {
    pub fn set_project_tree_visible(&mut self, visible: bool) {
        if self.project_tree_visible != visible {
            self.project_tree_visible = visible;
            self.revision = self.revision.next();
        }
    }

    pub fn set_preview_window_open(&mut self, open: bool) {
        if self.preview_window_open != open {
            self.preview_window_open = open;
            self.revision = self.revision.next();
        }
    }
}
