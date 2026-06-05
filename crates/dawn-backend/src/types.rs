use camino::Utf8PathBuf;
use dawn_language::{
    analysis::{ProjectAnalysis, ProjectOverlay},
    document::{
        DocumentDescriptor, DocumentViewId, FixtureDocument, LayoutDocument, SequenceDocument,
        SequenceDocumentEdit,
    },
    model::{Point3, SequenceEffect, Transform},
    sequence_render::SequenceRenderCache,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnalysisTaskId(pub u64);

#[derive(Debug, Clone)]
pub struct AnalysisTask {
    pub id: AnalysisTaskId,
    pub project_root: Utf8PathBuf,
    pub project_file: Utf8PathBuf,
    pub overlays: Vec<ProjectOverlay>,
    pub active_gui_document: Option<ActiveGuiDocumentRequest>,
}

#[derive(Debug, Clone)]
pub struct AnalysisTaskOutput {
    pub id: AnalysisTaskId,
    pub analysis: ProjectAnalysis,
    pub active_gui_document: Option<ActiveGuiDocumentOutput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveGuiDocumentCacheKey {
    pub project_root: Utf8PathBuf,
    pub path: Utf8PathBuf,
    pub view_id: DocumentViewId,
    pub object_key: String,
}

#[derive(Debug, Clone)]
pub struct ActiveGuiDocumentRequest {
    pub cache_key: ActiveGuiDocumentCacheKey,
    pub descriptor: DocumentDescriptor,
}

#[derive(Debug, Clone)]
pub struct ActiveGuiDocumentOutput {
    pub cache_key: ActiveGuiDocumentCacheKey,
    pub document: Box<ActiveGuiDocument>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RenderTaskId(pub u64);

#[derive(Debug, Clone)]
pub struct RenderFrameTask {
    pub id: RenderTaskId,
    pub analysis: ProjectAnalysis,
    pub document: SequenceDocument,
    pub position_seconds: f64,
    pub generation: u64,
    pub cache: SequenceRenderCache,
}

#[derive(Debug, Clone)]
pub struct RenderFrameTaskOutput {
    pub id: RenderTaskId,
    pub frame: RenderedFrame,
    pub cache: SequenceRenderCache,
}

#[derive(Debug, Clone)]
pub struct RenderEffectPreviewTask {
    pub id: RenderTaskId,
    pub analysis: ProjectAnalysis,
    pub document: SequenceDocument,
    pub effects: Vec<RenderEffectPreviewRequestEffect>,
    pub cache: SequenceRenderCache,
}

#[derive(Debug, Clone)]
pub struct RenderEffectPreviewRequestEffect {
    pub effect_id: u32,
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct RenderEffectPreviewTaskOutput {
    pub id: RenderTaskId,
    pub results: Vec<SequenceEffectPreviewResult>,
    pub cache: SequenceRenderCache,
}

#[derive(Debug, Clone)]
pub enum SequenceEffectPreviewResult {
    Ready(SequenceEffectPreviewReadyResult),
    Unavailable(SequenceEffectPreviewUnavailableResult),
    Error(SequenceEffectPreviewErrorResult),
}

#[derive(Debug, Clone)]
pub struct SequenceEffectPreviewReadyResult {
    pub signature: String,
    pub preview: SequenceEffectPreview,
}

#[derive(Debug, Clone)]
pub struct SequenceEffectPreview {
    pub effect_id: u32,
    pub duration_seconds: f64,
    pub source_pixel_count: u32,
    pub sampled_pixel_indices: Vec<u32>,
    pub columns: u32,
    pub rows: u32,
    pub colors: Vec<dawn_language::model::Color>,
}

#[derive(Debug, Clone)]
pub struct SequenceEffectPreviewUnavailableResult {
    pub effect_id: u32,
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct SequenceEffectPreviewErrorResult {
    pub effect_id: u32,
    pub signature: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ExportFseqTask {
    pub id: RenderTaskId,
    pub analysis: ProjectAnalysis,
    pub document: SequenceDocument,
    pub output_path: Utf8PathBuf,
    pub options: FseqExportOptions,
    pub cache: SequenceRenderCache,
}

#[derive(Debug, Clone)]
pub struct ExportFseqTaskOutput {
    pub id: RenderTaskId,
    pub report: FseqExportReport,
    pub cache: SequenceRenderCache,
}

#[derive(Debug, Clone)]
pub struct FseqExportOptions {
    pub step_ms: u8,
    pub metadata: FseqExportMetadata,
}

impl Default for FseqExportOptions {
    fn default() -> Self {
        Self {
            step_ms: 50,
            metadata: FseqExportMetadata::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FseqExportMetadata {
    pub media_filename: Option<String>,
    pub producer: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FseqExportReport {
    pub sequence: String,
    pub step_ms: u8,
    pub frame_count: u32,
    pub channel_count: u32,
    pub bytes_written: u64,
}

#[derive(Debug, Clone, Default)]
pub struct RenderView {
    pub frame: Option<RenderedFrame>,
    pub effect_previews: Vec<SequenceEffectPreviewResult>,
    pub export_report: Option<FseqExportReport>,
}

#[derive(Debug, Clone)]
pub struct RenderedFrame {
    pub source: RenderedFrameSource,
    pub time_seconds: f64,
    pub generation: u64,
    pub status: RenderedFrameStatus,
    pub bounds: dawn_language::render::GeometryRenderBounds,
    pub fixtures: Vec<RenderedFixtureFrame>,
}

#[derive(Debug, Clone)]
pub struct RenderedFrameSource {
    pub label: String,
    pub kind: RenderedFrameSourceKind,
    pub duration_seconds: f64,
    pub fps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderedFrameSourceKind {
    Sequence,
    Empty,
}

#[derive(Debug, Clone)]
pub enum RenderedFrameStatus {
    Live,
    Idle(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct RenderedFixtureFrame {
    pub id: dawn_language::model::FixtureId,
    pub name: String,
    pub bulb_radius: dawn_language::model::DistanceSpan,
    pub pixels: Vec<RenderedPixelFrame>,
}

#[derive(Debug, Clone)]
pub struct RenderedPixelFrame {
    pub position: dawn_language::render::GeometryRenderPoint,
    pub color: dawn_language::model::Color,
}

impl From<dawn_language::sequence_render::OutputFrame> for RenderedFrame {
    fn from(value: dawn_language::sequence_render::OutputFrame) -> Self {
        Self {
            source: value.source.into(),
            time_seconds: value.time_seconds,
            generation: value.generation,
            status: value.status.into(),
            bounds: value.bounds,
            fixtures: value.fixtures.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<dawn_language::sequence_render::OutputSourceMetadata> for RenderedFrameSource {
    fn from(value: dawn_language::sequence_render::OutputSourceMetadata) -> Self {
        Self {
            label: value.label,
            kind: value.kind.into(),
            duration_seconds: value.duration_seconds,
            fps: value.fps,
        }
    }
}

impl From<dawn_language::sequence_render::OutputSourceKind> for RenderedFrameSourceKind {
    fn from(value: dawn_language::sequence_render::OutputSourceKind) -> Self {
        match value {
            dawn_language::sequence_render::OutputSourceKind::Sequence => Self::Sequence,
            dawn_language::sequence_render::OutputSourceKind::Empty => Self::Empty,
        }
    }
}

impl From<dawn_language::sequence_render::OutputFrameStatus> for RenderedFrameStatus {
    fn from(value: dawn_language::sequence_render::OutputFrameStatus) -> Self {
        match value {
            dawn_language::sequence_render::OutputFrameStatus::Live => Self::Live,
            dawn_language::sequence_render::OutputFrameStatus::Idle(message) => Self::Idle(message),
            dawn_language::sequence_render::OutputFrameStatus::Error(message) => {
                Self::Error(message)
            }
        }
    }
}

impl From<dawn_language::sequence_render::OutputFixtureFrame> for RenderedFixtureFrame {
    fn from(value: dawn_language::sequence_render::OutputFixtureFrame) -> Self {
        Self {
            id: value.id,
            name: value.name,
            bulb_radius: value.bulb_radius,
            pixels: value.pixels.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<dawn_language::sequence_render::OutputPixelFrame> for RenderedPixelFrame {
    fn from(value: dawn_language::sequence_render::OutputPixelFrame) -> Self {
        Self {
            position: value.position,
            color: value.color,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorViewMode {
    #[default]
    Text,
    Gui,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileVersion {
    pub len: u64,
    pub modified_millis: Option<u128>,
    pub content_hash: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEntry {
    pub path: Utf8PathBuf,
    pub kind: WorkspaceEntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceEntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectFileMetadata {
    pub(crate) len: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectFileSnapshot {
    pub(crate) text: String,
    pub(crate) version: FileVersion,
}

#[derive(Debug, Clone, Default)]
pub struct ActiveDocumentView {
    pub descriptor: Option<DocumentDescriptor>,
    pub gui_document: Option<ActiveGuiDocument>,
}

#[derive(Debug, Clone)]
pub enum ActiveGuiDocument {
    Sequence(SequenceDocument),
    Layout(LayoutDocument),
    Fixture(FixtureDocument),
    Blocked(ActiveGuiDocumentBlocked),
}

#[derive(Debug, Clone)]
pub struct ActiveGuiDocumentBlocked {
    pub reason: String,
    pub diagnostics: Vec<dawn_language::analysis::ProjectDiagnostic>,
}

#[derive(Debug, Clone)]
pub enum SequenceGuiEdit {
    Document(SequenceDocumentEdit),
}

#[derive(Debug, Clone)]
pub enum SequenceSelection {
    Effects { ids: Vec<u32> },
    Marks { marks: Vec<SequenceMarkRef> },
}

#[derive(Debug, Clone)]
pub struct SequenceMarkRef {
    pub collection_key: String,
    pub index: u32,
}

#[derive(Debug, Clone)]
pub struct SequencePasteAnchor {
    pub lane_index: Option<u32>,
    pub time_seconds: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct SequenceSelectionEditResult {
    pub selection: Option<SequenceSelection>,
    pub copied_count: u32,
    pub skipped_count: u32,
}

#[derive(Debug, Clone)]
pub enum SequenceSelectionEdit {
    Copy {
        selection: SequenceSelection,
    },
    Cut {
        selection: SequenceSelection,
    },
    Delete {
        selection: SequenceSelection,
    },
    Paste {
        anchor: SequencePasteAnchor,
    },
    MoveEffects {
        ids: Vec<u32>,
        time_delta_seconds: f64,
        lane_delta: i32,
    },
    ResizeEffects {
        ids: Vec<u32>,
        edge: SequenceResizeEdge,
        time_delta_seconds: f64,
    },
    MoveMarks {
        marks: Vec<SequenceMarkRef>,
        time_delta_seconds: f64,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum SequenceResizeEdge {
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub enum SequenceClipboard {
    Effects(Vec<SequenceEffect<dawn_language::model::Authored>>),
    Marks(Vec<dawn_language::document::SequenceMarkPasteDocumentEdit>),
}

#[derive(Debug, Clone)]
pub enum LayoutGuiEdit {
    UpdatePlacementTransform { id: u32, transform: Transform },
}

#[derive(Debug, Clone)]
pub enum FixtureGuiEdit {
    UpdateBulbDiameter {
        object_key: String,
        bulb_diameter_meters: f64,
    },
    MovePoint {
        object_key: String,
        point_index: u32,
        point: Point3,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectPathMove {
    pub(crate) old_path: Utf8PathBuf,
    pub(crate) new_path: Utf8PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectSessionPreferences {
    pub(crate) tabs: Vec<ProjectSessionTabPreference>,
    pub(crate) active_file: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectSessionTabPreference {
    pub(crate) path: Utf8PathBuf,
    pub(crate) view_mode: EditorViewMode,
}
