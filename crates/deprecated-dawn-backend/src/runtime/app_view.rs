use dawn_language::document::DocumentDescriptor;
use dawn_language::path::Utf8PathBuf;

use crate::editor::BufferTab;
use crate::output::live_output::LiveOutputReadout;
use crate::preview::session::PreviewSnapshot;
use crate::runtime::contracts::RuntimeStatus;
use crate::workspace::ActiveGuiDocument;

use super::AppBackend;

#[derive(Debug, Clone)]
pub struct AppView {
    pub project_root: Option<String>,
    pub project_entries: Vec<dawn_language::fs::WorkspaceEntry>,
    pub analysis: Option<dawn_language::analysis::ProjectAnalysis>,
    pub diagnostics: Vec<dawn_language::analysis::ProjectDiagnostic>,
    pub project_tree_visible: bool,
    pub effect_preview_enabled: bool,
    pub preview: PreviewSnapshot,
    pub live_output: LiveOutputReadout,
    pub tabs: Vec<BufferTab>,
    pub active_file: Option<Utf8PathBuf>,
    pub active_buffer: Option<BufferTab>,
    pub active_document_descriptor: Option<DocumentDescriptor>,
    pub active_gui_document: Option<ActiveGuiDocument>,
    pub status: RuntimeStatus,
}

impl AppBackend {
    pub(crate) fn snapshot(
        &self,
        project_tree_visible: bool,
        effect_preview_enabled: bool,
        live_output: LiveOutputReadout,
    ) -> AppView {
        let active_document_descriptor = self.active_document_descriptor();
        let active_buffer = self.document_store.active_tab();
        let active_gui_document = self.workspace.active_gui_document(
            active_buffer.as_ref(),
            active_document_descriptor.as_ref(),
            self.document_store.dirty_overlays(),
        );
        AppView {
            project_root: self.workspace.project_root(),
            project_entries: self.workspace.project_entries(),
            analysis: self.workspace.analysis_cloned(),
            diagnostics: self.workspace.diagnostics_cloned(),
            project_tree_visible,
            effect_preview_enabled,
            preview: self.preview.snapshot(),
            live_output,
            tabs: self.document_store.tabs(),
            active_file: self.document_store.active_file().cloned(),
            active_buffer,
            active_document_descriptor,
            active_gui_document,
            status: self.status.clone(),
        }
    }
}
