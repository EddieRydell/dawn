use std::time::Duration;

use crate::sequence::Sequence;
use crate::setup::Setup;

pub struct DawnTime(pub Duration);
pub struct DawnDuration(pub Duration);

pub struct DawnProject {
    pub setup: Setup,
    pub sequences: Vec<Sequence>,
}
