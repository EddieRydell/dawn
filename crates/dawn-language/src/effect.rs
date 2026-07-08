use crate::effect_dsl::CompiledEffect;
use crate::effect_dsl::types::Identifier;
use crate::sequence::{MarkCollectionKey, SequenceLayerId};
use crate::setup::{FixtureGroupId, FixtureInstanceId};
use crate::values::{Curve, DawnDuration, DawnTime};
use indexmap::IndexMap;

#[derive(Clone, Debug, PartialEq)]
pub struct EffectInst {
    pub id: EffectInstId,
    pub layer_id: SequenceLayerId,
    pub start: DawnTime,
    pub duration: DawnDuration,
    pub target: EffectTarget,
    pub scope: EffectScope,
    pub definition: EffectDefinitionId,
    pub param_overrides: IndexMap<Identifier, EffectParamValue>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EffectInstId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EffectDefinitionId(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EffectTarget {
    Group(FixtureGroupId),
    Fixture(FixtureInstanceId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EffectScope {
    PerFixture,
    WholeTarget,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EffectParamValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Color(crate::values::Color),
    Enum(Identifier),
    Marks(MarkCollectionKey),
    Curve(CurveSource),
    Array(Vec<EffectParamValue>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum CurveSource {
    Inline(Curve),
    Reference(CurveId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectDefinition {
    pub compiled: CompiledEffect,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EffectDefinitionStore {
    pub definitions: IndexMap<EffectDefinitionId, EffectDefinition>,
}

impl EffectDefinitionStore {
    pub fn get(&self, key: &EffectDefinitionId) -> Option<&EffectDefinition> {
        self.definitions.get(key)
    }

    pub fn insert(
        &mut self,
        key: EffectDefinitionId,
        definition: EffectDefinition,
    ) -> Option<EffectDefinition> {
        self.definitions.insert(key, definition)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct CurveId(pub String);

#[derive(Clone, Debug, PartialEq)]
pub struct CurveDefinition {
    pub curve: Curve,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CurveDefinitionStore {
    pub definitions: IndexMap<CurveId, CurveDefinition>,
}

impl CurveDefinitionStore {
    pub fn get(&self, key: &CurveId) -> Option<&CurveDefinition> {
        self.definitions.get(key)
    }

    pub fn insert(&mut self, key: CurveId, curve: CurveDefinition) -> Option<CurveDefinition> {
        self.definitions.insert(key, curve)
    }
}
