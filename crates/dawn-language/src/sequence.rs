use crate::effect::{EffectInst, EffectInstId};
use crate::effect_dsl::types::Identifier;
use crate::values::{Color, Curve, DawnDuration, DawnTime};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SequenceId(pub String);

#[derive(Clone, Debug, PartialEq)]
pub struct Sequence {
    pub id: SequenceId,
    pub duration: DawnDuration,
    pub frame_rate: u32,
    pub audio: SequenceAudio,
    pub mark_collections: Vec<MarkCollection>,
    pub effects: Vec<EffectInst>,
    pub automation_clips: Vec<AutomationClip>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MarkCollection {
    pub key: MarkCollectionKey,
    pub name: String,
    pub display_color: Color,
    pub marks: Vec<DawnTime>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct MarkCollectionKey {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AutomationClip {
    pub id: AutomationClipId,
    pub start: DawnTime,
    pub duration: DawnDuration,
    pub anchor_lane_index: u32,
    pub lane_index: u32,
    pub curve: Curve,
    pub bindings: Vec<AutomationBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationClipId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct AutomationBinding {
    pub effect_id: EffectInstId,
    pub param: Identifier,
    pub mapping: AutomationMapping,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AutomationMapping {
    Float { min: f64, max: f64 },
    Int { min: i64, max: i64 },
    Bool,
    Enum { values: Vec<Identifier> },
    FloatCurve { min: f64, max: f64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SequenceAudio {
    None,
    Asset(AssetId),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AssetId(pub u32);
