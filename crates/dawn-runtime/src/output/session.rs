use std::collections::HashMap;
use std::ops::Range;
use std::sync::atomic::{AtomicU32, Ordering};

use dawn_language::element::ElementNodeKind;
use dawn_language::fixture_profile::{FixtureProfileStore, FixtureState};
use dawn_language::model::DawnProject;
use dawn_language::sequence::SequenceId;
use dawn_language::setup::SetupId;
use dawn_language::validation::validate_project;
use dawn_language::values::{Color, SampleTime, sample_time_from_seconds_f32};

use super::controls::{
    PreparedControl, apply_controls, apply_fixture_behavior_rules, prepare_controls,
};
use super::errors::{SequenceOutputPrepareError, SequenceOutputRenderError};
use super::frame::{ControllerPortFrame, RenderedElementState, RenderedSequenceFrame};
use super::patch::{PatchScratch, PreparedPatch};
use super::values::black;
use crate::sequence::timeline::{frame_at_or_before, sample_time_for_frame};
use crate::{PreparedSequenceRenderer, RenderError, SequenceRenderScratch};

static NEXT_OUTPUT_ID: AtomicU32 = AtomicU32::new(1);

pub struct PreparedSequenceOutput {
    id: u32,
    sequence: PreparedSequenceRenderer,
    element_templates: Box<[RenderedElementState]>,
    profiles: FixtureProfileStore,
    controls: Box<[PreparedControl]>,
    patch: PreparedPatch,
    controller_ports: Box<[ControllerPortFrame]>,
    color_spans: Box<[(usize, Range<usize>)]>,
}

#[derive(Debug, Default)]
pub struct SequenceOutputScratch {
    sequence: SequenceRenderScratch,
    patch: PatchScratch,
    output_id: Option<u32>,
    colors: Vec<Color>,
    elements: Vec<RenderedElementState>,
    explicit_fixture_controls: Vec<(u32, dawn_language::fixture_profile::FixtureFunctionId)>,
    controller_frames: Vec<ControllerPortFrame>,
}

impl PreparedSequenceOutput {
    pub fn prepare(
        project: &DawnProject,
        setup_id: &SetupId,
        sequence_id: &SequenceId,
    ) -> Result<Self, SequenceOutputPrepareError> {
        validate_project(project)
            .map_err(|error| SequenceOutputPrepareError::ProjectValidation(format!("{error:?}")))?;
        let setup = project
            .setups
            .get(setup_id)
            .ok_or(SequenceOutputPrepareError::MissingSetup)?;
        let tree = project
            .element_trees
            .get(&setup.elements)
            .ok_or(SequenceOutputPrepareError::MissingElementTree)?
            .clone();
        let sequence_definition = project
            .sequences
            .get(sequence_id)
            .ok_or(SequenceOutputPrepareError::MissingSequence)?;
        if sequence_definition
            .effects
            .iter()
            .any(|effect| effect.target.tree != tree.id)
        {
            return Err(SequenceOutputPrepareError::InvalidEffectTree);
        }
        let mut profiles = FixtureProfileStore::default();
        for node in tree.nodes.values() {
            let ElementNodeKind::Fixture { profile } = &node.kind else {
                continue;
            };
            if let Some(definition) = project
                .definitions
                .fixture_profiles
                .definitions
                .get(profile)
            {
                profiles
                    .definitions
                    .insert(profile.clone(), definition.clone());
            }
        }
        let controls = prepare_controls(&tree, &profiles, &sequence_definition.control_clips)?;
        let sequence = PreparedSequenceRenderer::prepare(project, setup_id, sequence_id)
            .map_err(SequenceOutputPrepareError::Render)?;
        let patch = project.patches.get(&setup.patch).ok_or_else(|| {
            SequenceOutputPrepareError::InvalidPatch("setup patch is missing".to_string())
        })?;
        let mut controller_ports = Vec::new();
        for controller_id in &setup.controllers {
            let controller = project.controllers.get(controller_id).ok_or_else(|| {
                SequenceOutputPrepareError::InvalidPatch("setup controller is missing".to_string())
            })?;
            for port in &controller.ports {
                controller_ports.push(ControllerPortFrame {
                    controller: controller_id.clone(),
                    port: port.id,
                    slots: vec![0; usize::from(port.slot_count)],
                });
            }
        }
        let patch = PreparedPatch::prepare(&tree, patch, &controller_ports)?;
        let mut element_templates = Vec::new();
        for (node_id, node) in &tree.nodes {
            let element = match &node.kind {
                ElementNodeKind::Group { .. } => continue,
                ElementNodeKind::Color { cells, capability } => RenderedElementState::Color {
                    node: *node_id,
                    capability: capability.clone(),
                    cells: vec![black(); *cells as usize],
                },
                ElementNodeKind::Scalar { cells } => RenderedElementState::Scalar {
                    node: *node_id,
                    cells: vec![0.0; *cells as usize],
                },
                ElementNodeKind::Indexed { cells, .. } => RenderedElementState::Indexed {
                    node: *node_id,
                    cells: vec![0; *cells as usize],
                },
                ElementNodeKind::Fixture { profile } => {
                    let capacity = profiles
                        .definitions
                        .get(profile)
                        .map_or(0, |profile| profile.functions.len());
                    RenderedElementState::Fixture {
                        node: *node_id,
                        profile: profile.clone(),
                        color: black(),
                        state: FixtureState {
                            functions: Vec::with_capacity(capacity),
                        },
                    }
                }
            };
            element_templates.push(element);
        }
        let element_indexes = tree
            .nodes
            .iter()
            .filter(|(_, node)| !matches!(node.kind, ElementNodeKind::Group { .. }))
            .enumerate()
            .map(|(index, (id, _))| (*id, index))
            .collect::<HashMap<_, _>>();
        let mut offset = 0;
        let mut color_spans = Vec::new();
        for element in sequence.prepared_elements() {
            let end = offset + element.pixel_count;
            let node = tree.nodes.get(&element.id).ok_or_else(|| {
                SequenceOutputPrepareError::InvalidPatch(
                    "prepared render element is missing from its tree".to_string(),
                )
            })?;
            if matches!(
                node.kind,
                ElementNodeKind::Color { .. } | ElementNodeKind::Fixture { .. }
            ) {
                color_spans.push((element_indexes[&element.id], offset..end));
            }
            offset = end;
        }
        Ok(Self {
            id: NEXT_OUTPUT_ID.fetch_add(1, Ordering::Relaxed),
            sequence,
            element_templates: element_templates.into_boxed_slice(),
            profiles,
            controls: controls.into_boxed_slice(),
            patch,
            controller_ports: controller_ports.into_boxed_slice(),
            color_spans: color_spans.into_boxed_slice(),
        })
    }

