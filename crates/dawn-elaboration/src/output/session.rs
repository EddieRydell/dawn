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
use super::patch::{PatchWorkspace, PreparedPatch};
use super::values::black;
use crate::sequence::timeline::{frame_at_or_before, sample_time_for_frame};
use crate::{EvaluationWorkspace, PreparedSequence, RenderError, elaborate_sequence};

static NEXT_OUTPUT_ID: AtomicU32 = AtomicU32::new(1);

pub struct PreparedSequenceOutput {
    id: u32,
    sequence: PreparedSequence,
    element_templates: Box<[RenderedElementState]>,
    profiles: FixtureProfileStore,
    controls: Box<[PreparedControl]>,
    patch: PreparedPatch,
    controller_ports: Box<[ControllerPortFrame]>,
    color_spans: Box<[(usize, Range<usize>)]>,
}

#[derive(Debug)]
pub struct OutputEvaluationWorkspace {
    sequence: EvaluationWorkspace,
    patch: PatchWorkspace,
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
        let sequence = elaborate_sequence(project, setup_id, sequence_id)
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
        for element in &sequence.elements {
            let end = offset + element.pixel_count;
            let element_id = dawn_language::element::ElementNodeId(element.id);
            let node = tree.nodes.get(&element_id).ok_or_else(|| {
                SequenceOutputPrepareError::InvalidPatch(
                    "prepared render element is missing from its tree".to_string(),
                )
            })?;
            if matches!(
                node.kind,
                ElementNodeKind::Color { .. } | ElementNodeKind::Fixture { .. }
            ) {
                color_spans.push((element_indexes[&element_id], offset..end));
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
        self.render_seconds_with_workspace(seconds, &mut self.workspace())
    }

    pub fn render_seconds_with_workspace(
        &self,
        seconds: f32,
        workspace: &mut OutputEvaluationWorkspace,
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
        self.render_at(frame, sample_time, workspace)
    }

    pub fn render_frame(
        &self,
        frame: u32,
    ) -> Result<RenderedSequenceFrame, SequenceOutputRenderError> {
        let mut workspace = self.workspace();
        let frame = frame.min(self.frame_count().saturating_sub(1));
        let sample_time = sample_time_for_frame(frame, self.frame_rate())
            .map_err(SequenceOutputRenderError::Render)?;
        self.render_at(frame, sample_time, &mut workspace)
    }

    pub fn frame_rate(&self) -> u32 {
        self.sequence.frame_rate()
    }
    pub fn frame_count(&self) -> u32 {
        self.sequence.frame_count()
    }

    pub fn workspace(&self) -> OutputEvaluationWorkspace {
        let mut elements = self.element_templates.to_vec();
        for element in &mut elements {
            let RenderedElementState::Fixture { profile, state, .. } = element else {
                continue;
            };
            let function_count = self
                .profiles
                .definitions
                .get(profile)
                .map_or(0, |profile| profile.functions.len());
            state.functions.reserve(function_count);
        }
        OutputEvaluationWorkspace {
            sequence: self.sequence.workspace(),
            patch: self.patch.workspace(&self.profiles),
            output_id: Some(self.id),
            colors: vec![black(); self.sequence.pixel_count()],
            elements,
            explicit_fixture_controls: Vec::with_capacity(
                self.controls
                    .iter()
                    .map(PreparedControl::explicit_fixture_count)
                    .sum(),
            ),
            controller_frames: self.controller_ports.to_vec(),
        }
    }

    /// Samples controller port bytes into buffers owned by `workspace`.
    /// Repeated calls for the same prepared output reuse all output-stage
    /// element, patch, and controller storage.
    pub fn sample_into<'a>(
        &self,
        sample_time: SampleTime,
        workspace: &'a mut OutputEvaluationWorkspace,
    ) -> Result<&'a [ControllerPortFrame], SequenceOutputRenderError> {
        if workspace.output_id != Some(self.id) {
            return Err(SequenceOutputRenderError::Patch(
                "evaluation workspace belongs to another prepared output".to_string(),
            ));
        }
        self.sequence
            .evaluate(sample_time, &mut workspace.colors, &mut workspace.sequence)
            .map_err(|error| SequenceOutputRenderError::Render(error.into()))?;

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
            match &mut workspace.elements[*element] {
                RenderedElementState::Color { cells, .. } => {
                    cells.copy_from_slice(&workspace.colors[span.clone()]);
                }
                RenderedElementState::Fixture { color, .. } => {
                    *color = workspace.colors[span.start];
                }
                _ => unreachable!("prepared color span targets a color-capable element"),
            }
        }
        apply_controls(
            &mut workspace.elements,
            &self.controls,
            sample_time,
            &mut workspace.explicit_fixture_controls,
        )?;
        apply_fixture_behavior_rules(
            &mut workspace.elements,
            &self.profiles,
            &workspace.explicit_fixture_controls,
        )?;
        self.patch.evaluate(
            &self.profiles,
            &workspace.elements,
            &mut workspace.controller_frames,
            &mut workspace.patch,
        )?;
        Ok(&workspace.controller_frames)
    }

    fn render_at(
        &self,
        frame_index: u32,
        sample_time: SampleTime,
        workspace: &mut OutputEvaluationWorkspace,
    ) -> Result<RenderedSequenceFrame, SequenceOutputRenderError> {
        self.sample_into(sample_time, workspace)?;
        Ok(RenderedSequenceFrame {
            frame_index,
            frame_rate: self.frame_rate(),
            sample_time,
            elements: workspace.elements.clone(),
            controller_frames: workspace.controller_frames.clone(),
        })
    }
}
