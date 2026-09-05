use alloc::vec;
use alloc::vec::Vec;

use crate::fixture::{FixtureState, quantize8};
use crate::values::Color;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ElementNodeId(pub u32);

/// Mutable-state capacity required for an element, resolved before playback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ElementLayout {
    Color(u32),
    Scalar(u32),
    Indexed(u32),
    Fixture(u32),
}

impl ElementLayout {
    pub fn create(self, node: ElementNodeId) -> RenderedElementState {
        match self {
            Self::Color(cells) => RenderedElementState::Color {
                node,
                cells: vec![black(); cells as usize],
            },
            Self::Scalar(cells) => RenderedElementState::Scalar {
                node,
                cells: vec![0.0; cells as usize],
            },
            Self::Indexed(cells) => RenderedElementState::Indexed {
                node,
                cells: vec![0; cells as usize],
            },
            Self::Fixture(functions) => RenderedElementState::Fixture {
                node,
                color: black(),
                state: FixtureState {
                    functions: Vec::with_capacity(functions as usize),
                },
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RenderedElementState {
    Color {
        node: ElementNodeId,
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

pub fn black() -> Color {
    Color {
        red: 0,
        green: 0,
        blue: 0,
    }
}
fn grayscale(value: f32) -> Color {
    let channel = quantize8(value);
    Color {
        red: channel,
        green: channel,
        blue: channel,
    }
}
