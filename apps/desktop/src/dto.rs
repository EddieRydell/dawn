use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub project_root: Option<String>,
    pub project_tree_visible: bool,
    pub project_entries: Vec<WorkspaceEntry>,
    pub tabs: Vec<EditorBuffer>,
    pub active_file: Option<String>,
    pub active_buffer: Option<EditorBuffer>,
    pub active_document_descriptor: Option<DocumentDescriptor>,
    pub active_gui_document: Option<ActiveGuiDocument>,
    pub diagnostics: Vec<ProjectDiagnostic>,
    pub status: String,
    pub sequence_transport: SequenceTransportSnapshot,
    pub live_output: LiveOutputSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ActiveGuiDocument {
    Sequence {
        document: SequenceEditorDocument,
    },
    Layout {
        document: LayoutDocument,
    },
    Fixture {
        document: FixtureDocument,
    },
    Blocked {
        reason: String,
        diagnostics: Vec<ProjectDiagnostic>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AudioPlaybackStatus {
    None,
    Missing,
    Ready,
    Playing,
    Ended,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BufferExternalState {
    Current,
    ChangedOnDisk,
    DeletedOnDisk,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ColorCurvePoint {
    pub time: f64,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DocumentViewId {
    Text,
    Layout,
    Fixture,
    Sequence,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum EditorViewMode {
    Text,
    Gui,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum LayoutTargetKind {
    Group,
    Fixture,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ObjectKind {
    Project,
    Setup,
    Controller,
    Layout,
    Fixture,
    Patch,
    Sequence,
    Curve,
    Effect,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceCurveValueType {
    Float,
    Color,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceEffectParamKind {
    Int,
    Float,
    Bool,
    Color,
    Enum,
    Flags,
    FloatCurve,
    ColorCurve,
    IntArray,
    FloatArray,
    BoolArray,
    ColorArray,
    FloatCurveArray,
    ColorCurveArray,
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
pub enum SequenceEffectScriptKind {
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
pub enum SequenceTransportState {
    Stopped,
    Paused,
    Playing,
    Ended,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceEntryKind {
    Directory,
    File,
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
    pub view_mode: EditorViewMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EffectScriptReference {
    pub path: String,
    pub effect_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FixtureDefinition {
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
pub struct FixtureDocument {
    pub path: String,
    pub selected_object_key: Option<String>,
    pub fixtures: Vec<FixtureDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FixtureGuiEdit {
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
pub struct FloatCurvePoint {
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
pub struct LayoutDocument {
    pub path: String,
    pub object_key: String,
    pub name: String,
    pub render_bounds: GeometryRenderBounds,
    pub fixtures: Vec<LayoutFixturePlacement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LayoutFixturePlacement {
    pub id: u32,
    pub name: String,
    pub transform: Transform,
    pub resolved_fixture: ResolvedLayoutFixture,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LayoutGuiEdit {
    UpdatePlacementTransform { id: u32, transform: Transform },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LayoutTarget {
    pub kind: LayoutTargetKind,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LiveOutputSnapshot {
    pub enabled: bool,
    pub status: String,
    pub active_universe_count: u32,
    pub last_error: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLayoutFixture {
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
    pub path: String,
    pub object_key: String,
    pub display_name: String,
    pub value_type: SequenceCurveValueType,
    pub points: SequenceCurveLibraryPoints,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceCurveLibraryPoints {
    Float { points: Vec<FloatCurvePoint> },
    Color { points: Vec<ColorCurvePoint> },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEditorDocument {
    pub path: String,
    pub object_key: String,
    pub duration_seconds: f64,
    pub frame_rate: f64,
    pub audio: Option<SequenceAudio>,
    pub mark_collections: Vec<SequenceMarkCollection>,
    pub lanes: Vec<SequenceLane>,
    pub effect_scripts: Vec<SequenceEffectScript>,
    pub curve_library: Vec<SequenceCurveLibraryItem>,
    pub effects: Vec<SequenceEffect>,
    pub degraded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffect {
    pub index: u32,
    pub id: u32,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub target: LayoutTarget,
    pub target_label: String,
    pub scope: SequenceEffectScope,
    pub script: String,
    pub script_source: Option<EffectScriptReference>,
    pub params: Vec<SequenceEffectParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceEffectParamCurveSource {
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
pub struct SequenceEffectParam {
    pub name: String,
    pub kind: SequenceEffectParamKind,
    pub options: Vec<String>,
    pub editable: bool,
    pub value: SequenceEffectParamValue,
    pub curve_source: Option<SequenceEffectParamCurveSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceEffectParamValue {
    Int { value: f64 },
    Float { value: f64 },
    Bool { value: bool },
    Color { value: String },
    Enum { value: String },
    Flags { value: Vec<String> },
    FloatCurve { points: Vec<FloatCurvePoint> },
    ColorCurve { points: Vec<ColorCurvePoint> },
    IntArray { values: Vec<f64> },
    FloatArray { values: Vec<f64> },
    BoolArray { values: Vec<bool> },
    ColorArray { values: Vec<String> },
    FloatCurveArray { values: Vec<Vec<FloatCurvePoint>> },
    ColorCurveArray { values: Vec<Vec<ColorCurvePoint>> },
    Marks { key: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectScript {
    pub name: String,
    pub kind: SequenceEffectScriptKind,
    pub script: EffectScriptReference,
    #[serde(rename = "import")]
    pub import_path: String,
    pub params: Vec<SequenceEffectScriptParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectScriptParam {
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
    SetAudio {
        #[serde(rename = "import")]
        import_path: Option<String>,
    },
    AddEffect {
        script: EffectScriptReference,
        target: LayoutTarget,
        scope: SequenceEffectScope,
        start_seconds: f64,
        mark_collection_key: Option<String>,
    },
    MoveEffect {
        id: u32,
        start_seconds: f64,
        target: Option<LayoutTarget>,
    },
    ResizeEffect {
        id: u32,
        start_seconds: f64,
        duration_seconds: f64,
    },
    ChangeEffectScript {
        id: u32,
        script: EffectScriptReference,
    },
    DeleteEffect {
        id: u32,
    },
    RetargetEffect {
        id: u32,
        target: LayoutTarget,
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
#[serde(rename_all = "camelCase")]
pub struct SequenceKey {
    pub path: String,
    pub object_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceLane {
    pub target: LayoutTarget,
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
    pub selection: Option<SequenceSelection>,
    pub copied_count: u32,
    pub skipped_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceTransportSnapshot {
    pub source_label: String,
    pub source_key: Option<SequenceKey>,
    pub render_generation: u32,
    pub render_dirty_revision: u32,
    pub transport_state: SequenceTransportState,
    pub render_updating: bool,
    pub position_seconds: f64,
    pub home_seconds: f64,
    pub duration_seconds: f64,
    pub audio: Option<SequenceAudio>,
    pub clock_source: String,
    pub audio_playback_status: AudioPlaybackStatus,
    pub geometry_identity: String,
    pub status: String,
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
}
