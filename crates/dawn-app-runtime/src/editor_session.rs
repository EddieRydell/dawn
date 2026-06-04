use dawn_language::analysis::ProjectOverlay;
use dawn_language::path::Utf8PathBuf;

use crate::services::editor_state::{
    BufferExternalState, BufferTab, EditorStore, EditorViewMode, FileVersion,
};

#[derive(Debug, Clone)]
pub struct SessionBufferState {
    pub path: Utf8PathBuf,
    pub text: String,
    pub disk_version: FileVersion,
    pub view_mode: EditorViewMode,
}

#[derive(Debug, Default)]
pub struct EditorSession {
    store: EditorStore,
}

impl EditorSession {
    pub fn restore(&mut self, buffers: Vec<SessionBufferState>, active_file: Option<Utf8PathBuf>) {
        self.store.restore(
            buffers
                .into_iter()
                .map(|buffer| {
                    (
                        buffer.path,
                        buffer.text,
                        buffer.disk_version,
                        buffer.view_mode,
                    )
                })
                .collect(),
            active_file,
        );
    }

    pub fn clear(&mut self) {
        self.store.clear();
    }

    pub fn open_file(
        &mut self,
        path: Utf8PathBuf,
        text: String,
        disk_version: FileVersion,
        view_mode: EditorViewMode,
    ) {
        self.store
            .open_file_with_view_mode(path, text, disk_version, view_mode);
    }

    pub fn close_file(&mut self, path: &Utf8PathBuf) {
        self.store.close_file(path);
    }

    pub fn set_active_file(&mut self, path: Utf8PathBuf) {
        self.store.set_active_file(path);
    }

    pub fn set_active_view_mode(&mut self, mode: EditorViewMode) {
        if let Some(path) = self.store.active_file().cloned() {
            self.store.set_view_mode(&path, mode);
        }
    }

    pub fn update_active_text(&mut self, text: String) {
        self.store.update_active_text(text);
    }

    pub fn replace_active_text_from_runtime(&mut self, text: String) {
        self.store.replace_active_text_from_runtime(text);
    }

    pub fn replace_active_text_from_gui(&mut self, text: String) {
        self.store.replace_active_text_from_gui(text);
    }

    pub fn tabs(&self) -> Vec<BufferTab> {
        self.store.tabs()
    }

    pub fn active_file(&self) -> Option<&Utf8PathBuf> {
        self.store.active_file()
    }

    pub fn active_path(&self) -> Result<Utf8PathBuf, String> {
        self.active_file()
            .cloned()
            .ok_or_else(|| "no active document".to_string())
    }

    pub fn active_buffer(&self) -> Option<&BufferTab> {
        self.store.active_buffer()
    }

    pub fn active_text(&self) -> Result<String, String> {
        self.active_buffer()
            .map(|buffer| buffer.text.clone())
            .ok_or_else(|| "no active document".to_string())
    }

    pub fn ensure_active_buffer_not_conflicted(&self) -> Result<(), String> {
        let Some(buffer) = self.active_buffer() else {
            return Ok(());
        };
        if buffer.is_conflicted() {
            return Err("active document has external disk changes".to_string());
        }
        Ok(())
    }

    pub fn dirty_overlays(&self) -> Vec<ProjectOverlay> {
        self.store.dirty_overlays()
    }

    pub fn dirty_buffers(&self) -> Vec<BufferTab> {
        self.store.dirty_buffers()
    }

    pub fn dirty_autosave_buffers(&self) -> Vec<BufferTab> {
        self.store.dirty_autosave_buffers()
    }

    pub fn buffers(&self) -> Vec<BufferTab> {
        self.store.buffers()
    }

    pub fn record_saved_version(&mut self, path: &Utf8PathBuf, disk_version: FileVersion) {
        self.store.record_saved_version(path, disk_version);
    }

    pub fn replace_from_disk(
        &mut self,
        path: &Utf8PathBuf,
        text: String,
        disk_version: FileVersion,
        preserve_undo: bool,
    ) {
        self.store
            .replace_from_disk(path, text, disk_version, preserve_undo);
    }

    pub fn mark_external_state(&mut self, path: &Utf8PathBuf, state: BufferExternalState) {
        self.store.mark_external_state(path, state);
    }

    pub fn reconcile_moved_paths(&mut self, moves: &[(Utf8PathBuf, Utf8PathBuf)]) {
        self.store.reconcile_moved_paths(moves);
    }

    pub fn reconcile_deleted_path(&mut self, deleted_path: &Utf8PathBuf) {
        self.store.reconcile_deleted_path(deleted_path);
    }
}
