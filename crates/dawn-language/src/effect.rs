use crate::dsl::types::Identifier;
use crate::dsl::{CompiledEffect, Type, Value};
use crate::identity::SourceIdentity;
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
pub struct EffectDefinitionId(pub SourceIdentity);

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

impl EffectParamValue {
    pub fn default_for_type(ty: &Type) -> Option<Self> {
        Self::from_default_value(ty.default_value())
    }

    fn from_default_value(value: Value) -> Option<Self> {
        match value {
            Value::Int(value) => Some(Self::Int(value)),
            Value::Float(value) => Some(Self::Float(value)),
            Value::Bool(value) => Some(Self::Bool(value)),
            Value::Color(value) => Some(Self::Color(value)),
            Value::Curve(value) => Some(Self::Curve(CurveSource::Inline((*value).clone()))),
            Value::Array(values) => values
                .iter()
                .cloned()
                .map(Self::from_default_value)
                .collect::<Option<Vec<_>>>()
                .map(Self::Array),
            Value::Enum(value) => Some(Self::Enum(value)),
            Value::Void
            | Value::Marks(_)
            | Value::Target(_)
            | Value::TargetItems(_)
            | Value::TargetItem(_) => None,
        }
    }
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
pub struct CurveId(pub SourceIdentity);

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
