use crate::effect::{CurveDefinitionKey, EffectInst, EffectInstId};
use crate::values::{Color, DawnDuration, DawnTime};

pub struct Sequence {
    pub duration: DawnDuration,
    pub frame_rate: u32,
    pub audio: SequenceAudio,
    pub mark_collections: Vec<MarkCollection>,
    pub effects: Vec<EffectInst>,
    pub automation_clips: Vec<AutomationClip>,
}

pub struct MarkCollection {
    pub key: MarkCollectionKey,
    pub display_color: Color,
    pub marks: Vec<DawnTime>,
}

pub struct MarkCollectionKey {
    pub name: String,
}

pub struct AutomationClip {
    pub id: AutomationClipId,
    pub targets: Vec<EffectInstId>,
    pub start: DawnTime,
    pub duration: DawnDuration,
    pub curve: CurveDefinitionKey,
}

pub struct AutomationClipId(pub u32);

pub enum SequenceAudio {
    None,
    File(String),
}
