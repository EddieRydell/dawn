use alloc::{boxed::Box, vec, vec::Vec};
use core::ops::Range;

use crate::control::{ControlError, PreparedControl, apply_controls, apply_fixture_behavior_rules};
use crate::element::{ElementLayout, ElementNodeId, RenderedElementState, black};
use crate::fixture::{FixtureBehaviors, FixtureFunctionId};
use crate::patch::{PatchError, PatchWorkspace, PreparedPatch};
use crate::sequence::{EvaluationError, EvaluationWorkspace, PreparedSequence};
use crate::values::{Color, SampleTime};

/// Frozen playback data; authoring, elaboration, networking, and pin timing are external.
pub struct PreparedShow {
    pub workspace_key: u32,
    pub sequence: PreparedSequence,
    pub elements: Box<[(ElementNodeId, ElementLayout)]>,
    pub controls: Box<[PreparedControl]>,
    pub fixture_behaviors: FixtureBehaviors,
    pub patch: PreparedPatch,
    pub output_widths: Box<[u32]>,
    pub color_spans: Box<[(u32, Range<u32>)]>,
}

#[derive(Debug)]
pub struct ShowWorkspace {
    workspace_key: u32,
    sequence: EvaluationWorkspace,
    patch: PatchWorkspace,
    colors: Vec<Color>,
    elements: Vec<RenderedElementState>,
    explicit_fixture_controls: Vec<(u32, FixtureFunctionId)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ShowError {
    InvalidWorkspace,
    Sequence(EvaluationError),
    Control(ControlError),
    Patch(PatchError),
}

impl ShowWorkspace {
    pub fn elements(&self) -> &[RenderedElementState] {
        &self.elements
    }
}

impl PreparedShow {
    pub fn workspace(&self) -> ShowWorkspace {
        ShowWorkspace {
            workspace_key: self.workspace_key,
            sequence: self.sequence.workspace(),
            patch: self.patch.workspace(),
            colors: vec![black(); self.sequence.pixel_count()],
            elements: self
                .elements
                .iter()
                .map(|(node, layout)| layout.create(*node))
                .collect(),
            explicit_fixture_controls: Vec::with_capacity(
                self.controls
                    .iter()
                    .map(PreparedControl::explicit_fixture_count)
                    .sum(),
            ),
        }
    }

    /// Evaluate into caller-owned buffers in prepared output order.
    /// Create the workspace once, and size each buffer using `output_widths`.
    pub fn evaluate(
        &self,
        sample_time: SampleTime,
        buffers: &mut [impl AsMut<[u8]>],
        workspace: &mut ShowWorkspace,
    ) -> Result<(), ShowError> {
        if workspace.workspace_key != self.workspace_key {
            return Err(ShowError::InvalidWorkspace);
        }
        self.sequence
            .evaluate(sample_time, &mut workspace.colors, &mut workspace.sequence)
            .map_err(ShowError::Sequence)?;

        for element in &mut workspace.elements {
            match element {
                RenderedElementState::Color { cells, .. } => cells.fill(black()),
                RenderedElementState::Scalar { cells, .. } => cells.fill(0.0),
                RenderedElementState::Indexed { cells, .. } => cells.fill(0),
                RenderedElementState::Fixture { color, state, .. } => {
                    *color = black();
                    state.functions.clear();
                }
            }
        }
        for (element, span) in &self.color_spans {
            match &mut workspace.elements[*element as usize] {
                RenderedElementState::Color { cells, .. } => {
                    cells
                        .copy_from_slice(&workspace.colors[span.start as usize..span.end as usize]);
                }
                RenderedElementState::Fixture { color, .. } => {
                    *color = workspace.colors[span.start as usize];
                }
                _ => unreachable!("prepared color span targets a color-capable element"),
            }
        }
        apply_controls(
            &mut workspace.elements,
            &self.controls,
            sample_time,
            &mut workspace.explicit_fixture_controls,
        )
        .map_err(ShowError::Control)?;
        apply_fixture_behavior_rules(
            &mut workspace.elements,
            &self.fixture_behaviors,
            &workspace.explicit_fixture_controls,
        )
        .map_err(ShowError::Control)?;
        self.patch
            .evaluate(&workspace.elements, buffers, &mut workspace.patch)
            .map_err(ShowError::Patch)?;
        Ok(())
    }
}
