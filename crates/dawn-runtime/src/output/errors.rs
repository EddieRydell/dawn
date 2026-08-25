use dawn_language::element::ElementNodeId;
use dawn_language::values::Color;

use crate::RenderError;

#[derive(Clone, Debug, PartialEq)]
pub enum SequenceOutputPrepareError {
    ProjectValidation(String),
    Render(RenderError),
    MissingSetup,
    MissingElementTree,
    MissingSequence,
    InvalidEffectTree,
    InvalidControl {
        clip: u32,
        reason: String,
    },
    ControlConflict {
        first: u32,
        second: u32,
        node: ElementNodeId,
        cell: u32,
    },
    InvalidPatch(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SequenceOutputRenderError {
    Render(RenderError),
    Control { clip: u32, reason: String },
    UnsupportedFixtureColor { node: ElementNodeId, color: Color },
    Patch(String),
}
