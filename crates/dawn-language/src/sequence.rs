use crate::effect::Effect;
use crate::model::{DawnDuration, DawnTime};

pub struct Sequence {
    pub duration: DawnDuration,
    pub frame_rate: u32,
    pub audio: SequenceAudio,
    pub mark_collections: Vec<SequenceMarkCollection>,
    pub effects: Vec<Effect>,
    pub automation_clips: Vec<AutomationClip>,
}

pub struct SequenceMarkCollection {
    pub key: String,
    pub display_color: String,
    pub marks: Vec<DawnTime>,
}

pub struct AutomationClip {
    pub id: AutomationClipID,
}

pub enum SequenceAudio {
    None,
    File(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AutomationClipID(String);
