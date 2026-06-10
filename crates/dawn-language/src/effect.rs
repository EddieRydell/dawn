use crate::effect_dsl::types::{EffectDslIdentifier, EffectDslValue};
use crate::model::{DawnDuration, DawnTime};
use indexmap::IndexMap;

pub struct Effect {
    pub start: DawnTime,
    pub duration: DawnDuration,
    pub target: EffectTarget,
    pub params: IndexMap<EffectDslIdentifier, EffectDslValue>,
    pub script: EffectScript,
}

pub enum EffectTarget {
    Group,
    Fixture,
}
pub enum EffectScript {
    // contains bytecode
    // and the original source text
}
