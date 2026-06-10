use crate::effect::{CurveDefinitionStore, EffectDefinitionStore};
use crate::sequence::Sequence;
use crate::setup::{
    ControllerDefinitionStore, ControllerId, Display, DisplayId, FixtureDefinitionStore, Layout,
    LayoutId, Patch, PatchId,
};
use indexmap::IndexMap;

#[derive(Clone, Debug, PartialEq)]
pub struct DawnProject {
    pub root: ProjectRoot,
    pub displays: IndexMap<DisplayId, Display>,
    pub layouts: IndexMap<LayoutId, Layout>,
    pub patches: IndexMap<PatchId, Patch>,
    pub controllers: IndexMap<ControllerId, crate::setup::ControllerDefinition>,
    pub sequences: IndexMap<crate::sequence::SequenceId, Sequence>,
    pub definitions: ProjectDefinitionStores,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ProjectId(pub String);

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRoot {
    pub id: ProjectId,
    pub display: DisplayId,
    pub sequences: Vec<crate::sequence::SequenceId>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectDefinitionStores {
    pub effects: EffectDefinitionStore,
    pub fixtures: FixtureDefinitionStore,
    pub curves: CurveDefinitionStore,
    pub controllers: ControllerDefinitionStore,
}
