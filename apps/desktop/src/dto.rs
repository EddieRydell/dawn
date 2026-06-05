use dawn_backend::{
    AppView, EditorTabView, EditorView, EditorViewMode as BackendEditorViewMode,
    LoadedEditorTabView, RenderedFixtureFrame, RenderedFrame, SequenceEffectPreviewResult,
};
use dawn_language::analysis::{
    DiagnosticCode, DiagnosticSeverity, ProjectAnalysis, ProjectDiagnostic, TextRange,
};
use dawn_language::document::{
    EffectScriptReferenceDocument, FixtureDefinitionDocument, FixtureDocument, LayoutDocument,
    LayoutFixturePlacement, ResolvedLayoutFixture, SequenceAudioDocument,
};
use dawn_language::model::{
    Color, ColorModel, Geometry, LayoutTargetKind, Point3, SequenceEffectScope, Transform,
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
    UndoActiveEdit,
    RedoActiveEdit,
    ApplySequenceGuiEdit {
        edit: SequenceGuiEditDto,
    },
    ApplySequenceSelectionEdit {
        edit: SequenceSelectionEditDto,
    },
    ChooseSequenceAudio,
    ClearSequenceAudio,
    ExportActiveSequenceFseq {
        step_ms: u8,
    },
    ApplyLayoutGuiEdit {
        edit: LayoutGuiEditDto,
    },
    ApplyFixtureGuiEdit {
        edit: FixtureGuiEditDto,
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
    OptionalString {
        value: Option<String>,
    },
    SequenceSelectionEditResult {
        result: SequenceSelectionEditResultDto,
    },
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
            editor: EditorReadModelDto::from(view.editor),
            active_document: ActiveDocumentReadModelDto::default(),
            diagnostics: DiagnosticsReadModelDto { diagnostics },
            preview: PreviewReadModelDto::from_frame(view.render.frame.as_ref()),
            live_output: LiveOutputReadModelDto::default(),
            status: StatusReadModelDto { status },
            prefs: PrefsReadModelDto {
                project_tree_visible: true,
                effect_preview_enabled: false,
            },
        }
    }
}

impl PreviewReadModelDto {
    fn from_frame(frame: Option<&RenderedFrame>) -> Self {
        Self {
            preview: frame.map(PreviewSnapshotDto::from).unwrap_or_default(),
            effect_preview_enabled: false,
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
    pub(crate) project_tree_visible: bool,
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
    pub(crate) preview: PreviewSnapshotDto,
    pub(crate) effect_preview_enabled: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OutputReadoutDto {
    pub(crate) enabled: bool,
    pub(crate) status: String,
    pub(crate) active_universe_count: u32,
    pub(crate) last_error: Option<String>,
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
pub(crate) struct PrefsReadModelDto {
    pub(crate) project_tree_visible: bool,
    pub(crate) effect_preview_enabled: bool,
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
pub(crate) enum SequenceGuiEditDto {
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum SequenceSelectionDto {
    Effects { ids: Vec<u32> },
    Marks { marks: Vec<SequenceMarkRefDto> },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceMarkRefDto {
    pub(crate) collection_key: String,
    pub(crate) index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequencePasteAnchorDto {
    pub(crate) lane_index: u32,
    pub(crate) time_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SequenceSelectionEditResultDto {
    pub(crate) selection: Option<SequenceSelectionDto>,
    pub(crate) copied_count: u32,
    pub(crate) skipped_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum SequenceSelectionEditDto {
    Copy {
        selection: SequenceSelectionDto,
    },
    Cut {
        selection: SequenceSelectionDto,
    },
    Delete {
        selection: SequenceSelectionDto,
    },
    Paste {
        anchor: SequencePasteAnchorDto,
    },
    MoveEffects {
        ids: Vec<u32>,
        time_delta_seconds: f64,
        lane_delta: i32,
    },
    ResizeEffects {
        ids: Vec<u32>,
        edge: SequenceResizeEdgeDto,
        time_delta_seconds: f64,
    },
    MoveMarks {
        marks: Vec<SequenceMarkRefDto>,
        time_delta_seconds: f64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SequenceResizeEdgeDto {
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum LayoutGuiEditDto {
    UpdatePlacementTransform { id: u32, transform: TransformDto },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum FixtureGuiEditDto {
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SequenceEffectScriptKindDto {
    Sample,
    Generator,
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
