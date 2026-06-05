use dawn_language::analysis::{ProjectAnalysis, ProjectDiagnostic, ProjectOverlay};
use dawn_language::document::{
    DocumentDescriptor, DocumentEditOutcome, FixtureDocument, LayoutDocument, SequenceDocument,
    SequenceDocumentEdit,
};
use dawn_language::fs::WorkspaceEntry;
use dawn_language::path::Utf8PathBuf;

use crate::editor::{BufferTab, FileVersion};
use crate::workspace::ActiveGuiDocument;
use crate::workspace::ProjectWorkspace;

#[derive(Debug, Clone)]
pub struct CreatedRuntimeFile {
    pub path: Utf8PathBuf,
    pub text: String,
    pub disk_version: FileVersion,
}

#[derive(Debug, Default)]
pub struct WorkspaceSession {
    workspace: ProjectWorkspace,
    project_root: Option<String>,
    project_entries: Vec<WorkspaceEntry>,
    analysis: Option<ProjectAnalysis>,
    diagnostics: Vec<ProjectDiagnostic>,
}

impl WorkspaceSession {
    pub fn open_project(&mut self, path: impl AsRef<std::path::Path>) -> Result<(), String> {
        self.workspace.open_project(path)?;
        self.refresh_project_entries()
    }

    pub fn refresh_project_entries(&mut self) -> Result<(), String> {
        self.project_root = self
            .workspace
            .project_root_display()
            .map(ToString::to_string);
        self.project_entries = self.workspace.project_entries()?;
        Ok(())
    }

    pub fn refresh_analysis(&mut self, overlays: Vec<ProjectOverlay>) -> Result<(), String> {
        let analysis = self.workspace.analyze(overlays)?;
        self.diagnostics = analysis.diagnostics.clone();
        self.analysis = Some(analysis);
        Ok(())
    }

    pub fn refresh_analysis_from_overlays(
        &mut self,
        overlays: Vec<ProjectOverlay>,
    ) -> Result<(), String> {
        self.refresh_analysis(overlays)
    }

    pub fn project_root(&self) -> Option<String> {
        self.project_root.clone()
    }

    pub fn project_entries(&self) -> Vec<WorkspaceEntry> {
        self.project_entries.clone()
    }

    pub fn analysis(&self) -> Option<&ProjectAnalysis> {
        self.analysis.as_ref()
    }

    pub fn analysis_cloned(&self) -> Option<ProjectAnalysis> {
        self.analysis.clone()
    }

    pub fn diagnostics(&self) -> &[ProjectDiagnostic] {
        &self.diagnostics
    }

    pub fn diagnostics_cloned(&self) -> Vec<ProjectDiagnostic> {
        self.diagnostics.clone()
    }

    pub fn inspect_document(
        &self,
        path: Utf8PathBuf,
        overlays: Vec<ProjectOverlay>,
    ) -> Result<DocumentDescriptor, String> {
        self.workspace.inspect_document(path, overlays)
    }

    pub fn layout_document(
        &self,
        path: Utf8PathBuf,
        object_key: &str,
        overlays: Vec<ProjectOverlay>,
    ) -> Result<LayoutDocument, String> {
        self.workspace.layout_document(path, object_key, overlays)
    }

    pub fn fixture_document(
        &self,
        path: Utf8PathBuf,
        selected_object_key: Option<&str>,
        overlays: Vec<ProjectOverlay>,
    ) -> Result<FixtureDocument, String> {
        self.workspace
            .fixture_document(path, selected_object_key, overlays)
    }

    pub fn sequence_document(
        &self,
        path: Utf8PathBuf,
        object_key: &str,
        overlays: Vec<ProjectOverlay>,
    ) -> Result<SequenceDocument, String> {
        self.workspace.sequence_document(path, object_key, overlays)
    }

