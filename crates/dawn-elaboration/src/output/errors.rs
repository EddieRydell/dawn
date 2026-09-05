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

impl From<dawn_runtime::control::ControlError> for SequenceOutputRenderError {
    fn from(error: dawn_runtime::control::ControlError) -> Self {
        use dawn_runtime::control::{ControlError, ControlErrorKind};
        match error {
            ControlError::Control { clip, reason } => Self::Control {
                clip,
                reason: match reason {
                    ControlErrorKind::MissingTarget => "rendered target is missing",
                    ControlErrorKind::CellOutOfRange => "control cell is out of range",
                    ControlErrorKind::TypeMismatch => "control value does not match its target",
                    ControlErrorKind::FixtureTypeMismatch => {
                        "control value does not match the fixture function"
                    }
                }
                .to_string(),
            },
            ControlError::UnsupportedFixtureColor { node, color } => {
                Self::UnsupportedFixtureColor { node, color }
            }
        }
    }
}

impl From<dawn_runtime::sequence::SequenceError> for SequenceOutputRenderError {
    fn from(error: dawn_runtime::sequence::SequenceError) -> Self {
        use dawn_runtime::sequence::SequenceError;
        match error {
            SequenceError::InvalidWorkspace => {
                Self::Patch("evaluation workspace belongs to another prepared output".to_string())
            }
            SequenceError::Signal(error) => Self::Render(error.into()),
            SequenceError::Control(error) => error.into(),
            SequenceError::Patch(error) => Self::Patch(format!("{error:?}")),
        }
    }
}
