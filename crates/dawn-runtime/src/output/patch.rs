use std::collections::HashMap;

use dawn_language::element::{ElementSelection, ElementTree};
use dawn_language::fixture_profile::FixtureProfileStore;
use dawn_language::patch::{
    PatchGraph, PatchNode, PatchNodeId, PatchPortId, PatchValue, evaluate_filter,
};

use super::errors::{SequenceOutputPrepareError, SequenceOutputRenderError};
use super::frame::{ControllerPortFrame, RenderedElementState};
use super::values::check_source_width;

pub(crate) fn evaluate_patch(
    tree: &ElementTree,
    patch: &PatchGraph,
    profiles: &FixtureProfileStore,
    elements: &[RenderedElementState],
    empty_frames: &[ControllerPortFrame],
) -> Result<Vec<ControllerPortFrame>, SequenceOutputRenderError> {
    let order = patch
        .validate()
        .map_err(|error| SequenceOutputRenderError::Patch(format!("{error:?}")))?;
    let incoming = patch
        .edges
        .iter()
        .map(|edge| (edge.to, (edge.from, edge.from_port)))
        .collect::<HashMap<_, _>>();
    let mut values: HashMap<(PatchNodeId, PatchPortId), PatchValue> = HashMap::new();
    let mut frames = empty_frames.to_vec();
    for id in order {
        match patch.nodes.get(&id).ok_or_else(|| {
            SequenceOutputRenderError::Patch("prepared patch node disappeared".to_string())
        })? {
            PatchNode::Source(source) => {
                values.insert(
                    (id, PatchPortId(0)),
                    source_value(tree, &source.selection, &source.output, elements)?,
                );
            }
            PatchNode::Filter(filter) => {
                let source = incoming.get(&id).ok_or_else(|| {
                    SequenceOutputRenderError::Patch("filter input is missing".to_string())
                })?;
                let input = values.get(source).ok_or_else(|| {
                    SequenceOutputRenderError::Patch("filter source value is missing".to_string())
                })?;
                for (port, output) in evaluate_filter(filter, input, profiles)
                    .map_err(|error| SequenceOutputRenderError::Patch(format!("{error:?}")))?
                    .into_iter()
                    .enumerate()
                {
                    values.insert((id, PatchPortId(port as u16)), output);
                }
            }
            PatchNode::Sink(sink) => {
                let source = incoming.get(&id).ok_or_else(|| {
                    SequenceOutputRenderError::Patch("sink input is missing".to_string())
                })?;
                let PatchValue::Slots(slots) = values.get(source).ok_or_else(|| {
                    SequenceOutputRenderError::Patch("sink source value is missing".to_string())
                })?
                else {
                    return Err(SequenceOutputRenderError::Patch(
                        "sink input is not a slot vector".to_string(),
                    ));
                };
                let frame = frames
                    .iter_mut()
                    .find(|frame| frame.controller == sink.controller && frame.port == sink.port)
                    .ok_or_else(|| {
                        SequenceOutputRenderError::Patch(
                            "sink controller port is missing".to_string(),
                        )
                    })?;
                let start = usize::from(sink.start_slot);
                let end = start + usize::from(sink.slot_count);
                if slots.len() != usize::from(sink.slot_count) || end > frame.slots.len() {
                    return Err(SequenceOutputRenderError::Patch(
                        "sink width does not match its destination".to_string(),
                    ));
                }
                frame.slots[start..end].copy_from_slice(slots);
            }
        }
    }
    Ok(frames)
}

fn source_value(
    tree: &ElementTree,
    selection: &ElementSelection,
    output: &dawn_language::patch::PatchValueType,
    elements: &[RenderedElementState],
) -> Result<PatchValue, SequenceOutputRenderError> {
    let addresses = tree.flatten_selection(selection).map_err(|error| {
        SequenceOutputRenderError::Patch(format!("invalid source selection: {error:?}"))
    })?;
    match output {
        dawn_language::patch::PatchValueType::Color { width } => {
            let colors = addresses
                .iter()
                .map(
                    |address| match elements.iter().find(|state| state.node() == address.node) {
                        Some(RenderedElementState::Color { cells, .. }) => {
                            cells.get(address.cell as usize).copied()
                        }
                        Some(RenderedElementState::Fixture { color, .. }) if address.cell == 0 => {
                            Some(*color)
                        }
                        _ => None,
                    },
                )
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    SequenceOutputRenderError::Patch(
                        "color source targets a non-color element".to_string(),
                    )
                })?;
            check_source_width(*width, colors.len())?;
            Ok(PatchValue::Colors(colors))
        }
        dawn_language::patch::PatchValueType::Scalar { width } => {
            let values = addresses
                .iter()
                .map(
                    |address| match elements.iter().find(|state| state.node() == address.node) {
                        Some(RenderedElementState::Scalar { cells, .. }) => {
                            cells.get(address.cell as usize).copied()
                        }
                        _ => None,
                    },
                )
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    SequenceOutputRenderError::Patch(
                        "scalar source targets a non-scalar element".to_string(),
                    )
                })?;
            check_source_width(*width, values.len())?;
            Ok(PatchValue::Scalars(values))
        }
        dawn_language::patch::PatchValueType::Indexed { width } => {
            let values = addresses
                .iter()
                .map(
                    |address| match elements.iter().find(|state| state.node() == address.node) {
                        Some(RenderedElementState::Indexed { cells, .. }) => {
                            cells.get(address.cell as usize).copied()
                        }
                        _ => None,
                    },
                )
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    SequenceOutputRenderError::Patch(
                        "indexed source targets a non-indexed element".to_string(),
                    )
                })?;
            check_source_width(*width, values.len())?;
            Ok(PatchValue::Indexed(values))
        }
        dawn_language::patch::PatchValueType::FixtureState { width, profile } => {
            let values = addresses
                .iter()
                .map(
                    |address| match elements.iter().find(|state| state.node() == address.node) {
                        Some(RenderedElementState::Fixture {
                            profile: found,
                            state,
                            ..
                        }) if found == profile && address.cell == 0 => Some(state.clone()),
                        _ => None,
                    },
                )
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    SequenceOutputRenderError::Patch(
                        "fixture source targets an incompatible profile".to_string(),
                    )
                })?;
            check_source_width(*width, values.len())?;
            Ok(PatchValue::FixtureStates(values))
        }
        _ => Err(SequenceOutputRenderError::Patch(
            "patch source declares a derived value type".to_string(),
        )),
    }
}

pub(crate) fn validate_patch_sources(
    tree: &ElementTree,
    patch: &PatchGraph,
) -> Result<(), SequenceOutputPrepareError> {
    for node in patch.nodes.values() {
        if let PatchNode::Source(source) = node {
            let addresses = tree
                .flatten_selection(&source.selection)
                .map_err(|error| SequenceOutputPrepareError::InvalidPatch(format!("{error:?}")))?;
            if addresses.len() != source.output.width() {
                return Err(SequenceOutputPrepareError::InvalidPatch(
                    "patch source width does not match its selected element span".to_string(),
                ));
            }
        }
    }
    Ok(())
}
