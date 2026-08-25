use dawn_project_io::SourceObjectKind;
use serde::{Deserialize, Serialize};
use specta::Type;

mod app;
mod operator_rewrite;
mod package;
mod setup;

pub use app::*;
pub use operator_rewrite::*;
pub use package::*;
pub use setup::*;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewSequenceRequest {
    pub file_path: String,
    pub object_key: String,
    pub duration_seconds: f64,
    pub frame_rate: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLayoutState {
    pub sidebar_width_px: f64,
    pub inspector_width_px: f64,
    pub sidebar_collapsed: bool,
    pub inspector_collapsed: bool,
    pub active_sidebar_view: SidebarView,
}

impl<'de> Deserialize<'de> for WorkspaceLayoutState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct StoredLayout {
            sidebar_width_px: Option<f64>,
            project_tree_width_px: Option<f64>,
            inspector_width_px: f64,
            sidebar_collapsed: Option<bool>,
            project_tree_collapsed: Option<bool>,
            inspector_collapsed: bool,
            #[serde(default)]
            active_sidebar_view: SidebarView,
        }
        let stored = StoredLayout::deserialize(deserializer)?;
        Ok(Self {
            sidebar_width_px: stored
                .sidebar_width_px
                .or(stored.project_tree_width_px)
                .unwrap_or(288.0),
            inspector_width_px: stored.inspector_width_px,
            sidebar_collapsed: stored
                .sidebar_collapsed
                .or(stored.project_tree_collapsed)
                .unwrap_or(false),
            inspector_collapsed: stored.inspector_collapsed,
            active_sidebar_view: stored.active_sidebar_view,
        })
    }
}

