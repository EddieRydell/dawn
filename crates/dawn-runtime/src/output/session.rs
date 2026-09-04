use std::collections::HashMap;

use dawn_language::element::{ElementNodeKind, ElementTree};
use dawn_language::fixture_profile::{FixtureProfileStore, FixtureState};
use dawn_language::model::DawnProject;
use dawn_language::patch::{PatchGraph, PatchNode};
use dawn_language::sequence::SequenceId;
use dawn_language::setup::SetupId;
use dawn_language::validation::validate_project;
use indexmap::IndexMap;

use super::controls::{
    PreparedControl, apply_controls, apply_fixture_behavior_rules, prepare_controls,
};
use super::errors::{SequenceOutputPrepareError, SequenceOutputRenderError};
use super::frame::{ControllerPortFrame, RenderedElementState, RenderedSequenceFrame};
use super::patch::{evaluate_patch, validate_patch_sources};
use super::values::black;
use crate::{PreparedSequenceRenderer, RenderedFrame, SequenceRenderScratch};

pub struct PreparedSequenceOutput {
    sequence: PreparedSequenceRenderer,
    tree: ElementTree,
    profiles: FixtureProfileStore,
    controls: Vec<PreparedControl>,
    patch: PatchGraph,
    controller_ports: Vec<ControllerPortFrame>,
}

#[derive(Debug, Default)]
pub struct SequenceOutputScratch {
    sequence: SequenceRenderScratch,
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
        let controls = prepare_controls(
            &tree,
            &project.definitions.fixture_profiles,
            &sequence_definition.control_clips,
        )?;
        let sequence = PreparedSequenceRenderer::prepare(project, setup_id, sequence_id)
            .map_err(SequenceOutputPrepareError::Render)?;
        let patch = project
            .patches
            .get(&setup.patch)
            .ok_or_else(|| {
                SequenceOutputPrepareError::InvalidPatch("setup patch is missing".to_string())
            })?
            .clone();
        patch
            .validate()
            .map_err(|error| SequenceOutputPrepareError::InvalidPatch(format!("{error:?}")))?;
        validate_patch_sources(&tree, &patch)?;
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
        for node in patch.nodes.values() {
            if let PatchNode::Sink(sink) = node {
                let frame = controller_ports
                    .iter()
                    .find(|frame| frame.controller == sink.controller && frame.port == sink.port)
                    .ok_or_else(|| {
                        SequenceOutputPrepareError::InvalidPatch(
                            "patch sink references a controller port outside the active setup"
                                .to_string(),
                        )
                    })?;
                let end = usize::from(sink.start_slot)
                    .checked_add(usize::from(sink.slot_count))
                    .ok_or_else(|| {
                        SequenceOutputPrepareError::InvalidPatch(
                            "patch sink slot range overflowed".to_string(),
                        )
                    })?;
                if end > frame.slots.len() {
                    return Err(SequenceOutputPrepareError::InvalidPatch(
                        "patch sink exceeds its controller port".to_string(),
                    ));
                }
            }
        }
        Ok(Self {
            sequence,
            tree,
            profiles: project.definitions.fixture_profiles.clone(),
            controls,
            patch,
            controller_ports,
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
        let rendered = self
            .sequence
            .render_seconds_with_scratch(seconds, &mut scratch.sequence)
            .map_err(SequenceOutputRenderError::Render)?;
        self.finish_frame(rendered)
    }

    pub fn render_frame(
        &self,
        frame: u32,
    ) -> Result<RenderedSequenceFrame, SequenceOutputRenderError> {
        let rendered = self
            .sequence
            .render_frame(frame)
            .map_err(SequenceOutputRenderError::Render)?;
        self.finish_frame(rendered)
    }

    pub fn frame_rate(&self) -> u32 {
        self.sequence.frame_rate()
    }
    pub fn frame_count(&self) -> u32 {
        self.sequence.frame_count()
    }

    fn finish_frame(
        &self,
        rendered: RenderedFrame,
    ) -> Result<RenderedSequenceFrame, SequenceOutputRenderError> {
        let mut by_node = rendered
            .elements
            .into_iter()
            .map(|element| (element.element_id, element.pixels))
            .collect::<HashMap<_, _>>();
        let mut elements = Vec::new();
        for (node_id, node) in &self.tree.nodes {
            match &node.kind {
                ElementNodeKind::Group { .. } => {}
                ElementNodeKind::Color { cells, capability } => {
                    elements.push(RenderedElementState::Color {
                        node: *node_id,
                        capability: capability.clone(),
                        cells: by_node
                            .remove(node_id)
                            .unwrap_or_else(|| vec![black(); *cells as usize]),
                    })
                }
                ElementNodeKind::Scalar { cells } => elements.push(RenderedElementState::Scalar {
                    node: *node_id,
                    cells: vec![0.0; *cells as usize],
                }),
                ElementNodeKind::Indexed { cells, .. } => {
                    elements.push(RenderedElementState::Indexed {
                        node: *node_id,
                        cells: vec![0; *cells as usize],
                    })
                }
                ElementNodeKind::Fixture { profile } => {
                    elements.push(RenderedElementState::Fixture {
                        node: *node_id,
                        profile: profile.clone(),
                        color: by_node
                            .remove(node_id)
                            .and_then(|colors| colors.first().copied())
                            .unwrap_or_else(black),
                        state: FixtureState {
                            functions: IndexMap::new(),
                        },
                    })
                }
            }
        }
        let explicit = apply_controls(&mut elements, &self.controls, rendered.sample_time)?;
        apply_fixture_behavior_rules(&mut elements, &self.profiles, &explicit)?;
        let controller_frames = evaluate_patch(
            &self.tree,
            &self.patch,
            &self.profiles,
            &elements,
            &self.controller_ports,
        )?;
        Ok(RenderedSequenceFrame {
            frame_index: rendered.frame_index,
            frame_rate: rendered.frame_rate,
            sample_time: rendered.sample_time,
            elements,
            controller_frames,
        })
    }
}
