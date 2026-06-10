use crate::effect_dsl::types::{Identifier, Value};
use crate::effect_dsl::CompiledEffect;
use crate::setup::{FixtureGroupKey, FixtureInstKey};
use crate::values::{Curve, DawnDuration, DawnTime};
use camino::Utf8PathBuf;
use indexmap::IndexMap;

pub struct EffectInst {
    pub id: EffectInstId,
    pub start: DawnTime,
    pub duration: DawnDuration,
    pub target: EffectTarget,
    pub definition: EffectDefinitionKey,
    pub param_overrides: IndexMap<Identifier, Value>,
}

pub struct EffectInstId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EffectDefinitionKey {
    pub source_path: Utf8PathBuf,
    pub name: Identifier,
}

pub enum EffectTarget {
    Group(FixtureGroupKey),
    Fixture(FixtureInstKey),
}

pub struct EffectDefinition {
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

    pub fn insert(
        &mut self,
        key: EffectDefinitionKey,
        definition: EffectDefinition,
    ) -> Option<EffectDefinition> {
        self.definitions.insert(key, definition)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct CurveDefinitionKey {
    pub source_path: Utf8PathBuf,
    pub name: String,
}

pub struct CurveDefinition {
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

    pub fn insert(
        &mut self,
        key: CurveDefinitionKey,
        curve: CurveDefinition,
    ) -> Option<CurveDefinition> {
        self.definitions.insert(key, curve)
    }
}
