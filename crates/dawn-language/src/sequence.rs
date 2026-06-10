use crate::effect::{CurveId, EffectInst, EffectInstId};
use crate::values::{Color, DawnDuration, DawnTime};

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
    pub targets: Vec<EffectInstId>,
    pub start: DawnTime,
    pub duration: DawnDuration,
    pub curve: CurveId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationClipId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SequenceAudio {
    None,
    Asset(AssetId),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AssetId(pub u32);
