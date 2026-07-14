use crate::controller::{Controller, ControllerId};
use crate::effect::{CurveDefinitionStore, EffectDefinitionStore, GradientDefinitionStore};
use crate::element::{ElementTree, ElementTreeId};
use crate::fixture_profile::FixtureProfileStore;
use crate::identity::SourceIdentity;
use crate::operator::OperatorDefinitionStore;
use crate::patch::{PatchGraph, PatchId};
use crate::preview::{PreviewLayout, PreviewLayoutId, PropDefinitionStore};
use crate::sequence::Sequence;
use crate::setup::{Setup, SetupId};
use indexmap::IndexMap;

#[derive(Clone, Debug, PartialEq)]
pub struct DawnProject {
    pub root: ProjectRoot,
    pub setups: IndexMap<SetupId, Setup>,
    pub element_trees: IndexMap<ElementTreeId, ElementTree>,
    pub preview_layouts: IndexMap<PreviewLayoutId, PreviewLayout>,
    pub patches: IndexMap<PatchId, PatchGraph>,
    pub controllers: IndexMap<ControllerId, Controller>,
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
    pub props: PropDefinitionStore,
    pub fixture_profiles: FixtureProfileStore,
    pub curves: CurveDefinitionStore,
    pub gradients: GradientDefinitionStore,
    pub operators: OperatorDefinitionStore,
}
