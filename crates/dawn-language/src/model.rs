use crate::effect::{CurveDefinitionStore, EffectDefinitionStore, GradientDefinitionStore};
use crate::identity::SourceIdentity;
use crate::operator::OperatorDefinitionStore;
use crate::sequence::Sequence;
use crate::setup::{
    ControllerDefinitionStore, ControllerId, FixtureDefinitionStore, Layout, LayoutId, Patch,
    PatchId, Setup, SetupId,
};
use indexmap::IndexMap;

#[derive(Clone, Debug, PartialEq)]
pub struct DawnProject {
    pub root: ProjectRoot,
    pub setups: IndexMap<SetupId, Setup>,
    pub layouts: IndexMap<LayoutId, Layout>,
    pub patches: IndexMap<PatchId, Patch>,
    pub controllers: IndexMap<ControllerId, crate::setup::ControllerDefinition>,
    pub sequences: IndexMap<crate::sequence::SequenceId, Sequence>,
    pub definitions: ProjectDefinitionStores,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ProjectId(pub SourceIdentity);

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRoot {
    pub id: ProjectId,
    pub setup: SetupId,
    pub sequences: Vec<crate::sequence::SequenceId>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectDefinitionStores {
    pub effects: EffectDefinitionStore,
    pub fixtures: FixtureDefinitionStore,
    pub curves: CurveDefinitionStore,
    pub gradients: GradientDefinitionStore,
    pub controllers: ControllerDefinitionStore,
    pub operators: OperatorDefinitionStore,
}