    pub fn render_seconds(
        &self,
        seconds: f32,
    ) -> Result<RenderedSequenceFrame, SequenceOutputRenderError> {
        self.render_seconds_with_scratch(seconds, &mut SequenceOutputScratch::default())
    }

    pub fn render_seconds_with_scratch(
        &self,
        seconds: f32,
        scratch: &mut SequenceOutputScratch,
    ) -> Result<RenderedSequenceFrame, SequenceOutputRenderError> {
        if !seconds.is_finite() {
            return Err(SequenceOutputRenderError::Render(
                RenderError::InvalidTiming {
                    reason: "audio seconds must be finite".to_string(),
                },
            ));
        }
        let sample_time = sample_time_from_seconds_f32(seconds).map_err(|_| {
            SequenceOutputRenderError::Render(RenderError::InvalidTiming {
                reason: "audio seconds exceed the runtime clock range".to_string(),
            })
        })?;
        let frame = frame_at_or_before(sample_time, self.frame_rate())
            .min(self.frame_count().saturating_sub(1));
        self.render_at(frame, sample_time, scratch)
    }

    pub fn render_frame(
        &self,
        frame: u32,
    ) -> Result<RenderedSequenceFrame, SequenceOutputRenderError> {
        let mut scratch = SequenceOutputScratch::default();
        let frame = frame.min(self.frame_count().saturating_sub(1));
        let sample_time = sample_time_for_frame(frame, self.frame_rate())
            .map_err(SequenceOutputRenderError::Render)?;
        self.render_at(frame, sample_time, &mut scratch)
    }

    pub fn frame_rate(&self) -> u32 {
        self.sequence.frame_rate()
    }
    pub fn frame_count(&self) -> u32 {
        self.sequence.frame_count()
    }

    /// Samples controller port bytes into buffers owned by `scratch`.
    /// Repeated calls for the same prepared output reuse all output-stage
    /// element, patch, and controller storage.
    pub fn sample_into<'a>(
        &self,
        sample_time: SampleTime,
        scratch: &'a mut SequenceOutputScratch,
    ) -> Result<&'a [ControllerPortFrame], SequenceOutputRenderError> {
        self.prepare_scratch(scratch);
        self.sequence
            .sample_into(sample_time, &mut scratch.colors, &mut scratch.sequence)
            .map_err(SequenceOutputRenderError::Render)?;

        for element in &mut scratch.elements {
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
            match &mut scratch.elements[*element] {
                RenderedElementState::Color { cells, .. } => {
                    cells.copy_from_slice(&scratch.colors[span.clone()]);
                }
                RenderedElementState::Fixture { color, .. } => {
                    *color = scratch.colors[span.start];
                }
                _ => unreachable!("prepared color span targets a color-capable element"),
            }
        }
        apply_controls(
            &mut scratch.elements,
            &self.controls,
            sample_time,
            &mut scratch.explicit_fixture_controls,
        )?;
        apply_fixture_behavior_rules(
            &mut scratch.elements,
            &self.profiles,
            &scratch.explicit_fixture_controls,
        )?;
        self.patch.evaluate(
            &self.profiles,
            &scratch.elements,
            &mut scratch.controller_frames,
            &mut scratch.patch,
        )?;
        Ok(&scratch.controller_frames)
    }

    fn prepare_scratch(&self, scratch: &mut SequenceOutputScratch) {
        if scratch.output_id == Some(self.id) {
            return;
        }
        scratch.colors.resize(self.sequence.pixel_count(), black());
        scratch.elements.clear();
        scratch.elements.extend_from_slice(&self.element_templates);
        scratch.controller_frames.clear();
        scratch
            .controller_frames
            .extend_from_slice(&self.controller_ports);
        scratch.patch = PatchScratch::default();
        scratch.explicit_fixture_controls.clear();
        scratch.output_id = Some(self.id);
    }

    fn render_at(
        &self,
        frame_index: u32,
        sample_time: SampleTime,
        scratch: &mut SequenceOutputScratch,
    ) -> Result<RenderedSequenceFrame, SequenceOutputRenderError> {
        self.sample_into(sample_time, scratch)?;
        Ok(RenderedSequenceFrame {
            frame_index,
            frame_rate: self.frame_rate(),
            sample_time,
            elements: scratch.elements.clone(),
            controller_frames: scratch.controller_frames.clone(),
        })
    }
}
