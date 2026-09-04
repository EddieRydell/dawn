use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use dawn_language::element::{ElementNodeKind, ElementTree};
use dawn_language::fixture_profile::{FixtureProfileId, FixtureProfileStore, FixtureState};
use dawn_language::patch::{
    FilterDefinition, PatchGraph, PatchNode, PatchNodeId, PatchPortId, PatchValue, PatchValueType,
    evaluate_filter_into,
};

use super::errors::{SequenceOutputPrepareError, SequenceOutputRenderError};
use super::frame::{ControllerPortFrame, RenderedElementState};

static NEXT_PATCH_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Clone)]
pub(crate) struct PreparedPatch {
    id: u32,
    steps: Box<[PreparedPatchStep]>,
    value_types: Box<[PatchValueType]>,
}

#[derive(Clone)]
enum PreparedPatchStep {
    Source {
        output: usize,
        source: PreparedPatchSource,
    },
    Filter {
        input: usize,
        output_start: usize,
        output_count: usize,
        filter: FilterDefinition,
    },
    Sink {
        input: usize,
        frame: usize,
        start: usize,
        end: usize,
    },
}

#[derive(Clone)]
struct PreparedPatchSource {
    cells: Box<[PreparedSourceCell]>,
    kind: PreparedSourceKind,
}

#[derive(Clone, Copy)]
struct PreparedSourceCell {
    element: usize,
    cell: usize,
}

#[derive(Clone)]
enum PreparedSourceKind {
    Color,
    Scalar,
    Indexed,
    FixtureState(FixtureProfileId),
}

#[derive(Debug, Default)]
pub(crate) struct PatchScratch {
    patch_id: Option<u32>,
    values: Vec<PatchValue>,
}

