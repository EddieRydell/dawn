use alloc::{boxed::Box, vec::Vec};
use core::ops::Range;

use crate::control::{ControlError, PreparedControl, apply_controls, apply_fixture_behavior_rules};
use crate::element::{ElementLayout, ElementNodeId, RenderedElementState, black};
use crate::fixture::FixtureBehaviors;
use crate::patch::{PatchError, PatchWorkspace, PreparedPatch};
use crate::signal::{EvaluationError, EvaluationWorkspace, PreparedSignalGraph};
use crate::values::SampleTime;

/// Frozen playback data; authoring, elaboration, networking, and pin timing are external.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedSequence {
    pub workspace_key: u32,
    pub signals: PreparedSignalGraph,
    pub elements: Box<[(ElementNodeId, ElementLayout)]>,
    pub controls: Box<[PreparedControl]>,
    pub fixture_behaviors: FixtureBehaviors,
    pub patch: PreparedPatch,
    pub output_widths: Box<[u32]>,
    pub color_spans: Box<[(u32, Range<u32>)]>,
}

#[derive(Debug)]
pub struct SequenceWorkspace {
    workspace_key: u32,
    signals: EvaluationWorkspace,
    patch: PatchWorkspace,
    elements: Vec<RenderedElementState>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SequenceError {
    InvalidWorkspace,
    Signal(EvaluationError),
    Control(ControlError),
    Patch(PatchError),
}

impl SequenceWorkspace {
    pub fn elements(&self) -> &[RenderedElementState] {
        &self.elements
    }
}

impl PreparedSequence {
    pub fn workspace(&self) -> SequenceWorkspace {
        SequenceWorkspace {
            workspace_key: self.workspace_key,
            signals: self.signals.workspace(),
            patch: self.patch.workspace(),
            elements: self
                .elements
                .iter()
                .map(|(node, layout)| layout.create(*node))
                .collect(),
        }
    }

    /// Evaluate into caller-owned buffers in prepared output order.
    /// Create the workspace once, and size each buffer using `output_widths`.
    pub fn evaluate(
        &self,
        sample_time: SampleTime,
        buffers: &mut [impl AsMut<[u8]>],
        workspace: &mut SequenceWorkspace,
    ) -> Result<(), SequenceError> {
        if workspace.workspace_key != self.workspace_key {
            return Err(SequenceError::InvalidWorkspace);
        }
        let colors = self
            .signals
            .evaluate(sample_time, &mut workspace.signals)
            .map_err(SequenceError::Signal)?;

        for element in &mut workspace.elements {
            match element {
                // Color spans overwrite whole elements; unmapped colors stay at
                // their initial black value. Controls never write color elements.
                RenderedElementState::Color { .. } => {}
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
                    cells.copy_from_slice(&colors[span.start as usize..span.end as usize]);
                }
                RenderedElementState::Fixture { color, .. } => {
                    *color = colors[span.start as usize];
                }
                _ => unreachable!("prepared color span targets a color-capable element"),
            }
        }
        apply_controls(&mut workspace.elements, &self.controls, sample_time)
            .map_err(SequenceError::Control)?;
        apply_fixture_behavior_rules(&mut workspace.elements, &self.fixture_behaviors)
            .map_err(SequenceError::Control)?;
        self.patch
            .evaluate(&workspace.elements, buffers, &mut workspace.patch)
            .map_err(SequenceError::Patch)?;
        Ok(())
    }
}
