use crate::effect::{EffectInst, EffectInstID};
use crate::effect_dsl::Curve;
use crate::model::{DawnDuration, DawnTime};

pub struct Sequence {
    pub duration: DawnDuration,
    pub frame_rate: u32,
    pub audio: SequenceAudio,
    pub mark_collections: Vec<MarkCollection>,
    pub effects: Vec<EffectInst>,
    pub automation_clips: Vec<AutomationClip>,
}

pub struct MarkCollection {
    pub key: String,
    pub display_color: String,
    pub marks: Vec<DawnTime>,
}

pub struct AutomationClip {
    pub id: AutomationClipID,
    pub targets: Vec<EffectInstID>,
    pub start_time: DawnTime,
    pub duration: DawnDuration,
    pub curve: Curve,
}

pub struct AutomationClipID(u32);


pub enum SequenceAudio {
    None,
    File(String),
}