    pub fn apply_layout_edit(
        &self,
        path: Utf8PathBuf,
        object_key: &str,
        document: LayoutDocument,
        base_content: String,
        overlays: Vec<ProjectOverlay>,
    ) -> Result<DocumentEditOutcome<LayoutDocument>, String> {
        self.workspace
            .apply_layout_edit(path, object_key, document, base_content, overlays)
    }

    pub fn apply_fixture_edit(
        &self,
        path: Utf8PathBuf,
        document: FixtureDocument,
        base_content: String,
        overlays: Vec<ProjectOverlay>,
    ) -> Result<DocumentEditOutcome<FixtureDocument>, String> {
        self.workspace
            .apply_fixture_edit(path, document, base_content, overlays)
    }

    pub fn apply_sequence_edit(
        &self,
        path: Utf8PathBuf,
        object_key: &str,
        edit: SequenceDocumentEdit,
        base_content: String,
        overlays: Vec<ProjectOverlay>,
    ) -> Result<DocumentEditOutcome<SequenceDocument>, String> {
        let analysis = self
            .analysis
            .as_ref()
            .ok_or_else(|| "project analysis is not available".to_string())?;
        self.workspace
            .apply_sequence_edit(path, object_key, edit, base_content, overlays, analysis)
    }

    pub fn read_file_with_version(
        &self,
        path: Utf8PathBuf,
    ) -> Result<(String, FileVersion), String> {
        self.workspace.read_file_with_version(path)
    }

    pub fn write_text_file_with_version(
        &self,
        path: Utf8PathBuf,
        content: &str,
    ) -> Result<FileVersion, String> {
        self.workspace.write_text_file_with_version(path, content)
    }

    pub fn flush_autosave_buffers(
        &self,
        buffers: Vec<BufferTab>,
    ) -> Result<Vec<(Utf8PathBuf, FileVersion)>, String> {
        let mut saved = Vec::new();
        for buffer in buffers {
            let version = self
                .workspace
                .write_text_file_with_version(buffer.path.clone(), &buffer.text)?;
            saved.push((buffer.path, version));
        }
        Ok(saved)
    }

    pub fn create_file_for_runtime_open(
        &mut self,
        parent: Utf8PathBuf,
        name: &str,
    ) -> Result<CreatedRuntimeFile, String> {
        let path = self.workspace.create_file(parent, name)?;
        self.refresh_project_entries()?;
        let (text, disk_version) = self.workspace.read_file_with_version(path.clone())?;
        Ok(CreatedRuntimeFile {
            path,
            text,
            disk_version,
        })
    }

    pub fn create_directory(&mut self, parent: Utf8PathBuf, name: &str) -> Result<(), String> {
        self.workspace.create_directory(parent, name)?;
        self.refresh_project_entries()
    }

    pub fn rename_path(
        &mut self,
        path: Utf8PathBuf,
        new_name: &str,
    ) -> Result<Vec<(Utf8PathBuf, Utf8PathBuf)>, String> {
        let moves = self.workspace.rename_path(path, new_name)?;
        self.refresh_project_entries()?;
        Ok(moves)
    }

    pub fn delete_path(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        self.workspace.delete_path(path)?;
        self.refresh_project_entries()
    }

    pub fn active_gui_document(
        &self,
        active_buffer: Option<&BufferTab>,
        active_document_descriptor: Option<&DocumentDescriptor>,
        overlays: Vec<ProjectOverlay>,
    ) -> Option<ActiveGuiDocument> {
        crate::workspace::build_active_gui_document(
            &self.workspace,
            active_buffer,
            &self.diagnostics,
            active_document_descriptor,
            overlays,
        )
    }
}

pub fn load_project_workspace(path: &std::path::Path) -> Result<ProjectWorkspace, String> {
    let mut workspace = ProjectWorkspace::new();
    workspace.open_project(path)?;
    Ok(workspace)
}

pub fn project_root_label_for_path(path: &std::path::Path) -> Result<String, String> {
    let workspace = load_project_workspace(path)?;
    workspace
        .project_root_display()
        .map(ToString::to_string)
        .ok_or_else(|| "project root was not opened".to_string())
}
