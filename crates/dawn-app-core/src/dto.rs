use dawn_project::analysis::{DiagnosticSeverity, ProjectDiagnostic, TextRange};
use dawn_project::document::{
    default_sequence_effect_param, DocumentDescriptor, DocumentObjectDescriptor, DocumentViewId,
    EffectScriptReferenceDocument, FixtureDefinitionDocument, FixtureDocument, LayoutDocument,
    LayoutFixturePlacement, ResolvedLayoutFixture, SequenceAudioDocument,
    SequenceCurveLibraryItemDocument, SequenceDocument, SequenceEffectDocument,
    SequenceEffectParamCurvePointEditValue, SequenceEffectParamCurveSourceDocument,
    SequenceEffectParamCurveValueEditValue, SequenceEffectParamEditValue,
    SequenceEffectScriptDocument, SequenceEffectScriptParamDocument, SequenceLaneDocument,
};
use dawn_project::effect_script::{
    lex as lex_effect_script, parse_module as parse_effect_module, EffectParamSchema,
    EffectScriptKind, EffectVisibility, ScriptType,
};
use dawn_project::fs::{WorkspaceEntry, WorkspaceEntryKind};
use dawn_project::model::{
    Authored, ColorModel, Curve, CurveValue, CurveValueType, Distance, EffectParam, Geometry,
    InlineOrRef, LayoutTargetKind, ObjectKind, Point3, Rotation3, Scale3, SequenceEffectScope,
    Transform,
};
use dawn_project::path::PathStringExt;
use dawn_project::render::{
    GeometryRenderBounds, GeometryRenderGuide, GeometryRenderPlan, GeometryRenderPoint,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::app_model::{ActiveGuiDocument, AppSnapshot, LiveOutputSnapshot};
use crate::editor_session::{BufferExternalState, EditorBuffer, EditorViewMode};
use crate::preview_session::{AudioPlaybackStatus, PreviewTransportState};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshotDto {
    pub project_root: Option<String>,
    pub project_tree_visible: bool,
    pub project_entries: Vec<WorkspaceEntryDto>,
    pub tabs: Vec<EditorBufferDto>,
    pub active_file: Option<String>,
    pub active_buffer: Option<EditorBufferDto>,
    pub active_document_descriptor: Option<DocumentDescriptorDto>,
    pub active_gui_document: Option<ActiveGuiDocumentDto>,
    pub diagnostics: Vec<ProjectDiagnosticDto>,
    pub status: String,
    pub preview: PreviewSnapshotDto,
    pub effect_preview_enabled: bool,
    pub live_output: LiveOutputSnapshotDto,
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
    pub external_state: BufferExternalStateDto,
    pub view_mode: EditorViewModeDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BufferExternalStateDto {
    Current,
    ChangedOnDisk,
    DeletedOnDisk,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum EditorViewModeDto {
    Text,
    Gui,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDescriptorDto {
    pub path: String,
    pub objects: Vec<DocumentObjectDescriptorDto>,
    pub available_views: Vec<DocumentViewIdDto>,
    pub default_object_keys: Vec<DocumentDefaultObjectKeyDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDefaultObjectKeyDto {
    pub view: DocumentViewIdDto,
    pub object_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocumentObjectDescriptorDto {
    pub key: String,
    pub kind: ObjectKindDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DocumentViewIdDto {
    Text,
    Layout,
    Fixture,
    Sequence,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ObjectKindDto {
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
pub enum ActiveGuiDocumentDto {
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
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceGuiEditDto {
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
pub enum SequenceSelectionDto {
    Effects { ids: Vec<u32> },
    Marks { marks: Vec<SequenceMarkRefDto> },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceMarkRefDto {
    pub collection_key: String,
    pub index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequencePasteAnchorDto {
    pub lane_index: Option<u32>,
    pub time_seconds: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceSelectionEditResultDto {
    pub snapshot: AppSnapshotDto,
    pub selection: Option<SequenceSelectionDto>,
    pub copied_count: u32,
    pub skipped_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceSelectionEditDto {
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
pub enum SequenceResizeEdgeDto {
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LayoutGuiEditDto {
    UpdatePlacementTransform { id: u32, transform: TransformDto },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FixtureGuiEditDto {
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
pub struct LayoutTargetDto {
    pub kind: LayoutTargetKindDto,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum LayoutTargetKindDto {
    Group,
    Fixture,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TransformDto {
    pub position: Point3MetersDto,
    pub rotation: Rotation3DegreesDto,
    pub scale: Scale3Dto,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Point3MetersDto {
    pub x_meters: f64,
    pub y_meters: f64,
    pub z_meters: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Rotation3DegreesDto {
    pub x_degrees: f64,
    pub y_degrees: f64,
    pub z_degrees: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Scale3Dto {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceDocumentDto {
    pub path: String,
    pub object_key: String,
    pub duration_seconds: f64,
    pub frame_rate: u32,
    pub audio: Option<SequenceAudioDto>,
    pub mark_collections: Vec<SequenceMarkCollectionDto>,
    pub lanes: Vec<SequenceLaneDto>,
    pub effect_scripts: Vec<SequenceEffectScriptDto>,
    pub curve_library: Vec<SequenceCurveLibraryItemDto>,
    pub effects: Vec<SequenceEffectDto>,
    pub degraded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceMarkCollectionDto {
    pub key: String,
    pub name: String,
    pub color: String,
    pub marks_seconds: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceAudioDto {
    pub import: String,
    pub resolved_path: String,
    pub file_name: String,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceLaneDto {
    pub target: LayoutTargetDto,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectDto {
    pub index: u32,
    pub id: u32,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub target: LayoutTargetDto,
    pub target_label: String,
    pub scope: SequenceEffectScopeDto,
    pub script: String,
    pub script_source: Option<EffectScriptReferenceDto>,
    pub params: Vec<SequenceEffectParamDto>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceEffectScopeDto {
    PerFixture,
    WholeTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectParamDto {
    pub name: String,
    pub kind: SequenceEffectParamKindDto,
    pub options: Vec<String>,
    pub editable: bool,
    pub value: SequenceEffectParamValueDto,
    pub curve_source: Option<SequenceEffectParamCurveSourceDto>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceEffectParamKindDto {
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
pub enum SequenceEffectParamValueDto {
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
pub struct FloatCurvePointDto {
    pub time: f64,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ColorCurvePointDto {
    pub time: f64,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceCurveLibraryItemDto {
    pub path: String,
    pub object_key: String,
    pub display_name: String,
    pub value_type: SequenceCurveValueTypeDto,
    pub points: SequenceCurveLibraryPointsDto,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceCurveValueTypeDto {
    Float,
    Color,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceCurveLibraryPointsDto {
    Float { points: Vec<FloatCurvePointDto> },
    Color { points: Vec<ColorCurvePointDto> },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SequenceEffectParamCurveSourceDto {
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
pub struct SequenceEffectScriptDto {
    pub name: String,
    pub kind: SequenceEffectScriptKindDto,
    pub script: EffectScriptReferenceDto,
    pub import: String,
    pub params: Vec<SequenceEffectScriptParamDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EffectScriptReferenceDto {
    pub path: String,
    pub effect_name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SequenceEffectScriptKindDto {
    Sample,
    Generator,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectScriptParamDto {
    pub name: String,
    pub kind: SequenceEffectParamKindDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LayoutDocumentDto {
    pub path: String,
    pub object_key: String,
    pub name: String,
    pub render_bounds: GeometryRenderBoundsDto,
    pub fixtures: Vec<LayoutFixturePlacementDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LayoutFixturePlacementDto {
    pub id: u32,
    pub name: String,
    pub transform: TransformDto,
    pub resolved_fixture: ResolvedLayoutFixtureDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLayoutFixtureDto {
    pub name: String,
    pub color_model: String,
    pub bulb_diameter_meters: f64,
    pub geometry_summary: String,
    pub render_plan: GeometryRenderPlanDto,
    pub source_path: String,
    pub object_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FixtureDocumentDto {
    pub path: String,
    pub selected_object_key: Option<String>,
    pub fixtures: Vec<FixtureDefinitionDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FixtureDefinitionDto {
    pub object_key: String,
    pub name: String,
    pub color_model: String,
    pub bulb_diameter_meters: f64,
    pub geometry: GeometryDto,
    pub geometry_summary: String,
    pub render_plan: GeometryRenderPlanDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum GeometryDto {
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderPlanDto {
    pub emitters: Vec<GeometryRenderPointDto>,
    pub guides: Vec<GeometryRenderGuideDto>,
    pub bounds: GeometryRenderBoundsDto,
    pub bulb_radius_meters: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderPointDto {
    pub x_meters: f64,
    pub y_meters: f64,
    pub z_meters: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderBoundsDto {
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
pub enum GeometryRenderGuideDto {
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
    pub transport_state: PreviewTransportState,
    pub preview_updating: bool,
    pub position_seconds: f64,
    pub home_seconds: f64,
    pub duration_seconds: f64,
    pub audio: Option<SequenceAudioDto>,
    pub clock_source: String,
    pub audio_playback_status: AudioPlaybackStatus,
    pub frame_topology_identity: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LiveOutputSnapshotDto {
    pub enabled: bool,
    pub status: String,
    pub active_universe_count: u32,
    pub last_error: Option<String>,
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
            active_document_descriptor: snapshot
                .active_document_descriptor
                .map(DocumentDescriptorDto::from),
            active_gui_document: snapshot.active_gui_document.map(ActiveGuiDocumentDto::from),
            diagnostics: snapshot
                .diagnostics
                .into_iter()
                .map(ProjectDiagnosticDto::from)
                .collect(),
            status: snapshot.status,
            preview: PreviewSnapshotDto {
                source_label: snapshot.preview.source_label,
                transport_state: snapshot.preview.transport_state,
                preview_updating: snapshot.preview.preview_updating,
                position_seconds: snapshot.preview.position_seconds,
                home_seconds: snapshot.preview.home_seconds,
                duration_seconds: snapshot.preview.duration_seconds,
                audio: snapshot.preview.audio.map(SequenceAudioDto::from),
                clock_source: snapshot.preview.clock_source,
                audio_playback_status: snapshot.preview.audio_playback_status,
                frame_topology_identity: snapshot.preview.frame.topology_identity.stable_key(),
                status: snapshot.preview.status,
            },
            effect_preview_enabled: snapshot.workbench_layout.effect_preview_enabled,
            live_output: snapshot.live_output.into(),
        }
    }
}

impl From<LiveOutputSnapshot> for LiveOutputSnapshotDto {
    fn from(snapshot: LiveOutputSnapshot) -> Self {
        Self {
            enabled: snapshot.enabled,
            status: snapshot.status.label().to_string(),
            active_universe_count: snapshot.active_universe_count.min(u32::MAX as usize) as u32,
            last_error: snapshot.last_error,
        }
    }
}

impl From<EditorViewModeDto> for EditorViewMode {
    fn from(mode: EditorViewModeDto) -> Self {
        match mode {
            EditorViewModeDto::Text => Self::Text,
            EditorViewModeDto::Gui => Self::Gui,
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
                    view: DocumentViewIdDto::from(view),
                    object_key,
                })
                .collect(),
        }
    }
}

impl From<DocumentObjectDescriptor> for DocumentObjectDescriptorDto {
    fn from(object: DocumentObjectDescriptor) -> Self {
        Self {
            key: object.key,
            kind: ObjectKindDto::from(object.kind),
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
            ActiveGuiDocument::Blocked {
                reason,
                diagnostics,
            } => Self::Blocked {
                reason,
                diagnostics: diagnostics
                    .into_iter()
                    .map(ProjectDiagnosticDto::from)
                    .collect(),
            },
        }
    }
}

impl From<LayoutTargetDto> for dawn_project::document::LayoutTargetDocument {
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

impl From<dawn_project::document::LayoutTargetDocument> for LayoutTargetDto {
    fn from(target: dawn_project::document::LayoutTargetDocument) -> Self {
        Self {
            kind: match target.kind {
                LayoutTargetKind::Group => LayoutTargetKindDto::Group,
                LayoutTargetKind::Fixture => LayoutTargetKindDto::Fixture,
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

impl From<SequenceEffectScope> for SequenceEffectScopeDto {
    fn from(scope: SequenceEffectScope) -> Self {
        match scope {
            SequenceEffectScope::PerFixture => Self::PerFixture,
            SequenceEffectScope::WholeTarget => Self::WholeTarget,
        }
    }
}

impl From<SequenceDocument> for SequenceDocumentDto {
    fn from(document: SequenceDocument) -> Self {
        let mark_collection_key = document
            .mark_collections
            .first()
            .map(|collection| collection.key.clone());
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
                .filter_map(SequenceCurveLibraryItemDto::try_from_document)
                .collect(),
            effects: document
                .effects
                .into_iter()
                .map(|effect| {
                    SequenceEffectDto::from_document(effect, mark_collection_key.as_deref())
                })
                .collect(),
            degraded: document.degraded,
        }
    }
}

impl SequenceCurveLibraryItemDto {
    fn try_from_document(item: SequenceCurveLibraryItemDocument) -> Option<Self> {
        Some(Self {
            path: item.path,
            object_key: item.object_key,
            display_name: item.display_name,
            value_type: SequenceCurveValueTypeDto::from(item.value_type),
            points: SequenceCurveLibraryPointsDto::try_from_curve(&item.curve)?,
        })
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

impl SequenceCurveLibraryPointsDto {
    fn try_from_curve(curve: &Curve) -> Option<Self> {
        match curve_to_param_value(curve)? {
            SequenceEffectParamValueDto::FloatCurve { points } => Some(Self::Float { points }),
            SequenceEffectParamValueDto::ColorCurve { points } => Some(Self::Color { points }),
            _ => None,
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

impl From<dawn_project::document::SequenceMarkCollectionDocument> for SequenceMarkCollectionDto {
    fn from(collection: dawn_project::document::SequenceMarkCollectionDocument) -> Self {
        Self {
            key: collection.key,
            name: collection.name,
            color: collection.color,
            marks_seconds: collection.marks_seconds,
        }
    }
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

impl From<SequenceLaneDocument> for SequenceLaneDto {
    fn from(lane: SequenceLaneDocument) -> Self {
        Self {
            target: lane.target.into(),
            label: lane.label,
        }
    }
}

impl From<SequenceEffectScriptDocument> for SequenceEffectScriptDto {
    fn from(script: SequenceEffectScriptDocument) -> Self {
        Self {
            name: script.name,
            kind: match script.kind {
                EffectScriptKind::Sample => SequenceEffectScriptKindDto::Sample,
                EffectScriptKind::Generator => SequenceEffectScriptKindDto::Generator,
            },
            script: script.script.into(),
            import: script.import,
            params: script
                .params
                .into_iter()
                .filter_map(SequenceEffectScriptParamDto::try_from_document)
                .collect(),
        }
    }
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

impl SequenceEffectScriptParamDto {
    fn try_from_document(param: SequenceEffectScriptParamDocument) -> Option<Self> {
        Some(Self {
            name: param.name,
            kind: param_kind_from_script_type(param.value_type)?,
        })
    }
}

impl SequenceEffectDto {
    fn from_document(effect: SequenceEffectDocument, mark_collection_key: Option<&str>) -> Self {
        let script = effect.script;
        let script_source = effect.script_source.map(EffectScriptReferenceDto::from);
        let params = effect
            .render
            .as_ref()
            .map(|script_source| {
                sequence_effect_params_from_source(
                    &script,
                    &script_source.script_source,
                    &effect.params,
                    mark_collection_key,
                )
            })
            .unwrap_or_default();
        Self {
            index: effect.index.min(u32::MAX as usize) as u32,
            id: effect.id,
            start_seconds: effect.start_seconds,
            duration_seconds: effect.duration_seconds,
            target: effect.target.into(),
            target_label: effect.target_label,
            scope: effect.scope.into(),
            script,
            script_source,
            params,
        }
    }
}

fn sequence_effect_params_from_source(
    script: &str,
    script_source: &str,
    params: &[dawn_project::document::SequenceEffectParamDocument],
    mark_collection_key: Option<&str>,
) -> Vec<SequenceEffectParamDto> {
    let Some(schemas) = effect_param_schemas_from_source(script, script_source) else {
        return Vec::new();
    };
    schemas
        .iter()
        .filter_map(|schema| {
            let kind = param_kind_from_script_type(schema.value_type)?;
            let value = params
                .iter()
                .find(|param| param.name == schema.name)
                .and_then(|param| param_value_from_resolved(schema.value_type, &param.value))
                .filter(|value| param_value_options_match(value, &schema.options))
                .or_else(|| default_param_value(schema, mark_collection_key));
            Some(SequenceEffectParamDto {
                name: schema.name.clone(),
                kind,
                options: schema.options.clone(),
                editable: value.is_some(),
                value: value?,
                curve_source: params
                    .iter()
                    .find(|param| param.name == schema.name)
                    .and_then(|param| param.curve_source.clone())
                    .map(SequenceEffectParamCurveSourceDto::from),
            })
        })
        .collect()
}

fn effect_param_schemas_from_source(
    script: &str,
    script_source: &str,
) -> Option<Vec<EffectParamSchema>> {
    let tokens = lex_effect_script(script_source).ok()?;
    let module = parse_effect_module(&tokens).ok()?;
    let selected_name = effect_name_from_script_label(script);
    if let Some(selected_name) = selected_name {
        return module
            .effects
            .into_iter()
            .find(|effect| effect.name == selected_name)
            .map(|effect| effect.params);
    }
    if module.effects.len() == 1 {
        return module
            .effects
            .into_iter()
            .next()
            .map(|effect| effect.params);
    }
    let mut addable = module
        .effects
        .into_iter()
        .filter(|effect| effect.visibility == EffectVisibility::Addable)
        .collect::<Vec<_>>();
    if addable.len() == 1 {
        Some(addable.remove(0).params)
    } else {
        None
    }
}

fn effect_name_from_script_label(script: &str) -> Option<&str> {
    script
        .rsplit_once('.')
        .map(|(_, name)| name)
        .filter(|name| !name.is_empty())
}

fn param_kind_from_script_type(value_type: ScriptType) -> Option<SequenceEffectParamKindDto> {
    match value_type {
        ScriptType::Int => Some(SequenceEffectParamKindDto::Int),
        ScriptType::Float => Some(SequenceEffectParamKindDto::Float),
        ScriptType::Bool => Some(SequenceEffectParamKindDto::Bool),
        ScriptType::Color => Some(SequenceEffectParamKindDto::Color),
        ScriptType::Enum => Some(SequenceEffectParamKindDto::Enum),
        ScriptType::Flags => Some(SequenceEffectParamKindDto::Flags),
        ScriptType::CurveFloat => Some(SequenceEffectParamKindDto::FloatCurve),
        ScriptType::CurveColor => Some(SequenceEffectParamKindDto::ColorCurve),
        ScriptType::Marks => Some(SequenceEffectParamKindDto::Marks),
        ScriptType::Fixture
        | ScriptType::Pixel
        | ScriptType::Timeline
        | ScriptType::Target
        | ScriptType::TargetItems
        | ScriptType::TargetItem
        | ScriptType::Void => None,
    }
}

fn default_param_value(
    schema: &dawn_project::effect_script::EffectParamSchema,
    mark_collection_key: Option<&str>,
) -> Option<SequenceEffectParamValueDto> {
    let value = default_sequence_effect_param(schema, mark_collection_key);
    param_value_from_authored(schema.value_type, &value)
}

fn param_value_from_resolved(
    value_type: ScriptType,
    value: &EffectParam<dawn_project::model::Resolved>,
) -> Option<SequenceEffectParamValueDto> {
    match (value_type, value) {
        (ScriptType::Int, EffectParam::Integer { value }) => {
            Some(SequenceEffectParamValueDto::Int {
                value: (*value).min(u32::MAX as u64) as u32,
            })
        }
        (ScriptType::Float, EffectParam::Float { value }) if value.is_finite() => {
            Some(SequenceEffectParamValueDto::Float { value: *value })
        }
        (ScriptType::Bool, EffectParam::Boolean { value }) => {
            Some(SequenceEffectParamValueDto::Bool { value: *value })
        }
        (ScriptType::Color, EffectParam::Color { value }) => {
            Some(SequenceEffectParamValueDto::Color {
                value: value.to_hex(),
            })
        }
        (ScriptType::Enum, EffectParam::Enum { value }) => {
            Some(SequenceEffectParamValueDto::Enum {
                value: value.clone(),
            })
        }
        (ScriptType::Flags, EffectParam::Flags { value }) => {
            Some(SequenceEffectParamValueDto::Flags {
                value: value.values.clone(),
            })
        }
        (ScriptType::CurveFloat, EffectParam::Curve { curve })
            if curve.value_type == dawn_project::model::CurveValueType::Float =>
        {
            curve_to_param_value(curve)
        }
        (ScriptType::CurveColor, EffectParam::Curve { curve })
            if curve.value_type == dawn_project::model::CurveValueType::Color =>
        {
            curve_to_param_value(curve)
        }
        (ScriptType::Marks, EffectParam::Marks { key }) => {
            Some(SequenceEffectParamValueDto::Marks { key: key.clone() })
        }
        _ => None,
    }
}

fn param_value_from_authored(
    value_type: ScriptType,
    value: &EffectParam<Authored>,
) -> Option<SequenceEffectParamValueDto> {
    match (value_type, value) {
        (ScriptType::Int, EffectParam::Integer { value }) => {
            Some(SequenceEffectParamValueDto::Int {
                value: (*value).min(u32::MAX as u64) as u32,
            })
        }
        (ScriptType::Float, EffectParam::Float { value }) if value.is_finite() => {
            Some(SequenceEffectParamValueDto::Float { value: *value })
        }
        (ScriptType::Bool, EffectParam::Boolean { value }) => {
            Some(SequenceEffectParamValueDto::Bool { value: *value })
        }
        (ScriptType::Color, EffectParam::Color { value }) => {
            Some(SequenceEffectParamValueDto::Color {
                value: value.to_hex(),
            })
        }
        (ScriptType::Enum, EffectParam::Enum { value }) => {
            Some(SequenceEffectParamValueDto::Enum {
                value: value.clone(),
            })
        }
        (ScriptType::Flags, EffectParam::Flags { value }) => {
            Some(SequenceEffectParamValueDto::Flags {
                value: value.values.clone(),
            })
        }
        (
            ScriptType::CurveFloat,
            EffectParam::Curve {
                curve: InlineOrRef::Inline(curve),
            },
        ) if curve.value_type == dawn_project::model::CurveValueType::Float => {
            curve_to_param_value(curve)
        }
        (
            ScriptType::CurveColor,
            EffectParam::Curve {
                curve: InlineOrRef::Inline(curve),
            },
        ) if curve.value_type == dawn_project::model::CurveValueType::Color => {
            curve_to_param_value(curve)
        }
        (ScriptType::Marks, EffectParam::Marks { key }) => {
            Some(SequenceEffectParamValueDto::Marks { key: key.clone() })
        }
        _ => None,
    }
}

fn curve_to_param_value(curve: &Curve) -> Option<SequenceEffectParamValueDto> {
    match curve.value_type {
        dawn_project::model::CurveValueType::Float => {
            Some(SequenceEffectParamValueDto::FloatCurve {
                points: curve
                    .points
                    .iter()
                    .filter_map(|point| match point.value {
                        CurveValue::Float(value) if point.time.is_finite() && value.is_finite() => {
                            Some(FloatCurvePointDto {
                                time: point.time,
                                value,
                            })
                        }
                        _ => None,
                    })
                    .collect(),
            })
        }
        dawn_project::model::CurveValueType::Color => {
            Some(SequenceEffectParamValueDto::ColorCurve {
                points: curve
                    .points
                    .iter()
                    .filter_map(|point| match point.value {
                        CurveValue::Color(value) if point.time.is_finite() => {
                            Some(ColorCurvePointDto {
                                time: point.time,
                                value: value.to_hex(),
                            })
                        }
                        _ => None,
                    })
                    .collect(),
            })
        }
    }
}

fn param_value_options_match(value: &SequenceEffectParamValueDto, options: &[String]) -> bool {
    match value {
        SequenceEffectParamValueDto::Enum { value } => options.contains(value),
        SequenceEffectParamValueDto::Flags { value } => {
            value.iter().all(|flag| options.contains(flag))
        }
        _ => true,
    }
}

impl From<SequenceEffectParamValueDto> for SequenceEffectParamEditValue {
    fn from(value: SequenceEffectParamValueDto) -> Self {
        match value {
            SequenceEffectParamValueDto::Int { value } => Self::Integer(value.into()),
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

impl From<LayoutDocument> for LayoutDocumentDto {
    fn from(document: LayoutDocument) -> Self {
        Self {
            path: document.path,
            object_key: document.object_key,
            name: document.name,
            render_bounds: document.render_bounds.into(),
            fixtures: document
                .fixtures
                .into_iter()
                .map(LayoutFixturePlacementDto::from)
                .collect(),
        }
    }
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

impl From<FixtureDocument> for FixtureDocumentDto {
    fn from(document: FixtureDocument) -> Self {
        Self {
            path: document.path,
            selected_object_key: document.selected_object_key,
            fixtures: document
                .fixtures
                .into_iter()
                .map(FixtureDefinitionDto::from)
                .collect(),
        }
    }
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

impl From<Point3> for Point3MetersDto {
    fn from(point: Point3) -> Self {
        Self {
            x_meters: point.x.as_meters_f64(),
            y_meters: point.y.as_meters_f64(),
            z_meters: point.z.as_meters_f64(),
        }
    }
}

impl From<Geometry> for GeometryDto {
    fn from(geometry: Geometry) -> Self {
        match geometry {
            Geometry::Points { points } => Self::Points {
                points: points.into_iter().map(Point3MetersDto::from).collect(),
            },
            Geometry::Lines { points, pixels } => Self::Lines {
                points: points.into_iter().map(Point3MetersDto::from).collect(),
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

impl From<GeometryRenderPlan> for GeometryRenderPlanDto {
    fn from(plan: GeometryRenderPlan) -> Self {
        Self {
            emitters: plan
                .emitters
                .into_iter()
                .map(GeometryRenderPointDto::from)
                .collect(),
            guides: plan
                .guides
                .into_iter()
                .map(GeometryRenderGuideDto::from)
                .collect(),
            bounds: plan.bounds.into(),
            bulb_radius_meters: plan.bulb_radius.as_meters_f64(),
        }
    }
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

impl TryFrom<TransformDto> for Transform {
    type Error = &'static str;

    fn try_from(transform: TransformDto) -> Result<Self, Self::Error> {
        validate_finite(transform.rotation.x_degrees, "rotation x degrees")?;
        validate_finite(transform.rotation.y_degrees, "rotation y degrees")?;
        validate_finite(transform.rotation.z_degrees, "rotation z degrees")?;
        validate_finite(transform.scale.x, "scale x")?;
        validate_finite(transform.scale.y, "scale y")?;
        validate_finite(transform.scale.z, "scale z")?;
        Ok(Self {
            position: Point3::try_from(transform.position)?,
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

impl TryFrom<Point3MetersDto> for Point3 {
    type Error = &'static str;

    fn try_from(point: Point3MetersDto) -> Result<Self, Self::Error> {
        Ok(Self {
            x: Distance::try_from_meters_f64_truncated(point.x_meters)?,
            y: Distance::try_from_meters_f64_truncated(point.y_meters)?,
            z: Distance::try_from_meters_f64_truncated(point.z_meters)?,
        })
    }
}

fn validate_finite(value: f64, label: &'static str) -> Result<(), &'static str> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(label)
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
            external_state: match buffer.external_state {
                BufferExternalState::Current => BufferExternalStateDto::Current,
                BufferExternalState::ChangedOnDisk => BufferExternalStateDto::ChangedOnDisk,
                BufferExternalState::DeletedOnDisk => BufferExternalStateDto::DeletedOnDisk,
            },
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
