use std::time::Duration;

use crate::sequence::Sequence;
use crate::setup::Setup;

pub struct DawnTime(Duration);
pub struct DawnDuration(Duration);

pub struct DawnProject {
    pub setup: Setup,
    pub sequences: Vec<Sequence>,
}
