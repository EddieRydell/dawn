use dawn_language::controller::{ControllerId, ControllerPortId};
use dawn_language::element::{ColorCapability, ElementNodeId};
use dawn_language::fixture_profile::{FixtureProfileId, FixtureState};
use dawn_language::values::Color;

use super::values::{black, grayscale};

#[derive(Clone, Debug, PartialEq)]
pub enum RenderedElementState {
    Color {
        node: ElementNodeId,
        capability: ColorCapability,
        cells: Vec<Color>,
    },
    Scalar {
        node: ElementNodeId,
        cells: Vec<f32>,
    },
    Indexed {
        node: ElementNodeId,
        cells: Vec<u32>,
    },
    Fixture {
        node: ElementNodeId,
        profile: FixtureProfileId,
        color: Color,
        state: FixtureState,
    },
}

impl RenderedElementState {
    pub fn node(&self) -> ElementNodeId {
        match self {
            Self::Color { node, .. }
            | Self::Scalar { node, .. }
            | Self::Indexed { node, .. }
            | Self::Fixture { node, .. } => *node,
        }
    }

    pub fn preview_colors(&self) -> Vec<Color> {
        match self {
            Self::Color { cells, .. } => cells.clone(),
            Self::Fixture { color, .. } => vec![*color],
            Self::Scalar { cells, .. } => cells.iter().map(|level| grayscale(*level)).collect(),
            Self::Indexed { cells, .. } => vec![black(); cells.len()],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControllerPortFrame {
    pub controller: ControllerId,
    pub port: ControllerPortId,
    pub slots: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderedSequenceFrame {
    pub frame_index: u32,
    pub frame_rate: u32,
    pub sample_time: dawn_language::values::SampleTime,
    pub elements: Vec<RenderedElementState>,
    pub controller_frames: Vec<ControllerPortFrame>,
}
