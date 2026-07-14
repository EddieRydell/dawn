use std::collections::HashSet;

use crate::control::{ControlValidationError, controls_overlap, validate_control_clip};
use crate::element::ElementTreeValidationError;
use crate::model::DawnProject;
use crate::preview::PreviewValidationError;

#[derive(Clone, Debug, PartialEq)]
pub enum ProjectValidationError {
    MissingSetup,
    MissingElementTree,
    MissingPreviewLayout,
    MissingPatch,
    MissingController,
    MissingFixtureProfile,
    InvalidRelationship(String),
    ElementTree(ElementTreeValidationError),
    FixtureProfile(crate::fixture_profile::FixtureProfileValidationError),
    Controller(crate::controller::ControllerValidationError),
    Patch(crate::patch::PatchValidationError),
    Preview(PreviewValidationError),
    Control(ControlValidationError),
}

pub fn validate_project(project: &DawnProject) -> Result<(), ProjectValidationError> {
    let setup = project
        .setups
        .get(&project.root.setup)
        .ok_or(ProjectValidationError::MissingSetup)?;
    let tree = project
        .element_trees
        .get(&setup.elements)
        .ok_or(ProjectValidationError::MissingElementTree)?;
    tree.validate()
        .map_err(ProjectValidationError::ElementTree)?;

    for node in tree.nodes.values() {
        if let crate::element::ElementNodeKind::Fixture { profile } = &node.kind {
            let profile = project
                .definitions
                .fixture_profiles
                .definitions
                .get(profile)
                .ok_or(ProjectValidationError::MissingFixtureProfile)?;
            profile
                .validate()
                .map_err(ProjectValidationError::FixtureProfile)?;
        }
    }

    let layout = project
        .preview_layouts
        .get(&setup.preview)
        .ok_or(ProjectValidationError::MissingPreviewLayout)?;
    if layout.element_tree != setup.elements {
        return Err(ProjectValidationError::Preview(
            PreviewValidationError::MissingElementCell {
                prop: crate::preview::PropInstanceId(0),
                address: crate::element::ElementCellAddress {
                    node: crate::element::ElementNodeId(0),
                    cell: 0,
                },
            },
        ));
    }
    let cells = tree
        .flattened_cells()
        .map_err(|_| ProjectValidationError::MissingElementTree)?
        .into_iter()
        .collect::<HashSet<_>>();
    let mut props = HashSet::new();
    for prop in &layout.props {
        if !props.insert(prop.id) {
            return Err(ProjectValidationError::Preview(
                PreviewValidationError::DuplicateProp(prop.id),
            ));
        }
        let definition = project
            .definitions
            .props
            .definitions
            .get(&prop.definition)
            .ok_or_else(|| {
                ProjectValidationError::Preview(PreviewValidationError::MissingDefinition(
                    prop.definition.clone(),
                ))
            })?;
        let expected = definition.geometry.point_count();
        if prop.bindings.len() != expected {
            return Err(ProjectValidationError::Preview(
                PreviewValidationError::BindingCount {
                    prop: prop.id,
                    expected,
                    actual: prop.bindings.len(),
                },
            ));
        }
        for address in &prop.bindings {
            if !cells.contains(address) {
                return Err(ProjectValidationError::Preview(
                    PreviewValidationError::MissingElementCell {
                        prop: prop.id,
                        address: *address,
                    },
                ));
            }
        }
    }

    let patch = project
        .patches
        .get(&setup.patch)
        .ok_or(ProjectValidationError::MissingPatch)?;
    patch.validate().map_err(ProjectValidationError::Patch)?;
    for controller in &setup.controllers {
        project
            .controllers
            .get(controller)
            .ok_or(ProjectValidationError::MissingController)?
            .validate()
            .map_err(ProjectValidationError::Controller)?;
    }
    for sequence in project.sequences.values() {
        for effect in &sequence.effects {
            if effect.target.tree != tree.id {
                return Err(ProjectValidationError::InvalidRelationship(
                    "effect targets a different element tree".to_string(),
                ));
            }
            let addresses = tree.flatten_selection(&effect.target).map_err(|error| {
                ProjectValidationError::InvalidRelationship(format!(
                    "invalid effect selection: {error:?}"
                ))
            })?;
            for address in addresses {
                let node = tree
                    .nodes
                    .get(&address.node)
                    .ok_or(ProjectValidationError::MissingElementTree)?;
                match &node.kind {
                    crate::element::ElementNodeKind::Color { .. } => {}
                    crate::element::ElementNodeKind::Fixture { profile } => {
                        let accepts_color = project.definitions.fixture_profiles.definitions.get(profile).is_some_and(|profile| profile.functions.values().any(|function| matches!(function.kind, crate::fixture_profile::FixtureFunctionKind::ColorMixing { .. })));
                        if !accepts_color {
                            return Err(ProjectValidationError::InvalidRelationship(
                                "effect targets a fixture without configured color support"
                                    .to_string(),
                            ));
                        }
                    }
                    _ => {
                        return Err(ProjectValidationError::InvalidRelationship(
                            "color effect targets a non-color element".to_string(),
                        ));
                    }
                }
            }
        }
        for clip in &sequence.control_clips {
            validate_control_clip(clip).map_err(ProjectValidationError::Control)?;
        }
        for (index, clip) in sequence.control_clips.iter().enumerate() {
            for other in sequence.control_clips.iter().skip(index + 1) {
                if controls_overlap(clip, other) {
                    return Err(ProjectValidationError::Control(
                        ControlValidationError::Conflict {
                            first: clip.id,
                            second: other.id,
                        },
                    ));
                }
            }
        }
    }
    for node in patch.nodes.values() {
        match node {
            crate::patch::PatchNode::Source(source) => {
                if source.selection.tree != tree.id {
                    return Err(ProjectValidationError::InvalidRelationship(
                        "patch source targets a different element tree".to_string(),
                    ));
                }
                let width = tree
                    .flatten_selection(&source.selection)
                    .map_err(|error| {
                        ProjectValidationError::InvalidRelationship(format!(
                            "invalid patch source selection: {error:?}"
                        ))
                    })?
                    .len();
                if width != source.output.width() {
                    return Err(ProjectValidationError::InvalidRelationship(
                        "patch source width does not match its element selection".to_string(),
                    ));
                }
            }
            crate::patch::PatchNode::Sink(sink) => {
                if !setup.controllers.contains(&sink.controller) {
                    return Err(ProjectValidationError::InvalidRelationship(
                        "patch sink controller is not active in the setup".to_string(),
                    ));
                }
                let controller = project
                    .controllers
                    .get(&sink.controller)
                    .ok_or(ProjectValidationError::MissingController)?;
                let port = controller
                    .ports
                    .iter()
                    .find(|port| port.id == sink.port)
                    .ok_or_else(|| {
                        ProjectValidationError::InvalidRelationship(
                            "patch sink controller port is missing".to_string(),
                        )
                    })?;
                let end = sink
                    .start_slot
                    .checked_add(sink.slot_count)
                    .ok_or_else(|| {
                        ProjectValidationError::InvalidRelationship(
                            "patch sink slot range overflowed".to_string(),
                        )
                    })?;
                if end > port.slot_count {
                    return Err(ProjectValidationError::InvalidRelationship(
                        "patch sink exceeds its controller port".to_string(),
                    ));
                }
            }
            crate::patch::PatchNode::Filter(
                crate::patch::FilterDefinition::FixtureProfileEncoding {
                    profile,
                    slot_count,
                    ..
                },
            ) => {
                let definition = project
                    .definitions
                    .fixture_profiles
                    .definitions
                    .get(profile)
                    .ok_or(ProjectValidationError::MissingFixtureProfile)?;
                if *slot_count != definition.slot_count() {
                    return Err(ProjectValidationError::InvalidRelationship(
                        "fixture-profile filter slot width differs from the profile".to_string(),
                    ));
                }
            }
            crate::patch::PatchNode::Filter(_) => {}
        }
    }
    Ok(())
}
