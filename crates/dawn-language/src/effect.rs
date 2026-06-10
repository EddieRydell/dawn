use crate::effect_dsl::types::{Identifier, Value};
use crate::model::{DawnDuration, DawnTime};
use indexmap::IndexMap;

pub struct Effect {
    pub start: DawnTime,
    pub duration: DawnDuration,
    pub target: EffectTarget,
    pub script: EffectScript,
    pub param_overrides: IndexMap<Identifier, Value>,
}

pub enum EffectTarget {
    Group,
    Fixture,
}
pub enum EffectScript {
    // contains bytecode
    // and the original source text
}
