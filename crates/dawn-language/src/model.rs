use std::time::Duration;

use crate::effect::{CurveDefinitionStore, EffectDefinitionStore};
use crate::sequence::Sequence;
use crate::setup::{ControllerDefinitionStore, FixtureDefinitionStore, Setup};

pub struct DawnTime(pub Duration);
pub struct DawnDuration(pub Duration);

pub struct DawnProject {
    pub setup: Setup,
    pub sequences: Vec<Sequence>,
    pub definitions: ProjectDefinitionStores,
}

pub struct ProjectDefinitionStores {
    pub effects: EffectDefinitionStore,
    pub fixtures: FixtureDefinitionStore,
    pub curves: CurveDefinitionStore,
    pub controllers: ControllerDefinitionStore,
}
