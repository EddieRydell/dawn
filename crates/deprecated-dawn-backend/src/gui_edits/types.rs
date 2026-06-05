use dawn_language::document::{EffectScriptReferenceDocument, LayoutTargetDocument};
use dawn_language::model::{Point3, SequenceEffectScope, Transform};

#[derive(Debug, Clone)]
pub enum SequenceGuiEdit {
    SetAudio {
        import: Option<String>,
    },
    AddEffect {
        script: EffectScriptReferenceDocument,
        target: LayoutTargetDocument,
        scope: SequenceEffectScope,
        start_seconds: f64,
        mark_collection_key: Option<String>,
    },
    MoveEffect {
        id: u32,
        start_seconds: f64,
        target: Option<LayoutTargetDocument>,
    },
    ResizeEffect {
        id: u32,
        start_seconds: f64,
        duration_seconds: f64,
    },
    ChangeEffectScript {
        id: u32,
        script: EffectScriptReferenceDocument,
    },
    DeleteEffect {
        id: u32,
    },
    RetargetEffect {
        id: u32,
        target: LayoutTargetDocument,
    },
    SetEffectScope {
        id: u32,
        scope: SequenceEffectScope,
    },
    UpdateEffectParam {
        id: u32,
        name: String,
        value: dawn_language::document::SequenceEffectParamEditValue,
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
