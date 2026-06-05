// TypeScript-facing shapes and conversions only; no fabricated app state or policy.
use dawn_backend::{
    ActiveDocumentView, ActiveGuiDocument, AppView, EditorTabView, EditorView,
    EditorViewMode as BackendEditorViewMode, LoadedEditorTabView, PreviewRenderTiming,
    PreviewSnapshot, RenderedFixtureFrame, RenderedFrame, SequenceEffectPreviewResult,
    WorkspaceEntry, WorkspaceEntryKind,
};
use dawn_language::analysis::{
    DiagnosticCode, DiagnosticSeverity, ProjectAnalysis, ProjectDiagnostic, TextRange,
};
use dawn_language::document::{
    DocumentDescriptor, DocumentEdit, DocumentObjectDescriptor, DocumentViewId,
    EffectScriptReferenceDocument, FixtureDefinitionDocument, FixtureDocument, FixtureDocumentEdit,
    LayoutDocument, LayoutDocumentEdit, LayoutFixturePlacement, LayoutTargetDocument,
    ResolvedLayoutFixture, SequenceAudioDocument, SequenceCurveLibraryItemDocument,
    SequenceDocument, SequenceDocumentEdit, SequenceEffectDocument,
    SequenceEffectParamCurvePointEditValue, SequenceEffectParamCurveSourceDocument,
    SequenceEffectParamCurveValueEditValue, SequenceEffectParamDocument,
    SequenceEffectParamEditValue, SequenceEffectScriptDocument, SequenceEffectScriptParamDocument,
    SequenceLaneDocument, SequenceMarkCollectionDocument,
};
use dawn_language::effect_script::{EffectScriptKind, ScriptType};
use dawn_language::model::{
    Color, ColorModel, CurveValue, CurveValueType, Distance, DistanceSpan, EffectParam, FixtureId,
    Geometry, LayoutTargetKind, ObjectKind, Point3, Resolved, Rotation3, Scale3,
    SequenceEffectScope, Transform,
};
use dawn_language::path::{PathStringExt, Utf8PathBuf};
use dawn_language::render::{
    GeometryRenderBounds, GeometryRenderGuide, GeometryRenderPlan, GeometryRenderPoint,
};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum AppCommandDto {
    OpenProjectDialog,
    OpenProject {
        path: String,
    },
    ChooseNewProjectParentDirectory,
    CreateNewProject {
        parent_path: String,
        directory_name: String,
    },
    OpenFile {
        path: String,
    },
    CloseFile {
        path: String,
    },
    SetActiveFile {
        path: String,
    },
    UpdateActiveText {
        text: String,
    },
    SetActiveViewMode {
        mode: EditorViewModeDto,
    },
    ApplyActiveDocumentEdit {
        edit: DocumentEditDto,
    },
    ChooseSequenceAudio,
    ClearSequenceAudio,
    ExportActiveSequenceFseq {
        step_ms: u8,
    },
    FlushAutosave,
    ReloadActiveBufferFromDisk,
    KeepActiveBuffer,
    CreateFile {
        parent: String,
        name: String,
    },
    CreateDirectory {
        parent: String,
        name: String,
    },
    RenamePath {
        path: String,
        new_name: String,
    },
    DeletePath {
        path: String,
    },
    ReloadProject,
    ToggleProjectTree,
    SetEffectPreviewEnabled {
        enabled: bool,
    },
    SetEffectPreviewEffects {
        ids: Vec<u32>,
    },
    OpenPreviewWindow,
    PreviewPlay,
    PreviewPause,
    PreviewStop,
    PreviewRewindToZero,
    PreviewSeek {
        position_seconds: f64,
    },
    SetLiveOutputEnabled {
        enabled: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum AppCommandResponseDto {
    None,
    OptionalString { value: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppBackendChangedDto {
    pub(crate) snapshot: AppSnapshotDto,
    pub(crate) changed_slices: Vec<BackendSliceDto>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum BackendSliceDto {
    Workspace,
    Editor,
    ActiveDocument,
    Diagnostics,
    Preview,
    LiveOutput,
    Status,
    Prefs,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppSnapshotDto {
    pub(crate) workspace: WorkspaceReadModelDto,
    pub(crate) editor: EditorReadModelDto,
    pub(crate) active_document: ActiveDocumentReadModelDto,
    pub(crate) diagnostics: DiagnosticsReadModelDto,
    pub(crate) preview: PreviewReadModelDto,
    pub(crate) status: StatusReadModelDto,
    pub(crate) command_availability: Vec<CommandAvailabilityDto>,
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
                project_entries: view
                    .project_entries
                    .into_iter()
                    .map(WorkspaceEntryDto::from)
                    .collect(),
            },
            editor: EditorReadModelDto::from(view.editor),
            active_document: ActiveDocumentReadModelDto::from(view.active_document),
            diagnostics: DiagnosticsReadModelDto { diagnostics },
            preview: PreviewReadModelDto {
                preview: view.preview.as_ref().map(PreviewSnapshotDto::from),
                effect_preview_enabled: view.effect_preview_enabled,
            },
            status: StatusReadModelDto { status },
            command_availability: command_availability(),
        }
    }
}

impl From<&PreviewSnapshot> for PreviewSnapshotDto {
    fn from(snapshot: &PreviewSnapshot) -> Self {
        Self {
            source_label: snapshot.source_label.clone(),
            is_playing: snapshot.is_playing,
            preview_updating: snapshot.preview_updating,
            effect_preview_active: snapshot.effect_preview_active,
            position_seconds: snapshot.position_seconds,
            home_seconds: snapshot.home_seconds,
            duration_seconds: snapshot.duration_seconds,
            audio: snapshot.audio.clone().map(SequenceAudioDto::from),
            clock_source: snapshot.clock_source.clone(),
            audio_playback_status: AudioPlaybackStatus::from(snapshot.audio_playback_status),
            status: snapshot.status.clone(),
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceReadModelDto {
    pub(crate) project_root: Option<String>,
    pub(crate) project_entries: Vec<WorkspaceEntryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceEntryDto {
    pub(crate) path: String,
    pub(crate) kind: WorkspaceEntryKindDto,
    pub(crate) name: String,
    pub(crate) parent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub(crate) enum WorkspaceEntryKindDto {
    Directory,
    File,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorReadModelDto {
    pub(crate) tabs: Vec<EditorBufferDto>,
    pub(crate) active_file: Option<String>,
    pub(crate) active_buffer: Option<EditorBufferDto>,
}

impl From<EditorView> for EditorReadModelDto {
    fn from(editor: EditorView) -> Self {
        Self {
            tabs: editor
                .tabs
                .into_iter()
                .filter_map(EditorBufferDto::from_tab)
                .collect(),
            active_file: editor.active_file.map(|path| path.to_slash_string()),
            active_buffer: editor.active_buffer.map(EditorBufferDto::from),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditorBufferDto {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) text: String,
    pub(crate) dirty: bool,
    pub(crate) external_state: BufferExternalStateDto,
    pub(crate) view_mode: EditorViewModeDto,
}

impl EditorBufferDto {
    fn from_tab(tab: EditorTabView) -> Option<Self> {
        match tab {
            EditorTabView::Loaded(tab) => Some(Self::from(tab)),
            EditorTabView::Unloaded(tab) => Some(Self {
                path: tab.path.to_slash_string(),
                name: file_name(&tab.path),
                text: String::new(),
                dirty: false,
                external_state: BufferExternalStateDto::Current,
                view_mode: EditorViewModeDto::from(tab.view_mode),
            }),
        }
    }
}

impl From<LoadedEditorTabView> for EditorBufferDto {
    fn from(tab: LoadedEditorTabView) -> Self {
        Self {
            path: tab.path.to_slash_string(),
            name: file_name(&tab.path),
            text: tab.buffer.text,
            dirty: tab.buffer.dirty,
            external_state: BufferExternalStateDto::Current,
            view_mode: EditorViewModeDto::from(tab.view_mode),
        }
    }
}

fn file_name(path: &Utf8PathBuf) -> String {
    path.file_name()
        .map(ToString::to_string)
        .unwrap_or_else(|| path.to_slash_string())
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub(crate) enum BufferExternalStateDto {
    Current,
    ChangedOnDisk,
    DeletedOnDisk,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub(crate) enum EditorViewModeDto {
    Text,
    Gui,
}

impl From<BackendEditorViewMode> for EditorViewModeDto {
    fn from(mode: BackendEditorViewMode) -> Self {
        match mode {
            BackendEditorViewMode::Text => Self::Text,
            BackendEditorViewMode::Gui => Self::Gui,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActiveDocumentReadModelDto {
    pub(crate) descriptor: Option<DocumentDescriptorDto>,
    pub(crate) gui_document: Option<ActiveGuiDocumentDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocumentDescriptorDto {
    pub(crate) path: String,
    pub(crate) objects: Vec<DocumentObjectDescriptorDto>,
    pub(crate) available_views: Vec<DocumentViewIdDto>,
    pub(crate) default_object_keys: Vec<DocumentDefaultObjectKeyDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocumentDefaultObjectKeyDto {
    pub(crate) view: DocumentViewIdDto,
    pub(crate) object_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DocumentObjectDescriptorDto {
    pub(crate) key: String,
    pub(crate) kind: ObjectKindDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DocumentViewIdDto {
    Text,
    Layout,
    Fixture,
    Sequence,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ObjectKindDto {
    Project,
    Display,
    Controller,
    Layout,
    Fixture,
    Patch,
    Sequence,
    Curve,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum ActiveGuiDocumentDto {
    Sequence {
        document: SequenceDocumentDto,
    },
    Layout {
        document: LayoutDocumentDto,
    },
    Fixture {
        document: FixtureDocumentDto,
    },
    Blocked {
        reason: String,
        diagnostics: Vec<ProjectDiagnosticDto>,
    },
}

impl From<ActiveDocumentView> for ActiveDocumentReadModelDto {
    fn from(document: ActiveDocumentView) -> Self {
        Self {
            descriptor: document.descriptor.map(DocumentDescriptorDto::from),
            gui_document: document.gui_document.map(ActiveGuiDocumentDto::from),
        }
    }
}

impl From<DocumentDescriptor> for DocumentDescriptorDto {
    fn from(descriptor: DocumentDescriptor) -> Self {
        Self {
            path: descriptor.path,
            objects: descriptor
                .objects
                .into_iter()
                .map(DocumentObjectDescriptorDto::from)
                .collect(),
            available_views: descriptor
                .available_views
                .into_iter()
                .map(DocumentViewIdDto::from)
                .collect(),
            default_object_keys: descriptor
                .default_object_keys
                .into_iter()
                .map(|(view, object_key)| DocumentDefaultObjectKeyDto {
                    view: view.into(),
                    object_key,
                })
                .collect(),
        }
    }
}

impl From<DocumentObjectDescriptor> for DocumentObjectDescriptorDto {
    fn from(descriptor: DocumentObjectDescriptor) -> Self {
        Self {
            key: descriptor.key,
            kind: descriptor.kind.into(),
        }
    }
}

impl From<DocumentViewId> for DocumentViewIdDto {
    fn from(view: DocumentViewId) -> Self {
        match view {
            DocumentViewId::Text => Self::Text,
            DocumentViewId::Layout => Self::Layout,
            DocumentViewId::Fixture => Self::Fixture,
            DocumentViewId::Sequence => Self::Sequence,
        }
    }
}

impl From<ObjectKind> for ObjectKindDto {
    fn from(kind: ObjectKind) -> Self {
        match kind {
            ObjectKind::Project => Self::Project,
            ObjectKind::Display => Self::Display,
            ObjectKind::Controller => Self::Controller,
            ObjectKind::Layout => Self::Layout,
            ObjectKind::Fixture => Self::Fixture,
            ObjectKind::Patch => Self::Patch,
            ObjectKind::Sequence => Self::Sequence,
            ObjectKind::Curve => Self::Curve,
        }
    }
}

impl From<ActiveGuiDocument> for ActiveGuiDocumentDto {
    fn from(document: ActiveGuiDocument) -> Self {
        match document {
            ActiveGuiDocument::Sequence(document) => Self::Sequence {
                document: document.into(),
            },
            ActiveGuiDocument::Layout(document) => Self::Layout {
                document: document.into(),
            },
            ActiveGuiDocument::Fixture(document) => Self::Fixture {
                document: document.into(),
            },
            ActiveGuiDocument::Blocked(blocked) => Self::Blocked {
                reason: blocked.reason,
                diagnostics: blocked
                    .diagnostics
                    .iter()
                    .map(ProjectDiagnosticDto::from)
                    .collect(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiagnosticsReadModelDto {
    pub(crate) diagnostics: Vec<ProjectDiagnosticDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TextPositionDto {
    pub(crate) line: u32,
    pub(crate) character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewReadModelDto {
    pub(crate) preview: Option<PreviewSnapshotDto>,
    pub(crate) effect_preview_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandAvailabilityDto {
    pub(crate) command: AppCommandKindDto,
    pub(crate) available: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AppCommandKindDto {
    OpenProjectDialog,
    OpenProject,
    ChooseNewProjectParentDirectory,
    CreateNewProject,
    OpenFile,
    CloseFile,
    SetActiveFile,
    UpdateActiveText,
    SetActiveViewMode,
    ApplyActiveDocumentEdit,
    ChooseSequenceAudio,
    ClearSequenceAudio,
    ExportActiveSequenceFseq,
    FlushAutosave,
    ReloadActiveBufferFromDisk,
    KeepActiveBuffer,
    CreateFile,
    CreateDirectory,
    RenamePath,
    DeletePath,
    ReloadProject,
    ToggleProjectTree,
    SetEffectPreviewEnabled,
    SetEffectPreviewEffects,
    OpenPreviewWindow,
    PreviewPlay,
    PreviewPause,
    PreviewStop,
    PreviewRewindToZero,
    PreviewSeek,
    SetLiveOutputEnabled,
}

fn command_availability() -> Vec<CommandAvailabilityDto> {
    use AppCommandKindDto::*;
    [
        (OpenProjectDialog, true),
        (OpenProject, true),
        (ChooseNewProjectParentDirectory, false),
        (CreateNewProject, false),
        (OpenFile, true),
        (CloseFile, true),
        (SetActiveFile, true),
        (UpdateActiveText, true),
        (SetActiveViewMode, true),
        (ApplyActiveDocumentEdit, true),
        (ChooseSequenceAudio, true),
        (ClearSequenceAudio, true),
        (ExportActiveSequenceFseq, false),
        (FlushAutosave, true),
        (ReloadActiveBufferFromDisk, true),
        (KeepActiveBuffer, true),
        (CreateFile, true),
        (CreateDirectory, true),
        (RenamePath, true),
        (DeletePath, true),
        (ReloadProject, false),
        (ToggleProjectTree, false),
        (SetEffectPreviewEnabled, true),
        (SetEffectPreviewEffects, true),
        (OpenPreviewWindow, true),
        (PreviewPlay, true),
        (PreviewPause, true),
        (PreviewStop, true),
        (PreviewRewindToZero, true),
        (PreviewSeek, true),
        (SetLiveOutputEnabled, false),
    ]
    .into_iter()
    .map(|(command, available)| CommandAvailabilityDto { command, available })
    .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewSnapshotDto {
    pub(crate) source_label: String,
    pub(crate) is_playing: bool,
    pub(crate) preview_updating: bool,
    pub(crate) effect_preview_active: bool,
    pub(crate) position_seconds: f64,
    pub(crate) home_seconds: f64,
    pub(crate) duration_seconds: f64,
    pub(crate) audio: Option<SequenceAudioDto>,
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

impl From<&RenderedFrame> for PreviewSnapshotDto {
    fn from(frame: &RenderedFrame) -> Self {
        Self {
            source_label: frame.source.label.clone(),
            is_playing: false,
            preview_updating: matches!(frame.status, dawn_backend::RenderedFrameStatus::Live),
            effect_preview_active: false,
            position_seconds: frame.time_seconds,
            home_seconds: 0.0,
            duration_seconds: frame.source.duration_seconds,
            audio: None,
            clock_source: "backend".to_string(),
            audio_playback_status: AudioPlaybackStatus::None,
            status: match &frame.status {
                dawn_backend::RenderedFrameStatus::Live => "Live".to_string(),
                dawn_backend::RenderedFrameStatus::Idle(message)
                | dawn_backend::RenderedFrameStatus::Error(message) => message.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AudioPlaybackStatus {
    None,
    Missing,
    Loading,
    LoadingToPlay,
    Ready,
    Playing,
    Ended,
    Error,
}

impl From<dawn_backend::AudioPlaybackStatus> for AudioPlaybackStatus {
    fn from(status: dawn_backend::AudioPlaybackStatus) -> Self {
        match status {
            dawn_backend::AudioPlaybackStatus::None => Self::None,
            dawn_backend::AudioPlaybackStatus::Missing => Self::Missing,
            dawn_backend::AudioPlaybackStatus::Loading => Self::Loading,
            dawn_backend::AudioPlaybackStatus::LoadingToPlay => Self::LoadingToPlay,
            dawn_backend::AudioPlaybackStatus::Ready => Self::Ready,
            dawn_backend::AudioPlaybackStatus::Playing => Self::Playing,
            dawn_backend::AudioPlaybackStatus::Ended => Self::Ended,
            dawn_backend::AudioPlaybackStatus::Error => Self::Error,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewStateEventDto {
    #[serde(flatten)]
    pub(crate) preview: PreviewSnapshotDto,
    pub(crate) timing: PreviewTimingDto,
}

#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewTimingDto {
    pub(crate) backend_seconds: f64,
    pub(crate) target_fps: u32,
    pub(crate) active_fps: u32,
    pub(crate) target_frame_ms: f64,
    pub(crate) sleep_planned_ms: f64,
    pub(crate) loop_interval_ms: f64,
    pub(crate) audio_position_seconds: Option<f64>,
    pub(crate) snapshot_position_seconds: f64,
    pub(crate) frame_position_seconds: f64,
    pub(crate) snapshot_minus_audio_ms: Option<f64>,
    pub(crate) frame_minus_audio_ms: Option<f64>,
    pub(crate) loop_elapsed_ms: f64,
    pub(crate) preview_transport_lock_ms: f64,
    pub(crate) live_output_lock_ms: f64,
    pub(crate) model_lock_wait_ms: f64,
    pub(crate) preview_snapshot_ms: f64,
    pub(crate) analysis_clone_ms: f64,
    pub(crate) audio_poll_ms: f64,
    pub(crate) audio_apply_ms: f64,
    pub(crate) model_update_ms: f64,
    pub(crate) render_ms: f64,
    pub(crate) renderer_build_ms: f64,
    pub(crate) frame_evaluate_ms: f64,
    pub(crate) frame_fixture_clone_ms: f64,
    pub(crate) frame_effect_loop_ms: f64,
    pub(crate) frame_output_ms: f64,
    pub(crate) publish_ms: f64,
    pub(crate) event_emit_ms: f64,
    pub(crate) live_output_ms: f64,
    pub(crate) event_interval_ms: f64,
    pub(crate) rendered_active_effects: u32,
    pub(crate) rendered_sampled_pixels: u32,
    pub(crate) has_sink: bool,
    pub(crate) published_frame: bool,
    pub(crate) rendered_frame: bool,
}

impl PreviewTimingDto {
    pub(crate) fn from_render(
        timing: PreviewRenderTiming,
        backend_seconds: f64,
        target_fps: u32,
        loop_elapsed_ms: f64,
        has_sink: bool,
        published_frame: bool,
    ) -> Self {
        let target_frame_ms = 1000.0 / f64::from(target_fps.max(1));
        Self {
            backend_seconds,
            target_fps,
            active_fps: target_fps,
            target_frame_ms,
            loop_elapsed_ms,
            render_ms: timing.total_ms,
            renderer_build_ms: timing.renderer_build_ms,
            frame_evaluate_ms: timing.frame_evaluate_ms,
            frame_fixture_clone_ms: timing.fixture_clone_ms,
            frame_effect_loop_ms: timing.effect_loop_ms,
            frame_output_ms: timing.output_frame_ms,
            rendered_active_effects: timing.active_effects,
            rendered_sampled_pixels: timing.sampled_pixels,
            has_sink,
            published_frame,
            rendered_frame: true,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatusReadModelDto {
    pub(crate) status: RuntimeStatusDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", tag = "type")]
pub(crate) enum RuntimeStatusDto {
    NoProjectOpen,
    Saved,
    Message { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceEffectPreviewResultsDto {
    pub(crate) results: Vec<SequenceEffectPreviewResultDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceEffectPreviewRequestEffectDto {
    pub(crate) effect_id: u32,
    pub(crate) signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum SequenceEffectPreviewResultDto {
    Ready(SequenceEffectPreviewReadyResultDto),
    Unavailable(SequenceEffectPreviewUnavailableResultDto),
    Error(SequenceEffectPreviewErrorResultDto),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceEffectPreviewReadyResultDto {
    pub(crate) request_id: u32,
    pub(crate) signature: String,
    pub(crate) preview: SequenceEffectPreviewDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceEffectPreviewUnavailableResultDto {
    pub(crate) request_id: u32,
    pub(crate) effect_id: u32,
    pub(crate) signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceEffectPreviewErrorResultDto {
    pub(crate) request_id: u32,
    pub(crate) effect_id: u32,
    pub(crate) signature: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceEffectPreviewDto {
    pub(crate) effect_id: u32,
    pub(crate) duration_seconds: f64,
    pub(crate) source_pixel_count: u32,
    pub(crate) sampled_pixel_indices: Vec<u32>,
    pub(crate) columns: u32,
    pub(crate) rows: u32,
    pub(crate) colors: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewSceneDto {
    pub(crate) generation: u32,
    pub(crate) source_label: String,
    pub(crate) bounds: GeometryRenderBoundsDto,
    pub(crate) pixel_count: u32,
    pub(crate) fixtures: Vec<PreviewSceneFixtureDto>,
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

impl From<&RenderedFrame> for PreviewSceneDto {
    fn from(frame: &RenderedFrame) -> Self {
        let mut first_pixel_index = 0usize;
        let fixtures = frame
            .fixtures
            .iter()
            .map(|fixture| PreviewSceneFixtureDto::from_fixture(fixture, &mut first_pixel_index))
            .collect::<Vec<_>>();
        Self {
            generation: frame.generation.min(u32::MAX as u64) as u32,
            source_label: frame.source.label.clone(),
            bounds: GeometryRenderBoundsDto::from(frame.bounds),
            pixel_count: first_pixel_index.min(u32::MAX as usize) as u32,
            fixtures,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewSceneFixtureDto {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) bulb_radius_meters: f64,
    pub(crate) first_pixel_index: u32,
    pub(crate) pixels: Vec<GeometryRenderPointDto>,
}

impl PreviewSceneFixtureDto {
    fn from_fixture(fixture: &RenderedFixtureFrame, first_pixel_index: &mut usize) -> Self {
        let pixels = fixture
            .pixels
            .iter()
            .map(|pixel| GeometryRenderPointDto::from(pixel.position))
            .collect::<Vec<_>>();
        let dto = Self {
            id: fixture.id.0,
            name: fixture.name.clone(),
            bulb_radius_meters: fixture.bulb_radius.as_meters_f64(),
            first_pixel_index: (*first_pixel_index).min(u32::MAX as usize) as u32,
            pixels,
        };
        *first_pixel_index = first_pixel_index.saturating_add(fixture.pixels.len());
        dto
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeometryRenderBoundsDto {
    pub(crate) min_x_meters: f64,
    pub(crate) min_y_meters: f64,
    pub(crate) max_x_meters: f64,
    pub(crate) max_y_meters: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PreviewTransportMode {
    Webview2Shared,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum DocumentEditDto {
    Sequence { edit: SequenceDocumentEditDto },
    Layout { edit: LayoutDocumentEditDto },
    Fixture { edit: FixtureDocumentEditDto },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum SequenceDocumentEditDto {
    SetAudio {
        import: Option<String>,
    },
    AddEffect {
        script: EffectScriptReferenceDto,
        target: LayoutTargetDto,
        scope: SequenceEffectScopeDto,
        start_seconds: f64,
        mark_collection_key: Option<String>,
    },
    MoveEffect {
        id: u32,
        start_seconds: f64,
        target: Option<LayoutTargetDto>,
    },
    ResizeEffect {
        id: u32,
        start_seconds: f64,
        duration_seconds: f64,
    },
    ChangeEffectScript {
        id: u32,
        script: EffectScriptReferenceDto,
    },
    DeleteEffect {
        id: u32,
    },
    RetargetEffect {
        id: u32,
        target: LayoutTargetDto,
    },
    SetEffectScope {
        id: u32,
        scope: SequenceEffectScopeDto,
    },
    UpdateEffectParam {
        id: u32,
        name: String,
        value: SequenceEffectParamValueDto,
    },
    LinkEffectCurveParam {
        id: u32,
        name: String,
        curve_path: String,
        object_key: String,
    },
    UnlinkEffectCurveParam {
        id: u32,
        name: String,
    },
    CreateMarkCollection {
        key: String,
        name: String,
        color: String,
    },
    RenameMarkCollection {
        key: String,
        name: String,
    },
    DeleteMarkCollection {
        key: String,
    },
    SetMarkCollectionColor {
        key: String,
        color: String,
    },
    AddMark {
        collection_key: String,
        time_seconds: f64,
    },
    MoveMark {
        collection_key: String,
        index: u32,
        time_seconds: f64,
    },
    DeleteMark {
        collection_key: String,
        index: u32,
    },
    DeleteEffects {
        ids: Vec<u32>,
    },
    MoveEffects {
        edits: Vec<SequenceEffectMoveDocumentEditDto>,
    },
    ResizeEffects {
        edits: Vec<SequenceEffectResizeDocumentEditDto>,
    },
    DeleteMarks {
        marks: Vec<SequenceMarkRefDto>,
    },
    MoveMarks {
        edits: Vec<SequenceMarkMoveDocumentEditDto>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceEffectMoveDocumentEditDto {
    pub(crate) id: u32,
    pub(crate) start_seconds: f64,
    pub(crate) target: Option<LayoutTargetDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceEffectResizeDocumentEditDto {
    pub(crate) id: u32,
    pub(crate) start_seconds: f64,
    pub(crate) duration_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceMarkMoveDocumentEditDto {
    pub(crate) collection_key: String,
    pub(crate) index: u32,
    pub(crate) time_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceMarkRefDto {
    pub(crate) collection_key: String,
    pub(crate) index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum LayoutDocumentEditDto {
    UpdatePlacementTransform { id: u32, transform: TransformDto },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum FixtureDocumentEditDto {
    UpdateBulbDiameter {
        object_key: String,
        bulb_diameter_meters: f64,
    },
    MovePoint {
        object_key: String,
        point_index: u32,
        point: Point3MetersDto,
    },
}

impl TryFrom<DocumentEditDto> for DocumentEdit {
    type Error = String;

    fn try_from(edit: DocumentEditDto) -> Result<Self, Self::Error> {
        Ok(match edit {
            DocumentEditDto::Sequence { edit } => Self::Sequence(edit.try_into()?),
            DocumentEditDto::Layout { edit } => Self::Layout(edit.try_into()?),
            DocumentEditDto::Fixture { edit } => Self::Fixture(edit.try_into()?),
        })
    }
}

impl TryFrom<SequenceDocumentEditDto> for SequenceDocumentEdit {
    type Error = String;

    fn try_from(edit: SequenceDocumentEditDto) -> Result<Self, Self::Error> {
        Ok(match edit {
            SequenceDocumentEditDto::SetAudio { import } => Self::SetAudio { import },
            SequenceDocumentEditDto::AddEffect {
                script,
                target,
                scope,
                start_seconds,
                mark_collection_key,
            } => Self::AddEffect {
                script: script.into(),
                target: target.into(),
                scope: scope.into(),
                start_seconds,
                mark_collection_key,
            },
            SequenceDocumentEditDto::MoveEffect {
                id,
                start_seconds,
                target,
            } => Self::MoveEffect {
                id,
                start_seconds,
                target: target.map(Into::into),
            },
            SequenceDocumentEditDto::ResizeEffect {
                id,
                start_seconds,
                duration_seconds,
            } => Self::ResizeEffect {
                id,
                start_seconds,
                duration_seconds,
            },
            SequenceDocumentEditDto::ChangeEffectScript { id, script } => {
                Self::ChangeEffectScript {
                    id,
                    script: script.into(),
                }
            }
            SequenceDocumentEditDto::DeleteEffect { id } => Self::DeleteEffect { id },
            SequenceDocumentEditDto::RetargetEffect { id, target } => Self::RetargetEffect {
                id,
                target: target.into(),
            },
            SequenceDocumentEditDto::SetEffectScope { id, scope } => Self::SetEffectScope {
                id,
                scope: scope.into(),
            },
            SequenceDocumentEditDto::UpdateEffectParam { id, name, value } => {
                Self::UpdateEffectParam {
                    id,
                    name,
                    value: value.into(),
                }
            }
            SequenceDocumentEditDto::LinkEffectCurveParam {
                id,
                name,
                curve_path,
                object_key,
            } => Self::LinkEffectCurveParam {
                id,
                name,
                curve_path,
                object_key,
            },
            SequenceDocumentEditDto::UnlinkEffectCurveParam { id, name } => {
                Self::UnlinkEffectCurveParam { id, name }
            }
            SequenceDocumentEditDto::CreateMarkCollection { key, name, color } => {
                Self::CreateMarkCollection { key, name, color }
            }
            SequenceDocumentEditDto::RenameMarkCollection { key, name } => {
                Self::RenameMarkCollection { key, name }
            }
            SequenceDocumentEditDto::DeleteMarkCollection { key } => {
                Self::DeleteMarkCollection { key }
            }
            SequenceDocumentEditDto::SetMarkCollectionColor { key, color } => {
                Self::SetMarkCollectionColor { key, color }
            }
            SequenceDocumentEditDto::AddMark {
                collection_key,
                time_seconds,
            } => Self::AddMark {
                collection_key,
                time_seconds,
            },
            SequenceDocumentEditDto::MoveMark {
                collection_key,
                index,
                time_seconds,
            } => Self::MoveMark {
                collection_key,
                index: index as usize,
                time_seconds,
            },
            SequenceDocumentEditDto::DeleteMark {
                collection_key,
                index,
            } => Self::DeleteMark {
                collection_key,
                index: index as usize,
            },
            SequenceDocumentEditDto::DeleteEffects { ids } => Self::DeleteEffects { ids },
            SequenceDocumentEditDto::MoveEffects { edits } => Self::MoveEffects {
                edits: edits.into_iter().map(Into::into).collect(),
            },
            SequenceDocumentEditDto::ResizeEffects { edits } => Self::ResizeEffects {
                edits: edits.into_iter().map(Into::into).collect(),
            },
            SequenceDocumentEditDto::DeleteMarks { marks } => Self::DeleteMarks {
                marks: marks.into_iter().map(Into::into).collect(),
            },
            SequenceDocumentEditDto::MoveMarks { edits } => Self::MoveMarks {
                edits: edits.into_iter().map(Into::into).collect(),
            },
        })
    }
}

impl From<SequenceEffectMoveDocumentEditDto>
    for dawn_language::document::SequenceEffectMoveDocumentEdit
{
    fn from(edit: SequenceEffectMoveDocumentEditDto) -> Self {
        Self {
            id: edit.id,
            start_seconds: edit.start_seconds,
            target: edit.target.map(Into::into),
        }
    }
}

impl From<SequenceEffectResizeDocumentEditDto>
    for dawn_language::document::SequenceEffectResizeDocumentEdit
{
    fn from(edit: SequenceEffectResizeDocumentEditDto) -> Self {
        Self {
            id: edit.id,
            start_seconds: edit.start_seconds,
            duration_seconds: edit.duration_seconds,
        }
    }
}

impl From<SequenceMarkRefDto> for dawn_language::document::SequenceMarkRefDocumentEdit {
    fn from(mark: SequenceMarkRefDto) -> Self {
        Self {
            collection_key: mark.collection_key,
            index: mark.index as usize,
        }
    }
}

impl From<SequenceMarkMoveDocumentEditDto>
    for dawn_language::document::SequenceMarkMoveDocumentEdit
{
    fn from(mark: SequenceMarkMoveDocumentEditDto) -> Self {
        Self {
            collection_key: mark.collection_key,
            index: mark.index as usize,
            time_seconds: mark.time_seconds,
        }
    }
}

impl From<SequenceEffectParamValueDto> for SequenceEffectParamEditValue {
    fn from(value: SequenceEffectParamValueDto) -> Self {
        match value {
            SequenceEffectParamValueDto::Int { value } => Self::Integer(value as u64),
            SequenceEffectParamValueDto::Float { value } => Self::Float(value),
            SequenceEffectParamValueDto::Bool { value } => Self::Boolean(value),
            SequenceEffectParamValueDto::Color { value } => Self::Color(value),
            SequenceEffectParamValueDto::Enum { value } => Self::Enum(value),
            SequenceEffectParamValueDto::Flags { value } => Self::Flags(value),
            SequenceEffectParamValueDto::FloatCurve { points } => Self::FloatCurve(
                points
                    .into_iter()
                    .map(|point| SequenceEffectParamCurvePointEditValue {
                        time: point.time,
                        value: SequenceEffectParamCurveValueEditValue::Float(point.value),
                    })
                    .collect(),
            ),
            SequenceEffectParamValueDto::ColorCurve { points } => Self::ColorCurve(
                points
                    .into_iter()
                    .map(|point| SequenceEffectParamCurvePointEditValue {
                        time: point.time,
                        value: SequenceEffectParamCurveValueEditValue::Color(point.value),
                    })
                    .collect(),
            ),
            SequenceEffectParamValueDto::Marks { key } => Self::Marks(key),
        }
    }
}

impl TryFrom<LayoutDocumentEditDto> for LayoutDocumentEdit {
    type Error = String;

    fn try_from(edit: LayoutDocumentEditDto) -> Result<Self, Self::Error> {
        match edit {
            LayoutDocumentEditDto::UpdatePlacementTransform { id, transform } => {
                Ok(Self::UpdatePlacementTransform {
                    id: FixtureId(id),
                    transform: transform.try_into()?,
                })
            }
        }
    }
}

impl TryFrom<FixtureDocumentEditDto> for FixtureDocumentEdit {
    type Error = String;

    fn try_from(edit: FixtureDocumentEditDto) -> Result<Self, Self::Error> {
        match edit {
            FixtureDocumentEditDto::UpdateBulbDiameter {
                object_key,
                bulb_diameter_meters,
            } => Ok(Self::UpdateBulbDiameter {
                object_key,
                bulb_diameter: DistanceSpan::try_from_meters_f64_truncated(bulb_diameter_meters)
                    .map_err(str::to_string)?,
            }),
            FixtureDocumentEditDto::MovePoint {
                object_key,
                point_index,
                point,
            } => Ok(Self::MovePoint {
                object_key,
                point_index: point_index as usize,
                point: point.try_into()?,
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LayoutTargetDto {
    pub(crate) kind: LayoutTargetKindDto,
    pub(crate) name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LayoutTargetKindDto {
    Group,
    Fixture,
}

impl From<LayoutTargetKind> for LayoutTargetKindDto {
    fn from(kind: LayoutTargetKind) -> Self {
        match kind {
            LayoutTargetKind::Group => Self::Group,
            LayoutTargetKind::Fixture => Self::Fixture,
        }
    }
}

impl From<LayoutTargetDocument> for LayoutTargetDto {
    fn from(target: LayoutTargetDocument) -> Self {
        Self {
            kind: target.kind.into(),
            name: target.name,
        }
    }
}

impl From<LayoutTargetDto> for LayoutTargetDocument {
    fn from(target: LayoutTargetDto) -> Self {
        Self {
            kind: match target.kind {
                LayoutTargetKindDto::Group => LayoutTargetKind::Group,
                LayoutTargetKindDto::Fixture => LayoutTargetKind::Fixture,
            },
            name: target.name,
        }
    }
}

impl From<SequenceEffectScopeDto> for SequenceEffectScope {
    fn from(scope: SequenceEffectScopeDto) -> Self {
        match scope {
            SequenceEffectScopeDto::PerFixture => Self::PerFixture,
            SequenceEffectScopeDto::WholeTarget => Self::WholeTarget,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TransformDto {
    pub(crate) position: Point3MetersDto,
    pub(crate) rotation: Rotation3DegreesDto,
    pub(crate) scale: Scale3Dto,
}

impl From<Transform> for TransformDto {
    fn from(transform: Transform) -> Self {
        Self {
            position: transform.position.into(),
            rotation: Rotation3DegreesDto {
                x_degrees: transform.rotation.x,
                y_degrees: transform.rotation.y,
                z_degrees: transform.rotation.z,
            },
            scale: Scale3Dto {
                x: transform.scale.x,
                y: transform.scale.y,
                z: transform.scale.z,
            },
        }
    }
}

impl TryFrom<TransformDto> for Transform {
    type Error = String;

    fn try_from(transform: TransformDto) -> Result<Self, Self::Error> {
        Ok(Self {
            position: transform.position.try_into()?,
            rotation: Rotation3 {
                x: transform.rotation.x_degrees,
                y: transform.rotation.y_degrees,
                z: transform.rotation.z_degrees,
            },
            scale: Scale3 {
                x: transform.scale.x,
                y: transform.scale.y,
                z: transform.scale.z,
            },
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Point3MetersDto {
    pub(crate) x_meters: f64,
    pub(crate) y_meters: f64,
    pub(crate) z_meters: f64,
}

impl From<Point3> for Point3MetersDto {
    fn from(point: Point3) -> Self {
        Self {
            x_meters: point.x.as_meters_f64(),
            y_meters: point.y.as_meters_f64(),
            z_meters: point.z.as_meters_f64(),
        }
    }
}

impl TryFrom<Point3MetersDto> for Point3 {
    type Error = String;

    fn try_from(point: Point3MetersDto) -> Result<Self, Self::Error> {
        Ok(Self {
            x: Distance::try_from_meters_f64_truncated(point.x_meters).map_err(str::to_string)?,
            y: Distance::try_from_meters_f64_truncated(point.y_meters).map_err(str::to_string)?,
            z: Distance::try_from_meters_f64_truncated(point.z_meters).map_err(str::to_string)?,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Rotation3DegreesDto {
    pub(crate) x_degrees: f64,
    pub(crate) y_degrees: f64,
    pub(crate) z_degrees: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Scale3Dto {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) z: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceDocumentDto {
    pub(crate) path: String,
    pub(crate) object_key: String,
    pub(crate) duration_seconds: f64,
    pub(crate) frame_rate: u32,
    pub(crate) audio: Option<SequenceAudioDto>,
    pub(crate) mark_collections: Vec<SequenceMarkCollectionDto>,
    pub(crate) lanes: Vec<SequenceLaneDto>,
    pub(crate) effect_scripts: Vec<SequenceEffectScriptDto>,
    pub(crate) curve_library: Vec<SequenceCurveLibraryItemDto>,
    pub(crate) effects: Vec<SequenceEffectDto>,
    pub(crate) degraded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceMarkCollectionDto {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) color: String,
    pub(crate) marks_seconds: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceAudioDto {
    pub(crate) import: String,
    pub(crate) resolved_path: String,
    pub(crate) file_name: String,
    pub(crate) exists: bool,
}

impl From<SequenceAudioDocument> for SequenceAudioDto {
    fn from(audio: SequenceAudioDocument) -> Self {
        Self {
            import: audio.import,
            resolved_path: audio.resolved_path,
            file_name: audio.file_name,
            exists: audio.exists,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceLaneDto {
    pub(crate) target: LayoutTargetDto,
    pub(crate) label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceEffectDto {
    pub(crate) index: u32,
    pub(crate) id: u32,
    pub(crate) start_seconds: f64,
    pub(crate) duration_seconds: f64,
    pub(crate) target: LayoutTargetDto,
    pub(crate) target_label: String,
    pub(crate) scope: SequenceEffectScopeDto,
    pub(crate) script: String,
    pub(crate) script_source: Option<EffectScriptReferenceDto>,
    pub(crate) params: Vec<SequenceEffectParamDto>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SequenceEffectScopeDto {
    PerFixture,
    WholeTarget,
}

impl From<SequenceEffectScope> for SequenceEffectScopeDto {
    fn from(scope: SequenceEffectScope) -> Self {
        match scope {
            SequenceEffectScope::PerFixture => Self::PerFixture,
            SequenceEffectScope::WholeTarget => Self::WholeTarget,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceEffectParamDto {
    pub(crate) name: String,
    pub(crate) kind: SequenceEffectParamKindDto,
    pub(crate) options: Vec<String>,
    pub(crate) editable: bool,
    pub(crate) value: SequenceEffectParamValueDto,
    pub(crate) curve_source: Option<SequenceEffectParamCurveSourceDto>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SequenceEffectParamKindDto {
    Int,
    Float,
    Bool,
    Color,
    Enum,
    Flags,
    FloatCurve,
    ColorCurve,
    Marks,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum SequenceEffectParamValueDto {
    Int { value: u32 },
    Float { value: f64 },
    Bool { value: bool },
    Color { value: String },
    Enum { value: String },
    Flags { value: Vec<String> },
    FloatCurve { points: Vec<FloatCurvePointDto> },
    ColorCurve { points: Vec<ColorCurvePointDto> },
    Marks { key: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FloatCurvePointDto {
    pub(crate) time: f64,
    pub(crate) value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ColorCurvePointDto {
    pub(crate) time: f64,
    pub(crate) value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceCurveLibraryItemDto {
    pub(crate) path: String,
    pub(crate) object_key: String,
    pub(crate) display_name: String,
    pub(crate) value_type: SequenceCurveValueTypeDto,
    pub(crate) points: SequenceCurveLibraryPointsDto,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SequenceCurveValueTypeDto {
    Float,
    Color,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum SequenceCurveLibraryPointsDto {
    Float { points: Vec<FloatCurvePointDto> },
    Color { points: Vec<ColorCurvePointDto> },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum SequenceEffectParamCurveSourceDto {
    Inline,
    Library {
        reference: String,
        path: Option<String>,
        object_key: Option<String>,
        display_name: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceEffectScriptDto {
    pub(crate) name: String,
    pub(crate) kind: SequenceEffectScriptKindDto,
    pub(crate) script: EffectScriptReferenceDto,
    pub(crate) import: String,
    pub(crate) params: Vec<SequenceEffectScriptParamDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EffectScriptReferenceDto {
    pub(crate) path: String,
    pub(crate) effect_name: String,
}

impl From<EffectScriptReferenceDocument> for EffectScriptReferenceDto {
    fn from(script: EffectScriptReferenceDocument) -> Self {
        Self {
            path: script.path,
            effect_name: script.effect_name,
        }
    }
}

impl From<EffectScriptReferenceDto> for EffectScriptReferenceDocument {
    fn from(script: EffectScriptReferenceDto) -> Self {
        Self {
            path: script.path,
            effect_name: script.effect_name,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SequenceEffectScriptKindDto {
    Sample,
    Generator,
}

impl From<SequenceDocument> for SequenceDocumentDto {
    fn from(document: SequenceDocument) -> Self {
        Self {
            path: document.path,
            object_key: document.object_key,
            duration_seconds: document.duration_seconds,
            frame_rate: document.frame_rate,
            audio: document.audio.map(SequenceAudioDto::from),
            mark_collections: document
                .mark_collections
                .into_iter()
                .map(SequenceMarkCollectionDto::from)
                .collect(),
            lanes: document
                .lanes
                .into_iter()
                .map(SequenceLaneDto::from)
                .collect(),
            effect_scripts: document
                .effect_scripts
                .into_iter()
                .map(SequenceEffectScriptDto::from)
                .collect(),
            curve_library: document
                .curve_library
                .into_iter()
                .map(SequenceCurveLibraryItemDto::from)
                .collect(),
            effects: document
                .effects
                .into_iter()
                .map(SequenceEffectDto::from)
                .collect(),
            degraded: document.degraded,
        }
    }
}

impl From<SequenceMarkCollectionDocument> for SequenceMarkCollectionDto {
    fn from(collection: SequenceMarkCollectionDocument) -> Self {
        Self {
            key: collection.key,
            name: collection.name,
            color: collection.color,
            marks_seconds: collection.marks_seconds,
        }
    }
}

impl From<SequenceLaneDocument> for SequenceLaneDto {
    fn from(lane: SequenceLaneDocument) -> Self {
        Self {
            target: lane.target.into(),
            label: lane.label,
        }
    }
}

impl From<SequenceEffectDocument> for SequenceEffectDto {
    fn from(effect: SequenceEffectDocument) -> Self {
        Self {
            index: effect.index.min(u32::MAX as usize) as u32,
            id: effect.id,
            start_seconds: effect.start_seconds,
            duration_seconds: effect.duration_seconds,
            target: effect.target.into(),
            target_label: effect.target_label,
            scope: effect.scope.into(),
            script: effect.script,
            script_source: effect.script_source.map(EffectScriptReferenceDto::from),
            params: effect
                .params
                .into_iter()
                .map(SequenceEffectParamDto::from)
                .collect(),
        }
    }
}

impl From<SequenceEffectParamDocument> for SequenceEffectParamDto {
    fn from(param: SequenceEffectParamDocument) -> Self {
        let kind = sequence_param_kind(&param.value);
        Self {
            name: param.name,
            kind,
            options: Vec::new(),
            editable: true,
            value: SequenceEffectParamValueDto::from(param.value),
            curve_source: param
                .curve_source
                .map(SequenceEffectParamCurveSourceDto::from),
        }
    }
}

impl From<EffectParam<Resolved>> for SequenceEffectParamValueDto {
    fn from(param: EffectParam<Resolved>) -> Self {
        match param {
            EffectParam::Integer { value } => Self::Int {
                value: value.min(u32::MAX as u64) as u32,
            },
            EffectParam::Float { value } => Self::Float { value },
            EffectParam::Boolean { value } => Self::Bool { value },
            EffectParam::Color { value } => Self::Color {
                value: value.to_hex(),
            },
            EffectParam::Enum { value } => Self::Enum { value },
            EffectParam::Flags { value } => Self::Flags {
                value: value.values,
            },
            EffectParam::Curve { curve } => match curve.value_type {
                CurveValueType::Float => Self::FloatCurve {
                    points: curve
                        .points
                        .into_iter()
                        .filter_map(|point| match point.value {
                            CurveValue::Float(value) => Some(FloatCurvePointDto {
                                time: point.time,
                                value,
                            }),
                            CurveValue::Color(_) => None,
                        })
                        .collect(),
                },
                CurveValueType::Color => Self::ColorCurve {
                    points: curve
                        .points
                        .into_iter()
                        .filter_map(|point| match point.value {
                            CurveValue::Color(value) => Some(ColorCurvePointDto {
                                time: point.time,
                                value: value.to_hex(),
                            }),
                            CurveValue::Float(_) => None,
                        })
                        .collect(),
                },
            },
            EffectParam::Marks { key } => Self::Marks { key },
        }
    }
}

impl From<SequenceEffectParamCurveSourceDocument> for SequenceEffectParamCurveSourceDto {
    fn from(source: SequenceEffectParamCurveSourceDocument) -> Self {
        match source {
            SequenceEffectParamCurveSourceDocument::Inline => Self::Inline,
            SequenceEffectParamCurveSourceDocument::Library {
                reference,
                path,
                object_key,
                display_name,
            } => Self::Library {
                reference,
                path,
                object_key,
                display_name,
            },
        }
    }
}

impl From<SequenceCurveLibraryItemDocument> for SequenceCurveLibraryItemDto {
    fn from(item: SequenceCurveLibraryItemDocument) -> Self {
        let points = match item.curve.value_type {
            CurveValueType::Float => SequenceCurveLibraryPointsDto::Float {
                points: item
                    .curve
                    .points
                    .into_iter()
                    .filter_map(|point| match point.value {
                        CurveValue::Float(value) => Some(FloatCurvePointDto {
                            time: point.time,
                            value,
                        }),
                        CurveValue::Color(_) => None,
                    })
                    .collect(),
            },
            CurveValueType::Color => SequenceCurveLibraryPointsDto::Color {
                points: item
                    .curve
                    .points
                    .into_iter()
                    .filter_map(|point| match point.value {
                        CurveValue::Color(value) => Some(ColorCurvePointDto {
                            time: point.time,
                            value: value.to_hex(),
                        }),
                        CurveValue::Float(_) => None,
                    })
                    .collect(),
            },
        };
        Self {
            path: item.path,
            object_key: item.object_key,
            display_name: item.display_name,
            value_type: item.value_type.into(),
            points,
        }
    }
}

impl From<CurveValueType> for SequenceCurveValueTypeDto {
    fn from(value_type: CurveValueType) -> Self {
        match value_type {
            CurveValueType::Float => Self::Float,
            CurveValueType::Color => Self::Color,
        }
    }
}

impl From<SequenceEffectScriptDocument> for SequenceEffectScriptDto {
    fn from(script: SequenceEffectScriptDocument) -> Self {
        Self {
            name: script.name,
            kind: script.kind.into(),
            script: script.script.into(),
            import: script.import,
            params: script
                .params
                .into_iter()
                .filter_map(SequenceEffectScriptParamDto::from_document)
                .collect(),
        }
    }
}

impl SequenceEffectScriptParamDto {
    fn from_document(param: SequenceEffectScriptParamDocument) -> Option<Self> {
        Some(Self {
            name: param.name,
            kind: script_type_param_kind(param.value_type)?,
        })
    }
}

impl From<EffectScriptKind> for SequenceEffectScriptKindDto {
    fn from(kind: EffectScriptKind) -> Self {
        match kind {
            EffectScriptKind::Sample => Self::Sample,
            EffectScriptKind::Generator => Self::Generator,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceEffectScriptParamDto {
    pub(crate) name: String,
    pub(crate) kind: SequenceEffectParamKindDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LayoutDocumentDto {
    pub(crate) path: String,
    pub(crate) object_key: String,
    pub(crate) name: String,
    pub(crate) render_bounds: GeometryRenderBoundsDto,
    pub(crate) fixtures: Vec<LayoutFixturePlacementDto>,
}

impl From<LayoutDocument> for LayoutDocumentDto {
    fn from(document: LayoutDocument) -> Self {
        Self {
            path: document.path,
            object_key: document.object_key,
            name: document.name,
            render_bounds: document.render_bounds.into(),
            fixtures: document.fixtures.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LayoutFixturePlacementDto {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) transform: TransformDto,
    pub(crate) resolved_fixture: ResolvedLayoutFixtureDto,
}

impl From<LayoutFixturePlacement> for LayoutFixturePlacementDto {
    fn from(placement: LayoutFixturePlacement) -> Self {
        Self {
            id: placement.id.0,
            name: placement.name,
            transform: placement.transform.into(),
            resolved_fixture: placement.resolved_fixture.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedLayoutFixtureDto {
    pub(crate) name: String,
    pub(crate) color_model: String,
    pub(crate) bulb_diameter_meters: f64,
    pub(crate) geometry_summary: String,
    pub(crate) render_plan: GeometryRenderPlanDto,
    pub(crate) source_path: String,
    pub(crate) object_key: Option<String>,
}

impl From<ResolvedLayoutFixture> for ResolvedLayoutFixtureDto {
    fn from(fixture: ResolvedLayoutFixture) -> Self {
        Self {
            name: fixture.name,
            color_model: color_model_name(fixture.color_model),
            bulb_diameter_meters: fixture.bulb_diameter.as_meters_f64(),
            geometry_summary: fixture.geometry_summary,
            render_plan: fixture.render_plan.into(),
            source_path: fixture.source_path,
            object_key: fixture.object_key,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FixtureDocumentDto {
    pub(crate) path: String,
    pub(crate) selected_object_key: Option<String>,
    pub(crate) fixtures: Vec<FixtureDefinitionDto>,
}

impl From<FixtureDocument> for FixtureDocumentDto {
    fn from(document: FixtureDocument) -> Self {
        Self {
            path: document.path,
            selected_object_key: document.selected_object_key,
            fixtures: document.fixtures.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FixtureDefinitionDto {
    pub(crate) object_key: String,
    pub(crate) name: String,
    pub(crate) color_model: String,
    pub(crate) bulb_diameter_meters: f64,
    pub(crate) geometry: GeometryDto,
    pub(crate) geometry_summary: String,
    pub(crate) render_plan: GeometryRenderPlanDto,
}

impl From<FixtureDefinitionDocument> for FixtureDefinitionDto {
    fn from(fixture: FixtureDefinitionDocument) -> Self {
        Self {
            object_key: fixture.object_key,
            name: fixture.name,
            color_model: color_model_name(fixture.color_model),
            bulb_diameter_meters: fixture.bulb_diameter.as_meters_f64(),
            geometry: fixture.geometry.into(),
            geometry_summary: fixture.geometry_summary,
            render_plan: fixture.render_plan.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum GeometryDto {
    Points {
        points: Vec<Point3MetersDto>,
    },
    Lines {
        points: Vec<Point3MetersDto>,
        pixels: u32,
    },
    Arc {
        center: Point3MetersDto,
        radius_meters: f64,
        start_degrees: f64,
        end_degrees: f64,
        pixels: u32,
    },
}

impl From<Geometry> for GeometryDto {
    fn from(geometry: Geometry) -> Self {
        match geometry {
            Geometry::Points { points } => Self::Points {
                points: points.into_iter().map(Into::into).collect(),
            },
            Geometry::Lines { points, pixels } => Self::Lines {
                points: points.into_iter().map(Into::into).collect(),
                pixels,
            },
            Geometry::Arc {
                center,
                radius,
                start_degrees,
                end_degrees,
                pixels,
            } => Self::Arc {
                center: center.into(),
                radius_meters: radius.as_meters_f64(),
                start_degrees,
                end_degrees,
                pixels,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeometryRenderPlanDto {
    pub(crate) emitters: Vec<GeometryRenderPointDto>,
    pub(crate) guides: Vec<GeometryRenderGuideDto>,
    pub(crate) bounds: GeometryRenderBoundsDto,
    pub(crate) bulb_radius_meters: f64,
}

impl From<GeometryRenderPlan> for GeometryRenderPlanDto {
    fn from(plan: GeometryRenderPlan) -> Self {
        Self {
            emitters: plan.emitters.into_iter().map(Into::into).collect(),
            guides: plan.guides.into_iter().map(Into::into).collect(),
            bounds: plan.bounds.into(),
            bulb_radius_meters: plan.bulb_radius.as_meters_f64(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeometryRenderPointDto {
    pub(crate) x_meters: f64,
    pub(crate) y_meters: f64,
    pub(crate) z_meters: f64,
}

impl From<GeometryRenderPoint> for GeometryRenderPointDto {
    fn from(point: GeometryRenderPoint) -> Self {
        Self {
            x_meters: point.x.as_meters_f64(),
            y_meters: point.y.as_meters_f64(),
            z_meters: point.z.as_meters_f64(),
        }
    }
}

impl From<GeometryRenderBounds> for GeometryRenderBoundsDto {
    fn from(bounds: GeometryRenderBounds) -> Self {
        Self {
            min_x_meters: bounds.min_x.as_meters_f64(),
            min_y_meters: bounds.min_y.as_meters_f64(),
            max_x_meters: bounds.max_x.as_meters_f64(),
            max_y_meters: bounds.max_y.as_meters_f64(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum GeometryRenderGuideDto {
    Line {
        from: GeometryRenderPointDto,
        to: GeometryRenderPointDto,
    },
    Arc {
        start: GeometryRenderPointDto,
        end: GeometryRenderPointDto,
        radius_x_meters: f64,
        radius_y_meters: f64,
        rotation: f64,
        large_arc: bool,
        sweep_positive: bool,
    },
}

impl From<GeometryRenderGuide> for GeometryRenderGuideDto {
    fn from(guide: GeometryRenderGuide) -> Self {
        match guide {
            GeometryRenderGuide::Line { from, to } => Self::Line {
                from: from.into(),
                to: to.into(),
            },
            GeometryRenderGuide::Arc {
                start,
                end,
                radius_x,
                radius_y,
                rotation,
                large_arc,
                sweep_positive,
            } => Self::Arc {
                start: start.into(),
                end: end.into(),
                radius_x_meters: radius_x.as_meters_f64(),
                radius_y_meters: radius_y.as_meters_f64(),
                rotation,
                large_arc,
                sweep_positive,
            },
        }
    }
}

fn color_model_name(color_model: ColorModel) -> String {
    format!("{color_model:?}").to_ascii_lowercase()
}

impl From<SequenceEffectPreviewResult> for SequenceEffectPreviewResultDto {
    fn from(result: SequenceEffectPreviewResult) -> Self {
        match result {
            SequenceEffectPreviewResult::Ready(result) => {
                Self::Ready(SequenceEffectPreviewReadyResultDto {
                    request_id: 0,
                    signature: result.signature,
                    preview: SequenceEffectPreviewDto {
                        effect_id: result.preview.effect_id,
                        duration_seconds: result.preview.duration_seconds,
                        source_pixel_count: result.preview.source_pixel_count,
                        sampled_pixel_indices: result.preview.sampled_pixel_indices,
                        columns: result.preview.columns,
                        rows: result.preview.rows,
                        colors: result.preview.colors.into_iter().map(pack_rgb).collect(),
                    },
                })
            }
            SequenceEffectPreviewResult::Unavailable(result) => {
                Self::Unavailable(SequenceEffectPreviewUnavailableResultDto {
                    request_id: 0,
                    effect_id: result.effect_id,
                    signature: result.signature,
                })
            }
            SequenceEffectPreviewResult::Error(result) => {
                Self::Error(SequenceEffectPreviewErrorResultDto {
                    request_id: 0,
                    effect_id: result.effect_id,
                    signature: result.signature,
                    message: result.message,
                })
            }
        }
    }
}

fn sequence_param_kind(param: &EffectParam<Resolved>) -> SequenceEffectParamKindDto {
    match param {
        EffectParam::Integer { .. } => SequenceEffectParamKindDto::Int,
        EffectParam::Float { .. } => SequenceEffectParamKindDto::Float,
        EffectParam::Boolean { .. } => SequenceEffectParamKindDto::Bool,
        EffectParam::Color { .. } => SequenceEffectParamKindDto::Color,
        EffectParam::Enum { .. } => SequenceEffectParamKindDto::Enum,
        EffectParam::Flags { .. } => SequenceEffectParamKindDto::Flags,
        EffectParam::Curve { curve } => match curve.value_type {
            CurveValueType::Float => SequenceEffectParamKindDto::FloatCurve,
            CurveValueType::Color => SequenceEffectParamKindDto::ColorCurve,
        },
        EffectParam::Marks { .. } => SequenceEffectParamKindDto::Marks,
    }
}

fn script_type_param_kind(value_type: ScriptType) -> Option<SequenceEffectParamKindDto> {
    match value_type {
        ScriptType::Float => Some(SequenceEffectParamKindDto::Float),
        ScriptType::Int => Some(SequenceEffectParamKindDto::Int),
        ScriptType::Bool => Some(SequenceEffectParamKindDto::Bool),
        ScriptType::Color => Some(SequenceEffectParamKindDto::Color),
        ScriptType::Marks => Some(SequenceEffectParamKindDto::Marks),
        ScriptType::CurveFloat => Some(SequenceEffectParamKindDto::FloatCurve),
        ScriptType::CurveColor => Some(SequenceEffectParamKindDto::ColorCurve),
        ScriptType::Enum => Some(SequenceEffectParamKindDto::Enum),
        ScriptType::Flags => Some(SequenceEffectParamKindDto::Flags),
        ScriptType::Fixture
        | ScriptType::Pixel
        | ScriptType::Timeline
        | ScriptType::Target
        | ScriptType::TargetItems
        | ScriptType::TargetItem
        | ScriptType::Void => None,
    }
}

fn pack_rgb(color: Color) -> u32 {
    ((color.red as u32) << 16) | ((color.green as u32) << 8) | color.blue as u32
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
