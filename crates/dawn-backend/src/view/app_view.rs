use crate::editor::EditorView;
use crate::types::{ActiveDocumentView, RenderView, WorkspaceEntry};

#[derive(Debug, Clone, Default)]
pub struct AppView {
    pub project_root: Option<String>,
    pub project_file: Option<String>,
    pub project_entries: Vec<WorkspaceEntry>,
    pub analysis: Option<dawn_language::analysis::ProjectAnalysis>,
    pub editor: EditorView,
    pub active_document: ActiveDocumentView,
    pub render: RenderView,
}
