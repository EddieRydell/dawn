use crate::editor::EditorView;

#[derive(Debug, Clone, Default)]
pub struct AppView {
    pub project_root: Option<String>,
    pub project_file: Option<String>,
    pub analysis: Option<dawn_language::analysis::ProjectAnalysis>,
    pub editor: EditorView,
}