impl PreparedPatch {
    pub(crate) fn prepare(
        tree: &ElementTree,
        patch: &PatchGraph,
        frames: &[ControllerPortFrame],
    ) -> Result<Self, SequenceOutputPrepareError> {
        let order = patch
            .validate()
            .map_err(|error| SequenceOutputPrepareError::InvalidPatch(format!("{error:?}")))?;
        let incoming = patch
            .edges
            .iter()
            .map(|edge| (edge.to, (edge.from, edge.from_port)))
            .collect::<HashMap<_, _>>();
        let element_indexes = tree
            .nodes
            .iter()
            .filter(|(_, node)| !matches!(node.kind, ElementNodeKind::Group { .. }))
            .enumerate()
            .map(|(index, (id, _))| (*id, index))
            .collect::<HashMap<_, _>>();
        let mut outputs = HashMap::<(PatchNodeId, PatchPortId), usize>::new();
        let mut value_types = Vec::new();
        let mut steps = Vec::with_capacity(order.len());

        for id in order {
            let node = patch.nodes.get(&id).ok_or_else(|| {
                SequenceOutputPrepareError::InvalidPatch(
                    "validated patch node disappeared".to_string(),
                )
            })?;
            match node {
                PatchNode::Source(source) => {
                    let addresses = tree.flatten_selection(&source.selection).map_err(|error| {
                        SequenceOutputPrepareError::InvalidPatch(format!(
                            "invalid source selection: {error:?}"
                        ))
                    })?;
                    if addresses.len() != source.output.width() {
                        return Err(SequenceOutputPrepareError::InvalidPatch(
                            "patch source width does not match its selected element span"
                                .to_string(),
                        ));
                    }
                    let cells = addresses
                        .into_iter()
                        .map(|address| {
                            Ok(PreparedSourceCell {
                                element: *element_indexes.get(&address.node).ok_or_else(|| {
                                    SequenceOutputPrepareError::InvalidPatch(
                                        "patch source references a group or missing element"
                                            .to_string(),
                                    )
                                })?,
                                cell: address.cell as usize,
                            })
                        })
                        .collect::<Result<Vec<_>, SequenceOutputPrepareError>>()?;
                    let kind = match &source.output {
                        PatchValueType::Color { .. } => PreparedSourceKind::Color,
                        PatchValueType::Scalar { .. } => PreparedSourceKind::Scalar,
                        PatchValueType::Indexed { .. } => PreparedSourceKind::Indexed,
                        PatchValueType::FixtureState { profile, .. } => {
                            PreparedSourceKind::FixtureState(profile.clone())
                        }
                        PatchValueType::Components { .. } | PatchValueType::Slots { .. } => {
                            return Err(SequenceOutputPrepareError::InvalidPatch(
                                "patch source declares a derived value type".to_string(),
                            ));
                        }
                    };
                    let output = value_types.len();
                    value_types.push(source.output.clone());
                    outputs.insert((id, PatchPortId(0)), output);
                    steps.push(PreparedPatchStep::Source {
                        output,
                        source: PreparedPatchSource {
                            cells: cells.into_boxed_slice(),
                            kind,
                        },
                    });
                }
                PatchNode::Filter(filter) => {
                    let input = incoming
                        .get(&id)
                        .and_then(|source| outputs.get(source))
                        .copied()
                        .ok_or_else(|| {
                            SequenceOutputPrepareError::InvalidPatch(
                                "filter input is missing".to_string(),
                            )
                        })?;
                    let output_start = value_types.len();
                    let output_count = usize::from(filter.output_port_count());
                    for port in 0..filter.output_port_count() {
                        let value_type =
                            filter.output_type(PatchPortId(port)).ok_or_else(|| {
                                SequenceOutputPrepareError::InvalidPatch(
                                    "filter output is invalid".to_string(),
                                )
                            })?;
                        outputs.insert((id, PatchPortId(port)), value_types.len());
                        value_types.push(value_type);
                    }
                    steps.push(PreparedPatchStep::Filter {
                        input,
                        output_start,
                        output_count,
                        filter: filter.clone(),
                    });
                }
                PatchNode::Sink(sink) => {
                    let input = incoming
                        .get(&id)
                        .and_then(|source| outputs.get(source))
                        .copied()
                        .ok_or_else(|| {
                            SequenceOutputPrepareError::InvalidPatch(
                                "sink input is missing".to_string(),
                            )
                        })?;
                    let frame = frames
                        .iter()
                        .position(|frame| {
                            frame.controller == sink.controller && frame.port == sink.port
                        })
                        .ok_or_else(|| {
                            SequenceOutputPrepareError::InvalidPatch(
                                "patch sink references a controller port outside the active setup"
                                    .to_string(),
                            )
                        })?;
                    let start = usize::from(sink.start_slot);
                    let end = start
                        .checked_add(usize::from(sink.slot_count))
                        .ok_or_else(|| {
                            SequenceOutputPrepareError::InvalidPatch(
                                "patch sink slot range overflowed".to_string(),
                            )
                        })?;
                    if end > frames[frame].slots.len() {
                        return Err(SequenceOutputPrepareError::InvalidPatch(
                            "patch sink exceeds its controller port".to_string(),
                        ));
                    }
                    steps.push(PreparedPatchStep::Sink {
                        input,
                        frame,
                        start,
                        end,
                    });
                }
            }
        }

        Ok(Self {
            id: NEXT_PATCH_ID.fetch_add(1, Ordering::Relaxed),
            steps: steps.into_boxed_slice(),
            value_types: value_types.into_boxed_slice(),
        })
    }