impl Default for WorkspaceLayoutState {
    fn default() -> Self {
        Self {
            sidebar_width_px: 288.0,
            inspector_width_px: 260.0,
            sidebar_collapsed: false,
            inspector_collapsed: false,
            active_sidebar_view: SidebarView::Explorer,
        }
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SidebarView {
    #[default]
    Explorer,
    Search,
    Packages,
    Problems,
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceExplorerState {
    pub expanded_paths: Vec<String>,
    pub recent_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub reopen_last_project: bool,
    #[serde(default = "default_editor_view_mode")]
    pub editor_view_mode: EditorViewMode,
    pub reopen_preview_window: bool,
    pub autosave_text_edits: bool,
    pub sequence_initial_zoom_mode: SequenceInitialZoomMode,
    pub sequence_initial_px_per_second: f64,
    pub sequence_initial_lane_height_px: f64,
    pub effect_raster: EffectRasterSettings,
}

fn default_editor_view_mode() -> EditorViewMode {
    EditorViewMode::Gui
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            reopen_last_project: true,
            editor_view_mode: EditorViewMode::Gui,
            reopen_preview_window: true,
            autosave_text_edits: true,
            sequence_initial_zoom_mode: SequenceInitialZoomMode::FitToWidth,
            sequence_initial_px_per_second: 80.0,
            sequence_initial_lane_height_px: 42.0,
            effect_raster: EffectRasterSettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceInitialZoomMode {
    FitToWidth,
    FixedPxPerSecond,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EffectRasterSettings {
    pub render_scale: f64,
    pub max_columns: u32,
    pub max_rows: u32,
    pub min_frame_stride: u32,
}

impl Default for EffectRasterSettings {
    fn default() -> Self {
        Self {
            render_scale: 1.0,
            max_columns: 256,
            max_rows: 50,
            min_frame_stride: 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AudioTransportState {
    Unloaded,
    Playing,
    Paused,
    Stopped,
    Ended,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AudioTransportSnapshot {
    pub state: AudioTransportState,
    pub source: Option<SequenceAudio>,
    pub generation: u32,
    pub position_seconds: f64,
    pub home_seconds: f64,
    pub duration_seconds: f64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum EditorViewMode {
    Text,
    Gui,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ElementTargetKind {
    Group,
    Element,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ObjectKind {
    Project,
    Setup,
    Controller,
    ElementTree,
    Preview,
    Prop,
    FixtureProfile,
    Patch,
    Sequence,
    Curve,
    Gradient,
    Effect,
    Operator,
}

impl From<&SourceObjectKind> for ObjectKind {
    fn from(kind: &SourceObjectKind) -> Self {
        match kind {
            SourceObjectKind::Project => Self::Project,
            SourceObjectKind::Setup => Self::Setup,
            SourceObjectKind::Controller => Self::Controller,
            SourceObjectKind::ElementTree => Self::ElementTree,
            SourceObjectKind::PreviewLayout => Self::Preview,
            SourceObjectKind::Patch => Self::Patch,
            SourceObjectKind::PropDefinition => Self::Prop,
            SourceObjectKind::FixtureProfile => Self::FixtureProfile,
            SourceObjectKind::Curve => Self::Curve,
            SourceObjectKind::Gradient => Self::Gradient,
            SourceObjectKind::Sequence => Self::Sequence,
            SourceObjectKind::EffectDefinition | SourceObjectKind::EffectInstance => Self::Effect,
            SourceObjectKind::OperatorDefinition => Self::Operator,
        }
    }
}

impl ObjectKind {
    pub(crate) fn document_view(&self) -> Option<DocumentViewId> {
        match self {
            Self::Setup => Some(DocumentViewId::Setup),
            Self::Preview => Some(DocumentViewId::Preview),
            Self::Prop => Some(DocumentViewId::Prop),
            Self::Sequence => Some(DocumentViewId::Sequence),
            Self::Project
            | Self::Controller
            | Self::ElementTree
            | Self::FixtureProfile
            | Self::Patch
            | Self::Curve
            | Self::Gradient
            | Self::Effect
            | Self::Operator => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceEffectParamKind {
    Int,
    Float,
    Bool,
    Color,
    Enum,
    Curve,
    Gradient,
    IntArray,
    FloatArray,
    BoolArray,
    ColorArray,
    CurveArray,
    GradientArray,
    Marks,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceEffectScope {
    PerFixture,
    WholeTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceEffectDefinitionKind {
    Sample,
    Generator,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceResizeEdge {
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceEntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceEntryRole {
    Directory,
    Project,
    Entrypoint,
    Setup,
    Layout,
    Fixture,
    Patch,
    Curve,
    Gradient,
    Effect,
    Operator,
    Sequence,
    Manifest,
    Lockfile,
    Asset,
    PathDependency,
    File,
}

pub(crate) fn workspace_role_for_source_object(kind: &SourceObjectKind) -> WorkspaceEntryRole {
    match kind {
        SourceObjectKind::Project => WorkspaceEntryRole::Project,
        SourceObjectKind::Setup => WorkspaceEntryRole::Setup,
        SourceObjectKind::PreviewLayout | SourceObjectKind::PropDefinition => {
            WorkspaceEntryRole::Layout
        }
        SourceObjectKind::FixtureProfile => WorkspaceEntryRole::Fixture,
        SourceObjectKind::Patch => WorkspaceEntryRole::Patch,
        SourceObjectKind::Curve => WorkspaceEntryRole::Curve,
        SourceObjectKind::Gradient => WorkspaceEntryRole::Gradient,
        SourceObjectKind::Sequence => WorkspaceEntryRole::Sequence,
        SourceObjectKind::EffectDefinition | SourceObjectKind::EffectInstance => {
            WorkspaceEntryRole::Effect
        }
        SourceObjectKind::OperatorDefinition => WorkspaceEntryRole::Operator,
        SourceObjectKind::Controller | SourceObjectKind::ElementTree => WorkspaceEntryRole::File,
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceEntryOwnership {
    Project,
    PathDependency,
    Registry,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceOperation {
    Open,
    Create,
    Rename,
    Delete,
    Move,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSearchRequest {
    pub request_id: u32,
    pub query: String,
    pub match_case: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSearchMatch {
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub preview: String,
    pub kind: ProjectSearchMatchKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ProjectSearchMatchKind {
    Filename,
    Content,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSearchResponse {
    pub request_id: u32,
    pub matches: Vec<ProjectSearchMatch>,
    pub skipped_binary: u32,
    pub skipped_oversized: u32,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePathChangeRequest {
    pub source: String,
    pub destination: String,
    pub project_revision: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum WorkspacePathOwnership {
    Project,
    PathDependency {
        module_id: String,
        module_root: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePathChangeImpact {
    pub documents: Vec<String>,
    pub imports: Vec<String>,
    pub manifests: Vec<String>,
    pub assets: Vec<String>,
    pub modules: Vec<String>,
    pub open_files: Vec<String>,
    pub recent_files: Vec<String>,
    pub persisted_state: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePathChangePlan {
    pub request: WorkspacePathChangeRequest,
    pub structural: bool,
    pub ownership: WorkspacePathOwnership,
    pub impact: WorkspacePathChangeImpact,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDefaultObjectKey {
    pub view: DocumentViewId,
    pub object_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDescriptor {
    pub path: String,
    pub objects: Vec<DocumentObjectDescriptor>,
    pub available_views: Vec<DocumentViewId>,
    pub default_object_keys: Vec<DocumentDefaultObjectKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentObjectDescriptor {
    pub key: String,
    pub kind: ObjectKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EditorBuffer {
    pub path: String,
    pub name: String,
    pub text: String,
    pub dirty: bool,
    pub external_state: BufferExternalState,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceEffectReference {
    Builtin {
        effect: SequenceBuiltinEffect,
    },
    Custom {
        module_id: String,
        path: String,
        effect_name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceBuiltinEffect {
    Pulse,
    Chase,
    Spin,
    MarkPulse,
    MarkChase,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PropDefinition {
    pub source_ref: GuiObjectRef,
    pub object_key: String,
    pub name: String,
    pub color_model: String,
    pub bulb_diameter_meters: f64,
    pub geometry: Geometry,
    pub geometry_summary: String,
    pub render_plan: GeometryRenderPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PropGuiDocument {
    pub path: String,
    pub source_ref: Option<GuiObjectRef>,
    pub selected_object_key: Option<String>,
    pub fixtures: Vec<PropDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PropGuiEdit {
    UpdateBulbDiameter {
        object_key: String,
        bulb_diameter_meters: f64,
    },
    MovePoint {
        object_key: String,
        point_index: u32,
        point: Point3Meters,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceCurvePoint {
    pub time: f64,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Geometry {
    Points {
        points: Vec<Point3Meters>,
    },
    Lines {
        points: Vec<Point3Meters>,
        pixels: u32,
    },
    Arc {
        center: Point3Meters,
        radius_meters: f64,
        start_degrees: f64,
        end_degrees: f64,
        pixels: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderBounds {
    pub min_x_meters: f64,
    pub min_y_meters: f64,
    pub max_x_meters: f64,
    pub max_y_meters: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GeometryRenderGuide {
    Line {
        from: GeometryRenderPoint,
        to: GeometryRenderPoint,
    },
    Arc {
        start: GeometryRenderPoint,
        end: GeometryRenderPoint,
        radius_x_meters: f64,
        radius_y_meters: f64,
        rotation: f64,
        large_arc: bool,
        sweep_positive: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderPlan {
    pub emitters: Vec<GeometryRenderPoint>,
    pub guides: Vec<GeometryRenderGuide>,
    pub bounds: GeometryRenderBounds,
    pub bulb_radius_meters: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderPoint {
    pub x_meters: f64,
    pub y_meters: f64,
    pub z_meters: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewGuiDocument {
    pub path: String,
    pub source_ref: GuiObjectRef,
    pub object_key: String,
    pub name: String,
    pub render_bounds: GeometryRenderBounds,
    pub fixtures: Vec<PreviewPropPlacement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewPropPlacement {
    pub source_ref: GuiObjectRef,
    pub id: u32,
    pub name: String,
    pub transform: Transform,
    pub resolved_fixture: ResolvedPreviewProp,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PreviewGuiEdit {
    UpdatePlacementTransform { id: u32, transform: Transform },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ElementTarget {
    pub kind: ElementTargetKind,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LiveOutputSnapshot {
    pub state: LiveOutputState,
    pub generation: u32,
    pub active_controller_count: u32,
    pub active_universe_count: u32,
    pub controllers: Vec<LiveOutputControllerSnapshot>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LiveOutputState {
    Disabled,
    Preparing,
    Holding,
    Streaming,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LiveOutputControllerSnapshot {
    pub id: String,
    pub state: LiveOutputControllerState,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum LiveOutputControllerState {
    Opening,
    Active,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Point3Meters {
    pub x_meters: f64,
    pub y_meters: f64,
    pub z_meters: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDiagnostic {
    pub path: String,
    pub range: Option<TextRange>,
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
    pub related: Vec<RelatedDiagnosticLocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RelatedDiagnosticLocation {
    pub path: String,
    pub range: Option<TextRange>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPreviewProp {
    pub name: String,
    pub color_model: String,
    pub bulb_diameter_meters: f64,
    pub geometry_summary: String,
    pub render_plan: GeometryRenderPlan,
    pub source_path: String,
    pub object_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Rotation3Degrees {
    pub x_degrees: f64,
    pub y_degrees: f64,
    pub z_degrees: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Scale3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceAudio {
    #[serde(rename = "import")]
    pub import_path: String,
    pub resolved_path: String,
    pub file_name: String,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceCurveLibraryItem {
    pub module_id: String,
    pub path: String,
    pub object_key: String,
    pub display_name: String,
    pub points: Vec<SequenceCurvePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGradientLibraryItem {
    pub module_id: String,
    pub path: String,
    pub object_key: String,
    pub display_name: String,
    pub stops: Vec<SequenceGradientStop>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGuiDocument {
    pub path: String,
    pub source_ref: GuiObjectRef,
    pub object_key: String,
    pub duration_seconds: f64,
    pub frame_rate: f64,
    pub audio: Option<SequenceAudio>,
    pub mark_collections: Vec<SequenceMarkCollection>,
    pub lanes: Vec<SequenceLane>,
    pub effect_definitions: Vec<SequenceEffectDefinition>,
    pub curve_library: Vec<SequenceCurveLibraryItem>,
    pub gradient_library: Vec<SequenceGradientLibraryItem>,
    pub layers: Vec<SequenceLayer>,
    pub effects: Vec<SequenceEffect>,
    pub control_clips: Vec<SequenceControlClip>,
    pub composition_graph: SequenceCompositionGraph,
    pub automation_clips: Vec<SequenceAutomationClip>,
    pub mode: GuiDocumentMode,
    pub recovery_items: Vec<InvalidSequencePlaceholder>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GuiDocumentMode {
    Editable,
    Recovery,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InvalidSequencePlaceholder {
    pub kind: InvalidSequencePlaceholderKind,
    pub id: String,
    pub placement: InvalidSequencePlacement,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InvalidSequencePlaceholderKind {
    Effect,
    AutomationClip,
    ControlClip,
    GraphNode,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum InvalidSequencePlacement {
    Timeline {
        start_seconds: f64,
        duration_seconds: f64,
        lane: InvalidSequenceLane,
    },
    Graph {
        x: f64,
        y: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum InvalidSequenceLane {
    Layer { layer_id: u32 },
    Lane { lane_index: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceLayer {
    pub id: u32,
    pub name: String,
    pub color: String,
    pub enabled: bool,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceAutomationClip {
    pub id: u32,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub anchor_lane_index: u32,
    pub lane_index: u32,
    pub curve: Vec<SequenceCurvePoint>,
    pub bindings: Vec<SequenceAutomationBinding>,
    pub detached_bindings: Vec<SequenceDetachedAutomationBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceAutomationBinding {
    pub target: SequenceAutomationTarget,
    pub mapping: SequenceAutomationMapping,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceDetachedAutomationBinding {
    pub target: SequenceAutomationTarget,
    pub mapping: SequenceAutomationMapping,
    pub reason: SequenceAutomationDetachmentReason,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceAutomationDetachmentReason {
    TargetDeleted,
    DefinitionChanged,
    OperatorSchemaChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceAutomationTarget {
    EffectParam { effect_id: u32, param: String },
    CompositionNodeParam { node_id: String, param: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceAutomationMapping {
    Float { min: f64, max: f64 },
    Int { min: f64, max: f64 },
    Bool,
    Enum { values: Vec<String> },
    Curve { min: f64, max: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRasterRequest {
    #[serde(flatten)]
    pub document: GuiDocumentRequest,
    pub items: Vec<SequenceClipRasterRequestItem>,
    pub display_row_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRasterRequestItem {
    pub effect_id: u32,
    pub signature: Option<String>,
    pub display_column_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRasterResponse {
    pub project_revision: u32,
    pub request_id: u32,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRasterResultBatch {
    pub project_revision: u32,
    pub request_id: u32,
    pub ready: Vec<SequenceClipRaster>,
    pub unavailable: Vec<SequenceClipRasterUnavailable>,
    pub errors: Vec<SequenceClipRasterError>,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRaster {
    pub request_id: u32,
    pub effect_id: u32,
    pub signature: String,
    pub columns: u32,
    pub rows: u32,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub pixels_rgba_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRasterError {
    pub request_id: u32,
    pub effect_id: u32,
    pub signature: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceClipRasterUnavailable {
    pub request_id: u32,
    pub effect_id: u32,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffect {
    pub index: u32,
    pub id: u32,
    pub layer_id: u32,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub target: ElementTarget,
    pub target_label: String,
    pub scope: SequenceEffectScope,
    pub effect: String,
    pub effect_reference: SequenceEffectReference,
    pub params: Vec<SequenceEffectParam>,
    pub kind: SequenceTimelineClipKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceTimelineClipKind {
    Effect,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceControlClip {
    pub id: u32,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub anchor_lane_index: u32,
    pub lane_index: u32,
    pub target: ElementTarget,
    pub target_label: String,
    pub control_type: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceCompositionGraph {
    pub id: u32,
    pub operator_catalog: Vec<SequenceGraphOperatorDefinition>,
    pub nodes: Vec<SequenceGraphNode>,
    pub edges: Vec<SequenceGraphEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphNode {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub inputs: Vec<SequenceGraphPortDefinition>,
    pub outputs: Vec<SequenceGraphPortDefinition>,
    pub kind: SequenceGraphNodeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceGraphNodeKind {
    Layer {
        layer_id: u32,
        layer_name: String,
        layer_color: String,
        enabled: bool,
    },
    Operator {
        operator: SequenceGraphOperator,
        params: Vec<SequenceEffectParam>,
    },
    Output,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphEdge {
    pub from_node: String,
    pub from_port: String,
    pub to_node: String,
    pub to_port: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceGraphOperator {
    Builtin {
        operator: SequenceBuiltinOperator,
    },
    Custom {
        module_id: String,
        path: String,
        object_key: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceBuiltinOperator {
    Max,
    Add,
    Multiply,
    IntensityModulate,
    Dim,
    Invert,
    Colorize,
    Delay,
    Echo,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphOperatorDefinition {
    pub operator: SequenceGraphOperator,
    pub source_name: String,
    pub display_name: String,
    pub inputs: Vec<SequenceGraphPortDefinition>,
    pub outputs: Vec<SequenceGraphPortDefinition>,
    pub params: Vec<SequenceEffectDefinitionParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGraphPortDefinition {
    pub source_name: String,
    pub display_name: String,
    pub cardinality: SequenceGraphPortCardinality,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceGraphPortCardinality {
    One,
    Many,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceCurveSource {
    Inline,
    Library {
        reference: String,
        module_id: Option<String>,
        path: Option<String>,
        object_key: Option<String>,
        display_name: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceGradientSource {
    Inline,
    Library {
        reference: String,
        module_id: Option<String>,
        path: Option<String>,
        object_key: Option<String>,
        display_name: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectParam {
    pub name: String,
    pub kind: SequenceEffectParamKind,
    pub options: Vec<String>,
    pub editable: bool,
    pub value: SequenceEffectParamValue,
    pub curve_source: Option<SequenceCurveSource>,
    pub gradient_source: Option<SequenceGradientSource>,
    pub automation: Option<SequenceParamAutomation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceParamAutomation {
    pub clip_id: u32,
    pub mapping: SequenceAutomationMapping,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceEffectParamValue {
    Int {
        value: f64,
    },
    Float {
        value: f64,
    },
    Bool {
        value: bool,
    },
    Color {
        value: String,
    },
    Enum {
        value: String,
    },
    Curve {
        points: Vec<SequenceCurvePoint>,
    },
    Gradient {
        stops: Vec<SequenceGradientStop>,
    },
    IntArray {
        values: Vec<f64>,
    },
    FloatArray {
        values: Vec<f64>,
    },
    BoolArray {
        values: Vec<bool>,
    },
    ColorArray {
        values: Vec<String>,
    },
    CurveArray {
        values: Vec<Vec<SequenceCurvePoint>>,
    },
    GradientArray {
        values: Vec<Vec<SequenceGradientStop>>,
    },
    Marks {
        key: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectDefinition {
    pub name: String,
    pub kind: SequenceEffectDefinitionKind,
    pub effect: SequenceEffectReference,
    #[serde(rename = "import")]
    pub import_path: Option<String>,
    pub params: Vec<SequenceEffectDefinitionParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectDefinitionParam {
    pub name: String,
    pub kind: SequenceEffectParamKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceGuiEdit {
    SetDuration {
        duration_seconds: f64,
    },
    SetAudio {
        #[serde(rename = "import")]
        import_path: Option<String>,
    },
    MoveControlClip {
        id: u32,
        start_seconds: f64,
        anchor_lane_index: u32,
        lane_index: u32,
    },
    ResizeControlClip {
        id: u32,
        start_seconds: f64,
        duration_seconds: f64,
    },
    DeleteControlClip {
        id: u32,
    },
    AddEffect {
        effect: SequenceEffectReference,
        target: ElementTarget,
        scope: SequenceEffectScope,
        start_seconds: f64,
        mark_collection_key: Option<String>,
    },
    CreateLayer {
        name: String,
        color: String,
    },
    CreateLayerAt {
        name: String,
        color: String,
        x: f64,
        y: f64,
    },
    RenameLayer {
        id: u32,
        name: String,
    },
    SetLayerColor {
        id: u32,
        color: String,
    },
    SetLayerEnabled {
        id: u32,
        enabled: bool,
    },
    DeleteLayer {
        id: u32,
        migrate_to_layer_id: u32,
    },
    SetEffectLayer {
        id: u32,
        layer_id: u32,
    },
    MoveEffect {
        id: u32,
        start_seconds: f64,
        target: Option<ElementTarget>,
    },
    ResizeEffect {
        id: u32,
        start_seconds: f64,
        duration_seconds: f64,
    },
    ChangeEffectDefinition {
        id: u32,
        effect: SequenceEffectReference,
    },
    DeleteEffect {
        id: u32,
    },
    RetargetEffect {
        id: u32,
        target: ElementTarget,
    },
    SetEffectScope {
        id: u32,
        scope: SequenceEffectScope,
    },
    UpdateEffectParam {
        id: u32,
        name: String,
        value: SequenceEffectParamValue,
    },
    LinkEffectCurve {
        id: u32,
        name: String,
        source_module_id: String,
        source_path: String,
        object_key: String,
    },
    UnlinkEffectCurve {
        id: u32,
        name: String,
    },
    LinkEffectGradient {
        id: u32,
        name: String,
        source_module_id: String,
        source_path: String,
        object_key: String,
    },
    UnlinkEffectGradient {
        id: u32,
        name: String,
    },
    AddGraphOperatorNode {
        operator: SequenceGraphOperator,
        x: f64,
        y: f64,
    },
    MoveGraphNode {
        node_id: String,
        x: f64,
        y: f64,
    },
    DeleteGraphNode {
        node_id: String,
    },
    ConnectGraphNodes {
        from_node: String,
        from_port: String,
        to_node: String,
        to_port: String,
    },
    DisconnectGraphNodes {
        from_node: String,
        from_port: String,
        to_node: String,
        to_port: String,
    },
    UpdateGraphOperatorParam {
        node_id: String,
        name: String,
        value: SequenceEffectParamValue,
    },
    LinkGraphOperatorCurve {
        node_id: String,
        name: String,
        source_module_id: String,
        source_path: String,
        object_key: String,
    },
    UnlinkGraphOperatorCurve {
        node_id: String,
        name: String,
    },
    LinkGraphOperatorGradient {
        node_id: String,
        name: String,
        source_module_id: String,
        source_path: String,
        object_key: String,
    },
    UnlinkGraphOperatorGradient {
        node_id: String,
        name: String,
    },
    AddAutomationClip {
        start_seconds: f64,
        duration_seconds: f64,
        anchor_lane_index: u32,
        lane_index: u32,
    },
    CreateAndBindAutomationClip {
        target: SequenceAutomationTarget,
        mapping: SequenceAutomationMapping,
    },
    MoveAutomationClip {
        id: u32,
        start_seconds: f64,
        anchor_lane_index: u32,
        lane_index: u32,
    },
    ResizeAutomationClip {
        id: u32,
        start_seconds: f64,
        duration_seconds: f64,
    },
    UpdateAutomationCurve {
        id: u32,
        curve: Vec<SequenceCurvePoint>,
    },
    UpdateAutomationParamMapping {
        clip_id: u32,
        target: SequenceAutomationTarget,
        mapping: SequenceAutomationMapping,
    },
    DeleteAutomationClip {
        id: u32,
    },
    BindAutomationParam {
        clip_id: u32,
        target: SequenceAutomationTarget,
        mapping: SequenceAutomationMapping,
    },
    UnbindAutomationParam {
        clip_id: u32,
        target: SequenceAutomationTarget,
    },
    RebindDetachedAutomation {
        clip_id: u32,
        detached_index: u32,
        target: SequenceAutomationTarget,
        mapping: SequenceAutomationMapping,
    },
    DiscardDetachedAutomation {
        clip_id: u32,
        detached_index: u32,
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
    ReassignMarkCollection {
        collection_key: String,
        index: u32,
        target_collection_key: String,
    },
    DeleteMark {
        collection_key: String,
        index: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceLane {
    pub target: ElementTarget,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceMarkCollection {
    pub key: String,
    pub name: String,
    pub color: String,
    pub marks_seconds: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceMarkRef {
    pub collection_key: String,
    pub index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequencePasteAnchor {
    pub lane_index: u32,
    pub time_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceSelection {
    Effects { ids: Vec<u32> },
    Marks { marks: Vec<SequenceMarkRef> },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceSelectionEditResult {
    pub snapshot: AppSnapshot,
    pub document: GuiDocument,
    pub selection: Option<SequenceSelection>,
    pub copied_count: u32,
    pub skipped_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Transform {
    pub position: Point3Meters,
    pub rotation: Rotation3Degrees,
    pub scale: Scale3,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEntry {
    pub path: String,
    pub kind: WorkspaceEntryKind,
    pub name: String,
    pub parent: String,
    pub role: WorkspaceEntryRole,
    pub ownership: WorkspaceEntryOwnership,
    pub operations: Vec<WorkspaceOperation>,
    pub operation_explanation: Option<String>,
}
