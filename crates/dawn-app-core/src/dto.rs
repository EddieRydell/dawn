use dawn_project::analysis::{DiagnosticSeverity, ProjectDiagnostic, TextRange};
use dawn_project::fs::{WorkspaceEntry, WorkspaceEntryKind};
use dawn_project::path::PathStringExt;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::app_model::AppSnapshot;
use crate::editor_session::{EditorBuffer, EditorViewMode};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshotDto {
    pub project_root: Option<String>,
    pub project_tree_visible: bool,
    pub project_entries: Vec<WorkspaceEntryDto>,
    pub tabs: Vec<EditorBufferDto>,
    pub active_file: Option<String>,
    pub active_buffer: Option<EditorBufferDto>,
    pub diagnostics: Vec<ProjectDiagnosticDto>,
    pub status: String,
    pub preview: PreviewSnapshotDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEntryDto {
    pub path: String,
    pub kind: WorkspaceEntryKindDto,
    pub name: String,
    pub parent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceEntryKindDto {
    Directory,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EditorBufferDto {
    pub path: String,
    pub name: String,
    pub text: String,
    pub dirty: bool,
    pub view_mode: EditorViewModeDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum EditorViewModeDto {
    Text,
    Gui,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDiagnosticDto {
    pub path: String,
    pub range: Option<TextRangeDto>,
    pub severity: DiagnosticSeverityDto,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverityDto {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TextRangeDto {
    pub start: TextPositionDto,
    pub end: TextPositionDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TextPositionDto {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSnapshotDto {
    pub source_label: String,
    pub is_playing: bool,
    pub position_ms: u32,
    pub duration_ms: u32,
    pub status: String,
}

impl From<AppSnapshot> for AppSnapshotDto {
    fn from(snapshot: AppSnapshot) -> Self {
        Self {
            project_root: snapshot.project_root,
            project_tree_visible: snapshot.workbench_layout.project_tree_visible,
            project_entries: snapshot
                .project_entries
                .into_iter()
                .map(WorkspaceEntryDto::from)
                .collect(),
            tabs: snapshot
                .tabs
                .into_iter()
                .map(EditorBufferDto::from)
                .collect(),
            active_file: snapshot.active_file.map(|path| path.to_slash_string()),
            active_buffer: snapshot.active_buffer.map(EditorBufferDto::from),
            diagnostics: snapshot
                .diagnostics
                .into_iter()
                .map(ProjectDiagnosticDto::from)
                .collect(),
            status: snapshot.status,
            preview: PreviewSnapshotDto {
                source_label: snapshot.preview.source_label,
                is_playing: snapshot.preview.is_playing,
                position_ms: snapshot.preview.position_ms.min(u32::MAX as u64) as u32,
                duration_ms: snapshot.preview.duration_ms.min(u32::MAX as u64) as u32,
                status: snapshot.preview.status,
            },
        }
    }
}

impl From<WorkspaceEntry> for WorkspaceEntryDto {
    fn from(entry: WorkspaceEntry) -> Self {
        let parent = entry
            .path
            .parent()
            .map(|path| path.to_slash_string())
            .unwrap_or_default();
        let name = entry
            .path
            .file_name()
            .map(ToString::to_string)
            .unwrap_or_else(|| entry.path.to_slash_string());
        Self {
            path: entry.path.to_slash_string(),
            kind: match entry.kind {
                WorkspaceEntryKind::Directory => WorkspaceEntryKindDto::Directory,
                WorkspaceEntryKind::File => WorkspaceEntryKindDto::File,
            },
            name,
            parent,
        }
    }
}

impl From<EditorBuffer> for EditorBufferDto {
    fn from(buffer: EditorBuffer) -> Self {
        let dirty = buffer.is_dirty();
        let name = buffer
            .path
            .file_name()
            .map(ToString::to_string)
            .unwrap_or_else(|| buffer.path.to_slash_string());
        Self {
            path: buffer.path.to_slash_string(),
            name,
            text: buffer.text,
            dirty,
            view_mode: match buffer.view_mode {
                EditorViewMode::Text => EditorViewModeDto::Text,
                EditorViewMode::Gui => EditorViewModeDto::Gui,
            },
        }
    }
}

impl From<ProjectDiagnostic> for ProjectDiagnosticDto {
    fn from(diagnostic: ProjectDiagnostic) -> Self {
        Self {
            path: diagnostic.path.to_slash_string(),
            range: diagnostic.range.map(TextRangeDto::from),
            severity: match diagnostic.severity {
                DiagnosticSeverity::Error => DiagnosticSeverityDto::Error,
                DiagnosticSeverity::Warning => DiagnosticSeverityDto::Warning,
            },
            code: format!("{:?}", diagnostic.code),
            message: diagnostic.message,
        }
    }
}

impl From<TextRange> for TextRangeDto {
    fn from(range: TextRange) -> Self {
        Self {
            start: TextPositionDto {
                line: range.start.line,
                character: range.start.character,
            },
            end: TextPositionDto {
                line: range.end.line,
                character: range.end.character,
            },
        }
    }
}
