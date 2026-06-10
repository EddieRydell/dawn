use crate::effect_dsl::types::{Identifier, Value};
use crate::effect_dsl::CompiledEffect;
use crate::values::{Curve, DawnDuration, DawnTime};
use camino::Utf8PathBuf;
use indexmap::IndexMap;

pub struct EffectInst {
    pub id: EffectInstID,
    pub start: DawnTime,
    pub duration: DawnDuration,
    pub target: EffectTarget,
    pub definition: EffectDefinitionKey,
    pub param_overrides: IndexMap<Identifier, Value>,
}

pub struct EffectInstID(pub u32);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EffectDefinitionKey {
    pub source_path: Utf8PathBuf,
    pub effect_name: Identifier,
}

pub enum EffectTarget {
    Group,
    Fixture,
}

pub struct EffectDefinition {
    pub key: EffectDefinitionKey,
    pub compiled: CompiledEffect,
}

#[derive(Default)]
pub struct EffectDefinitionStore {
    pub definitions: IndexMap<EffectDefinitionKey, EffectDefinition>,
}

impl EffectDefinitionStore {
    pub fn get(&self, key: &EffectDefinitionKey) -> Option<&EffectDefinition> {
        self.definitions.get(key)
    }

    pub fn insert(&mut self, definition: EffectDefinition) -> Option<EffectDefinition> {
        self.definitions.insert(definition.key.clone(), definition)
    }
}

pub struct CurveInst {
    pub id: CurveInstID,
    pub definition: CurveDefinitionKey,
}

pub struct CurveInstID(pub u32);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct CurveDefinitionKey {
    pub source_path: Utf8PathBuf,
    pub curve_name: String,
}

pub struct CurveDefinition {
    pub key: CurveDefinitionKey,
    pub curve: Curve,
}

#[derive(Default)]
pub struct CurveDefinitionStore {
    pub definitions: IndexMap<CurveDefinitionKey, CurveDefinition>,
}

impl CurveDefinitionStore {
    pub fn get(&self, key: &CurveDefinitionKey) -> Option<&CurveDefinition> {
        self.definitions.get(key)
    }

    pub fn insert(&mut self, curve: CurveDefinition) -> Option<CurveDefinition> {
        self.definitions.insert(curve.key.clone(), curve)
    }
}