    pub(crate) fn evaluate(
        &self,
        profiles: &FixtureProfileStore,
        elements: &[RenderedElementState],
        frames: &mut [ControllerPortFrame],
        scratch: &mut PatchScratch,
    ) -> Result<(), SequenceOutputRenderError> {
        if scratch.patch_id != Some(self.id) {
            scratch.values = self.value_types.iter().map(PatchValue::empty).collect();
            scratch.patch_id = Some(self.id);
        }
        for frame in frames.iter_mut() {
            frame.slots.fill(0);
        }
        for step in &self.steps {
            match step {
                PreparedPatchStep::Source { output, source } => {
                    source.write(elements, &mut scratch.values[*output])?;
                }
                PreparedPatchStep::Filter {
                    input,
                    output_start,
                    output_count,
                    filter,
                } => {
                    let (inputs, outputs) = scratch.values.split_at_mut(*output_start);
                    let input = inputs.get(*input).ok_or_else(|| {
                        SequenceOutputRenderError::Patch(
                            "prepared filter input is out of bounds".to_string(),
                        )
                    })?;
                    let outputs = outputs.get_mut(..*output_count).ok_or_else(|| {
                        SequenceOutputRenderError::Patch(
                            "prepared filter output is out of bounds".to_string(),
                        )
                    })?;
                    evaluate_filter_into(filter, input, profiles, outputs)
                        .map_err(|error| SequenceOutputRenderError::Patch(format!("{error:?}")))?;
                }
                PreparedPatchStep::Sink {
                    input,
                    frame,
                    start,
                    end,
                } => {
                    let PatchValue::Slots(slots) = &scratch.values[*input] else {
                        return Err(SequenceOutputRenderError::Patch(
                            "prepared sink input is not a slot vector".to_string(),
                        ));
                    };
                    if slots.len() != end - start {
                        return Err(SequenceOutputRenderError::Patch(
                            "prepared sink width changed".to_string(),
                        ));
                    }
                    frames[*frame].slots[*start..*end].copy_from_slice(slots);
                }
            }
        }
        Ok(())
    }
}

impl PreparedPatchSource {
    fn write(
        &self,
        elements: &[RenderedElementState],
        output: &mut PatchValue,
    ) -> Result<(), SequenceOutputRenderError> {
        match (&self.kind, output) {
            (PreparedSourceKind::Color, PatchValue::Colors(output)) => {
                output.clear();
                for cell in &self.cells {
                    let value = match elements.get(cell.element) {
                        Some(RenderedElementState::Color { cells, .. }) => {
                            cells.get(cell.cell).copied()
                        }
                        Some(RenderedElementState::Fixture { color, .. }) if cell.cell == 0 => {
                            Some(*color)
                        }
                        _ => None,
                    }
                    .ok_or_else(source_type_error)?;
                    output.push(value);
                }
            }
            (PreparedSourceKind::Scalar, PatchValue::Scalars(output)) => {
                output.clear();
                for cell in &self.cells {
                    let value = match elements.get(cell.element) {
                        Some(RenderedElementState::Scalar { cells, .. }) => {
                            cells.get(cell.cell).copied()
                        }
                        _ => None,
                    }
                    .ok_or_else(source_type_error)?;
                    output.push(value);
                }
            }
            (PreparedSourceKind::Indexed, PatchValue::Indexed(output)) => {
                output.clear();
                for cell in &self.cells {
                    let value = match elements.get(cell.element) {
                        Some(RenderedElementState::Indexed { cells, .. }) => {
                            cells.get(cell.cell).copied()
                        }
                        _ => None,
                    }
                    .ok_or_else(source_type_error)?;
                    output.push(value);
                }
            }
            (PreparedSourceKind::FixtureState(profile), PatchValue::FixtureStates(output)) => {
                output.resize_with(self.cells.len(), || FixtureState {
                    functions: Vec::new(),
                });
                for (output, cell) in output.iter_mut().zip(&self.cells) {
                    let state = match elements.get(cell.element) {
                        Some(RenderedElementState::Fixture {
                            profile: found,
                            state,
                            ..
                        }) if found == profile && cell.cell == 0 => Some(state),
                        _ => None,
                    }
                    .ok_or_else(source_type_error)?;
                    output.functions.clone_from(&state.functions);
                }
            }
            _ => return Err(source_type_error()),
        }
        Ok(())
    }
}

fn source_type_error() -> SequenceOutputRenderError {
    SequenceOutputRenderError::Patch(
        "prepared patch source no longer matches its element type".to_string(),
    )
}
