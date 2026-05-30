use dawn_project::analysis::{DiagnosticSeverity, ProjectDiagnostic, TextRange};
use dawn_project::document::{
    DocumentDescriptor, DocumentObjectDescriptor, DocumentViewId, FixtureDefinitionDocument,
    FixtureDocument, LayoutDocument, LayoutFixturePlacement, ResolvedLayoutFixture,
    SequenceAudioDocument, SequenceDocument, SequenceEffectDocument,
    SequenceEffectParamCurvePointEditValue, SequenceEffectParamCurveValueEditValue,
    SequenceEffectParamEditValue, SequenceEffectScriptDocument, SequenceEffectScriptParamDocument,
    SequenceLaneDocument,
};
use dawn_project::effect_script::{
    compile as compile_effect_script, ParamDefault, RuntimeValue, ScriptType,
};
use dawn_project::fs::{WorkspaceEntry, WorkspaceEntryKind};
use dawn_project::model::{
    ColorModel, Curve, CurveValue, EffectParam, Geometry, LayoutTargetKind, ObjectKind, Point3,
    Rotation3, Scale3, SequenceEffectScope, Transform,
};
use dawn_project::path::PathStringExt;
use dawn_project::render::{
    GeometryRenderBounds, GeometryRenderGuide, GeometryRenderPlan, GeometryRenderPoint,
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::app_model::{ActiveGuiDocument, AppSnapshot};
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
    pub active_document_descriptor: Option<DocumentDescriptorDto>,
    pub active_gui_document: Option<ActiveGuiDocumentDto>,
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
        script_path: String,
        target: LayoutTargetDto,
        scope: SequenceEffectScopeDto,
        start_ms: u32,
        mark_collection_key: Option<String>,
    },
    MoveEffect {
        id: u32,
        start_ms: u32,
        target: Option<LayoutTargetDto>,
    },
    ResizeEffect {
        id: u32,
        start_ms: u32,
        duration_ms: u32,
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
        time_ms: u32,
    },
    MoveMark {
        collection_key: String,
        index: u32,
        time_ms: u32,
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
    UpdateBulbSize {
        object_key: String,
        bulb_size: f64,
    },
    MovePoint {
        object_key: String,
        point_index: u32,
        point: Point3Dto,
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
    pub position: Point3Dto,
    pub rotation: Point3Dto,
    pub scale: Point3Dto,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Point3Dto {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceDocumentDto {
    pub path: String,
    pub object_key: String,
    pub duration_ms: u32,
    pub frame_rate: u32,
    pub audio: Option<SequenceAudioDto>,
    pub mark_collections: Vec<SequenceMarkCollectionDto>,
    pub lanes: Vec<SequenceLaneDto>,
    pub effect_scripts: Vec<SequenceEffectScriptDto>,
    pub effects: Vec<SequenceEffectDto>,
    pub degraded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceMarkCollectionDto {
    pub key: String,
    pub name: String,
    pub color: String,
    pub marks_ms: Vec<u32>,
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
    pub start_ms: u32,
    pub duration_ms: u32,
    pub target: LayoutTargetDto,
    pub target_label: String,
    pub scope: SequenceEffectScopeDto,
    pub script: String,
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
pub struct SequenceEffectScriptDto {
    pub name: String,
    pub path: String,
    pub import: String,
    pub params: Vec<SequenceEffectScriptParamDto>,
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
    pub units: String,
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
    pub bulb_size: f64,
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
    pub bulb_size: f64,
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
        points: Vec<Point3Dto>,
    },
    Lines {
        points: Vec<Point3Dto>,
        pixels: u32,
    },
    Arc {
        center: Point3Dto,
        radius: f64,
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
    pub bulb_radius: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderPointDto {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GeometryRenderBoundsDto {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
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
        radius_x: f64,
        radius_y: f64,
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
    pub is_playing: bool,
    pub position_ms: u32,
    pub home_ms: u32,
    pub duration_ms: u32,
    pub audio: Option<SequenceAudioDto>,
    pub clock_source: String,
    pub audio_playback_status: String,
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
                is_playing: snapshot.preview.is_playing,
                position_ms: snapshot.preview.position_ms.min(u32::MAX as u64) as u32,
                home_ms: snapshot.preview.home_ms.min(u32::MAX as u64) as u32,
                duration_ms: snapshot.preview.duration_ms.min(u32::MAX as u64) as u32,
                audio: snapshot.preview.audio.map(SequenceAudioDto::from),
                clock_source: snapshot.preview.clock_source,
                audio_playback_status: snapshot.preview.audio_playback_status,
                status: snapshot.preview.status,
            },
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
        Self {
            path: document.path,
            object_key: document.object_key,
            duration_ms: u32_ms(document.duration_ms),
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
            effects: document
                .effects
                .into_iter()
                .map(SequenceEffectDto::from)
                .collect(),
            degraded: document.degraded,
        }
    }
}

impl From<dawn_project::document::SequenceMarkCollectionDocument> for SequenceMarkCollectionDto {
    fn from(collection: dawn_project::document::SequenceMarkCollectionDocument) -> Self {
        Self {
            key: collection.key,
            name: collection.name,
            color: collection.color,
            marks_ms: collection.marks_ms.into_iter().map(u32_ms).collect(),
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
            path: script.path,
            import: script.import,
            params: script
                .params
                .into_iter()
                .filter_map(SequenceEffectScriptParamDto::try_from_document)
                .collect(),
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

impl From<SequenceEffectDocument> for SequenceEffectDto {
    fn from(effect: SequenceEffectDocument) -> Self {
        let params = effect
            .script_source
            .as_ref()
            .map(|script_source| sequence_effect_params_from_source(script_source, &effect.params))
            .unwrap_or_default();
        Self {
            index: effect.index.min(u32::MAX as usize) as u32,
            id: effect.id,
            start_ms: u32_ms(effect.start_ms),
            duration_ms: u32_ms(effect.duration_ms),
            target: effect.target.into(),
            target_label: effect.target_label,
            scope: effect.scope.into(),
            script: effect.script,
            params,
        }
    }
}

fn sequence_effect_params_from_source(
    script_source: &str,
    params: &[dawn_project::document::SequenceEffectParamDocument],
) -> Vec<SequenceEffectParamDto> {
    let Ok(script) = compile_effect_script(script_source) else {
        return Vec::new();
    };
    script
        .params
        .iter()
        .filter_map(|schema| {
            let kind = param_kind_from_script_type(schema.value_type)?;
            let value = params
                .iter()
                .find(|param| param.name == schema.name)
                .and_then(|param| param_value_from_resolved(schema.value_type, &param.value))
                .filter(|value| param_value_options_match(value, &schema.options))
                .or_else(|| default_param_value(schema));
            Some(SequenceEffectParamDto {
                name: schema.name.clone(),
                kind,
                options: schema.options.clone(),
                editable: value.is_some(),
                value: value?,
            })
        })
        .collect()
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
        ScriptType::Fixture | ScriptType::Pixel | ScriptType::Void => None,
    }
}

fn default_param_value(
    schema: &dawn_project::effect_script::EffectParamSchema,
) -> Option<SequenceEffectParamValueDto> {
    match &schema.default {
        Some(ParamDefault::Value(value)) => runtime_value_to_param_value(value),
        None => match schema.value_type {
            ScriptType::Int => Some(SequenceEffectParamValueDto::Int { value: 0 }),
            ScriptType::Float => Some(SequenceEffectParamValueDto::Float { value: 0.0 }),
            ScriptType::Bool => Some(SequenceEffectParamValueDto::Bool { value: false }),
            ScriptType::Color => Some(SequenceEffectParamValueDto::Color {
                value: "#ffffff".to_string(),
            }),
            ScriptType::Enum => Some(SequenceEffectParamValueDto::Enum {
                value: schema.options.first().cloned().unwrap_or_default(),
            }),
            ScriptType::Flags => Some(SequenceEffectParamValueDto::Flags { value: Vec::new() }),
            ScriptType::CurveFloat => Some(SequenceEffectParamValueDto::FloatCurve {
                points: vec![
                    FloatCurvePointDto {
                        time: 0.0,
                        value: 1.0,
                    },
                    FloatCurvePointDto {
                        time: 1.0,
                        value: 0.0,
                    },
                ],
            }),
            ScriptType::CurveColor => Some(SequenceEffectParamValueDto::ColorCurve {
                points: vec![ColorCurvePointDto {
                    time: 0.0,
                    value: "#ffffff".to_string(),
                }],
            }),
            ScriptType::Marks => None,
            ScriptType::Fixture | ScriptType::Pixel | ScriptType::Void => None,
        },
    }
}

fn runtime_value_to_param_value(value: &RuntimeValue) -> Option<SequenceEffectParamValueDto> {
    match value {
        RuntimeValue::Int(value) => Some(SequenceEffectParamValueDto::Int {
            value: (*value).max(0).min(u32::MAX as i64) as u32,
        }),
        RuntimeValue::Float(value) => Some(SequenceEffectParamValueDto::Float { value: *value }),
        RuntimeValue::Bool(value) => Some(SequenceEffectParamValueDto::Bool { value: *value }),
        RuntimeValue::Color(value) => Some(SequenceEffectParamValueDto::Color {
            value: value.to_hex(),
        }),
        RuntimeValue::Curve(curve) => curve_to_param_value(curve),
        RuntimeValue::Enum(value) => Some(SequenceEffectParamValueDto::Enum {
            value: value.clone(),
        }),
        RuntimeValue::Flags(value) => Some(SequenceEffectParamValueDto::Flags {
            value: value.values.clone(),
        }),
        RuntimeValue::Marks(_) => None,
        RuntimeValue::Fixture(_) | RuntimeValue::Pixel(_) => None,
    }
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
            units: format!("{:?}", document.units).to_ascii_lowercase(),
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
            bulb_size: fixture.bulb_size,
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
            bulb_size: fixture.bulb_size,
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
            rotation: Point3Dto {
                x: transform.rotation.x,
                y: transform.rotation.y,
                z: transform.rotation.z,
            },
            scale: Point3Dto {
                x: transform.scale.x,
                y: transform.scale.y,
                z: transform.scale.z,
            },
        }
    }
}

impl From<Point3> for Point3Dto {
    fn from(point: Point3) -> Self {
        Self {
            x: point.x,
            y: point.y,
            z: point.z,
        }
    }
}

impl From<Geometry> for GeometryDto {
    fn from(geometry: Geometry) -> Self {
        match geometry {
            Geometry::Points { points } => Self::Points {
                points: points.into_iter().map(Point3Dto::from).collect(),
            },
            Geometry::Lines { points, pixels } => Self::Lines {
                points: points.into_iter().map(Point3Dto::from).collect(),
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
                radius,
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
            bulb_radius: plan.bulb_radius,
        }
    }
}

impl From<GeometryRenderPoint> for GeometryRenderPointDto {
    fn from(point: GeometryRenderPoint) -> Self {
        Self {
            x: point.x,
            y: point.y,
            z: point.z,
        }
    }
}

impl From<GeometryRenderBounds> for GeometryRenderBoundsDto {
    fn from(bounds: GeometryRenderBounds) -> Self {
        Self {
            min_x: bounds.min_x,
            min_y: bounds.min_y,
            max_x: bounds.max_x,
            max_y: bounds.max_y,
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
                radius_x,
                radius_y,
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

fn u32_ms(value: u64) -> u32 {
    value.min(u32::MAX as u64) as u32
}

impl From<TransformDto> for Transform {
    fn from(transform: TransformDto) -> Self {
        Self {
            position: Point3::from(transform.position),
            rotation: Rotation3 {
                x: transform.rotation.x,
                y: transform.rotation.y,
                z: transform.rotation.z,
            },
            scale: Scale3 {
                x: transform.scale.x,
                y: transform.scale.y,
                z: transform.scale.z,
            },
        }
    }
}

impl From<Point3Dto> for Point3 {
    fn from(point: Point3Dto) -> Self {
        Self {
            x: point.x,
            y: point.y,
            z: point.z,
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
