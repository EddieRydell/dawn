use dawn_backend::AppView;
use dawn_language::analysis::{
    DiagnosticCode, DiagnosticSeverity, ProjectAnalysis, ProjectDiagnostic, TextRange,
};
use dawn_language::path::{PathStringExt, Utf8PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub(crate) enum AppCommandDto {
    OpenProjectDialog,
    OpenProject {
        path: String,
    },
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub(crate) enum AppCommandResponseDto {
    None,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppBackendChangedDto {
    pub(crate) snapshot: AppSnapshotDto,
    pub(crate) changed_slices: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppSnapshotDto {
    pub(crate) workspace: WorkspaceReadModelDto,
    pub(crate) editor: EditorReadModelDto,
    pub(crate) active_document: ActiveDocumentReadModelDto,
    pub(crate) diagnostics: DiagnosticsReadModelDto,
    pub(crate) preview: PreviewReadModelDto,
    pub(crate) live_output: LiveOutputReadModelDto,
    pub(crate) status: StatusReadModelDto,
    pub(crate) prefs: PrefsReadModelDto,
}

impl Default for AppSnapshotDto {
    fn default() -> Self {
        Self::from(AppView::default())
    }
}

impl From<AppView> for AppSnapshotDto {
    fn from(view: AppView) -> Self {
        let diagnostics = view
            .analysis
            .as_ref()
            .map(project_diagnostics)
            .unwrap_or_default();
        let status = status_from_view(&view);
        Self {
            workspace: WorkspaceReadModelDto {
                project_root: view.project_root,
                project_tree_visible: true,
                project_entries: Vec::new(),
            },
            editor: EditorReadModelDto::default(),
            active_document: ActiveDocumentReadModelDto::default(),
            diagnostics: DiagnosticsReadModelDto { diagnostics },
            preview: PreviewReadModelDto::default(),
            live_output: LiveOutputReadModelDto::default(),
            status: StatusReadModelDto { status },
            prefs: PrefsReadModelDto {
                project_tree_visible: true,
                effect_preview_enabled: false,
            },
        }
    }
}

fn status_from_view(view: &AppView) -> RuntimeStatusDto {
    if view.project_root.is_none() {
        return RuntimeStatusDto::NoProjectOpen;
    }
    match &view.analysis {
        Some(analysis) if analysis.has_errors() => RuntimeStatusDto::Message {
            message: "Project has diagnostics".to_string(),
        },
        Some(_) => RuntimeStatusDto::Saved,
        None => RuntimeStatusDto::Message {
            message: "Analyzing project".to_string(),
        },
    }
}

fn project_diagnostics(analysis: &ProjectAnalysis) -> Vec<ProjectDiagnosticDto> {
    analysis
        .diagnostics
        .iter()
        .map(ProjectDiagnosticDto::from)
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceReadModelDto {
    pub(crate) project_root: Option<String>,
    pub(crate) project_tree_visible: bool,
    pub(crate) project_entries: Vec<WorkspaceEntryDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceEntryDto {
    pub(crate) path: String,
    pub(crate) kind: WorkspaceEntryKindDto,
    pub(crate) name: String,
    pub(crate) parent: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub(crate) enum WorkspaceEntryKindDto {
    Directory,
    File,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorReadModelDto {
    pub(crate) tabs: Vec<EditorBufferDto>,
    pub(crate) active_file: Option<String>,
    pub(crate) active_buffer: Option<EditorBufferDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorBufferDto {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) text: String,
    pub(crate) dirty: bool,
    pub(crate) external_state: BufferExternalStateDto,
    pub(crate) view_mode: EditorViewModeDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub(crate) enum BufferExternalStateDto {
    Current,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub(crate) enum EditorViewModeDto {
    Text,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActiveDocumentReadModelDto {
    pub(crate) descriptor: Option<serde_json::Value>,
    pub(crate) gui_document: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiagnosticsReadModelDto {
    pub(crate) diagnostics: Vec<ProjectDiagnosticDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectDiagnosticDto {
    pub(crate) path: String,
    pub(crate) range: Option<TextRangeDto>,
    pub(crate) severity: DiagnosticSeverityDto,
    pub(crate) code: String,
    pub(crate) message: String,
}

impl From<&ProjectDiagnostic> for ProjectDiagnosticDto {
    fn from(diagnostic: &ProjectDiagnostic) -> Self {
        Self {
            path: display_path(&diagnostic.path),
            range: diagnostic.range.map(TextRangeDto::from),
            severity: DiagnosticSeverityDto::from(diagnostic.severity),
            code: diagnostic_code(diagnostic.code).to_string(),
            message: diagnostic.message.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TextRangeDto {
    pub(crate) start: TextPositionDto,
    pub(crate) end: TextPositionDto,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TextPositionDto {
    pub(crate) line: u32,
    pub(crate) character: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DiagnosticSeverityDto {
    Error,
    Warning,
}

impl From<DiagnosticSeverity> for DiagnosticSeverityDto {
    fn from(severity: DiagnosticSeverity) -> Self {
        match severity {
            DiagnosticSeverity::Error => Self::Error,
            DiagnosticSeverity::Warning => Self::Warning,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewReadModelDto {
    pub(crate) preview: PreviewSnapshotDto,
    pub(crate) effect_preview_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewSnapshotDto {
    pub(crate) source_label: String,
    pub(crate) is_playing: bool,
    pub(crate) preview_updating: bool,
    pub(crate) effect_preview_active: bool,
    pub(crate) position_seconds: f64,
    pub(crate) home_seconds: f64,
    pub(crate) duration_seconds: f64,
    pub(crate) audio: Option<serde_json::Value>,
    pub(crate) clock_source: String,
    pub(crate) audio_playback_status: AudioPlaybackStatus,
    pub(crate) status: String,
}

impl Default for PreviewSnapshotDto {
    fn default() -> Self {
        Self {
            source_label: "No sequence".to_string(),
            is_playing: false,
            preview_updating: false,
            effect_preview_active: false,
            position_seconds: 0.0,
            home_seconds: 0.0,
            duration_seconds: 0.0,
            audio: None,
            clock_source: "none".to_string(),
            audio_playback_status: AudioPlaybackStatus::None,
            status: "Idle".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AudioPlaybackStatus {
    None,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LiveOutputReadModelDto {
    pub(crate) live_output: OutputReadoutDto,
}

impl Default for LiveOutputReadModelDto {
    fn default() -> Self {
        Self {
            live_output: OutputReadoutDto {
                enabled: false,
                status: "disabled".to_string(),
                active_universe_count: 0,
                last_error: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OutputReadoutDto {
    pub(crate) enabled: bool,
    pub(crate) status: String,
    pub(crate) active_universe_count: u32,
    pub(crate) last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatusReadModelDto {
    pub(crate) status: RuntimeStatusDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub(crate) enum RuntimeStatusDto {
    NoProjectOpen,
    Saved,
    Message { message: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrefsReadModelDto {
    pub(crate) project_tree_visible: bool,
    pub(crate) effect_preview_enabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceEffectPreviewResultsDto {
    pub(crate) results: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewSceneDto {
    pub(crate) generation: u64,
    pub(crate) source_label: String,
    pub(crate) bounds: GeometryRenderBoundsDto,
    pub(crate) pixel_count: u32,
    pub(crate) fixtures: Vec<serde_json::Value>,
}

impl Default for PreviewSceneDto {
    fn default() -> Self {
        Self {
            generation: 0,
            source_label: "No sequence".to_string(),
            bounds: GeometryRenderBoundsDto::default(),
            pixel_count: 0,
            fixtures: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeometryRenderBoundsDto {
    pub(crate) min_x_meters: f64,
    pub(crate) min_y_meters: f64,
    pub(crate) max_x_meters: f64,
    pub(crate) max_y_meters: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PreviewTransportMode {
    Unsupported,
}

fn display_path(path: &Utf8PathBuf) -> String {
    path.to_slash_string()
}

fn diagnostic_code(code: DiagnosticCode) -> &'static str {
    match code {
        DiagnosticCode::Io => "io",
        DiagnosticCode::Yaml => "yaml",
        DiagnosticCode::Import => "import",
        DiagnosticCode::Lower => "lower",
        DiagnosticCode::ProjectKey => "project_key",
        DiagnosticCode::Sequence => "sequence",
        DiagnosticCode::Script => "script",
    }
}
