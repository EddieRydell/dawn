use std::collections::{HashMap, HashSet, VecDeque};

use indexmap::IndexMap;

use crate::controller::{ControllerId, ControllerPortId};
use crate::element::{ColorCapability, DiscreteColorMapping, ElementSelection, EmitterId};
use crate::fixture_profile::{
    ColorComponent, FixtureChannelRole, FixtureControlValue, FixtureProfileStore, FixtureState,
};
use crate::fixture_profile::{DimmingCurve, FixtureProfileId};
use crate::identity::SourceIdentity;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PatchId(pub SourceIdentity);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct PatchNodeId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct PatchPortId(pub u16);

#[derive(Clone, Debug, PartialEq)]
pub struct PatchGraph {
    pub id: PatchId,
    pub nodes: IndexMap<PatchNodeId, PatchNode>,
    pub edges: Vec<PatchEdge>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PatchNode {
    Source(PatchSource),
    Filter(FilterDefinition),
    Sink(PatchSink),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatchSource {
    pub selection: ElementSelection,
    pub output: PatchValueType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatchSink {
    pub controller: ControllerId,
    pub port: ControllerPortId,
    pub start_slot: u16,
    pub slot_count: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatchEdge {
    pub from: PatchNodeId,
    pub from_port: PatchPortId,
    pub to: PatchNodeId,
    pub to_port: PatchPortId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PatchValueType {
    Color {
        width: usize,
    },
    Scalar {
        width: usize,
    },
    Indexed {
        width: usize,
    },
    FixtureState {
        width: usize,
        profile: FixtureProfileId,
    },
    Components {
        width: usize,
    },
    Slots {
        width: usize,
    },
}

impl PatchValueType {
    pub fn width(&self) -> usize {
        match self {
            Self::Color { width }
            | Self::Scalar { width }
            | Self::Indexed { width }
            | Self::FixtureState { width, .. }
            | Self::Components { width }
            | Self::Slots { width } => *width,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum FilterDefinition {
    ColorBreakdown {
        capability: ColorCapability,
        cell_count: usize,
    },
    DimmingCurve {
        curve: DimmingCurve,
        width: usize,
    },
    ScaleInvert {
        scale: f32,
        invert: bool,
        width: usize,
    },
    FanOut {
        width: usize,
        outputs: u16,
    },
    ComponentReorder {
        components_per_cell: u16,
        order: Vec<u16>,
        cell_count: usize,
    },
    IndexedValueMapping {
        entries: IndexMap<u32, f32>,
        width: usize,
    },
    Quantize8 {
        width: usize,
    },
    Quantize16 {
        width: usize,
        byte_order: ByteOrder,
    },
    FixtureProfileEncoding {
        profile: FixtureProfileId,
        fixture_count: usize,
        slot_count: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ByteOrder {
    CoarseFine,
    FineCoarse,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PatchValue {
    Colors(Vec<crate::values::Color>),
    Scalars(Vec<f32>),
    Indexed(Vec<u32>),
    FixtureStates(Vec<FixtureState>),
    Components(Vec<f32>),
    Slots(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum FilterEvaluationError {
    TypeMismatch,
    WidthMismatch { expected: usize, actual: usize },
    UnsupportedDiscreteColor(crate::values::Color),
    MissingIndexedMapping(u32),
    MissingFixtureProfile(FixtureProfileId),
    MissingFixtureFunction,
    MissingFixtureEntry,
    InvalidFixtureSlot,
}

pub fn evaluate_filter(
    filter: &FilterDefinition,
    input: &PatchValue,
    profiles: &FixtureProfileStore,
) -> Result<Vec<PatchValue>, FilterEvaluationError> {
    let output = match (filter, input) {
        (
            FilterDefinition::ColorBreakdown {
                capability,
                cell_count,
            },
            PatchValue::Colors(colors),
        ) => {
            check_width(*cell_count, colors.len())?;
            let mut components =
                Vec::with_capacity(*cell_count * color_component_count(capability));
            for color in colors {
                let rgb = [
                    f32::from(color.red) / 255.0,
                    f32::from(color.green) / 255.0,
                    f32::from(color.blue) / 255.0,
                ];
                match capability {
                    ColorCapability::Rgb => components.extend(rgb),
                    ColorCapability::Rgbw => {
                        let white = rgb[0].min(rgb[1]).min(rgb[2]);
                        components.extend([rgb[0] - white, rgb[1] - white, rgb[2] - white, white]);
                    }
                    ColorCapability::Discrete { emitters, mappings } => {
                        let levels = discrete_mapping(mappings, *color)
                            .ok_or(FilterEvaluationError::UnsupportedDiscreteColor(*color))?;
                        components.extend(
                            emitters
                                .iter()
                                .map(|emitter| levels.get(&emitter.id).copied().unwrap_or(0.0)),
                        );
                    }
                }
            }
            PatchValue::Components(components)
        }
        (FilterDefinition::DimmingCurve { curve, width }, PatchValue::Components(values)) => {
            check_width(*width, values.len())?;
            PatchValue::Components(
                values
                    .iter()
                    .map(|value| apply_dimming_curve(curve, *value))
                    .collect(),
            )
        }
        (
            FilterDefinition::ScaleInvert {
                scale,
                invert,
                width,
            },
            PatchValue::Components(values),
        ) => {
            check_width(*width, values.len())?;
            PatchValue::Components(
                values
                    .iter()
                    .map(|value| {
                        let value = if *invert {
                            1.0 - value.clamp(0.0, 1.0)
                        } else {
                            *value
                        };
                        (value * scale).clamp(0.0, 1.0)
                    })
                    .collect(),
            )
        }
        (FilterDefinition::FanOut { width, outputs }, PatchValue::Components(values)) => {
            check_width(*width, values.len())?;
            return Ok((0..*outputs)
                .map(|_| PatchValue::Components(values.clone()))
                .collect());
        }
        (
            FilterDefinition::ComponentReorder {
                components_per_cell,
                order,
                cell_count,
            },
            PatchValue::Components(values),
        ) => {
            let per_cell = usize::from(*components_per_cell);
            check_width(per_cell * *cell_count, values.len())?;
            let mut output = Vec::with_capacity(values.len());
            for cell in values.chunks_exact(per_cell) {
                output.extend(order.iter().map(|component| cell[usize::from(*component)]));
            }
            PatchValue::Components(output)
        }
        (FilterDefinition::IndexedValueMapping { entries, width }, PatchValue::Indexed(values)) => {
            check_width(*width, values.len())?;
            PatchValue::Components(
                values
                    .iter()
                    .map(|value| {
                        entries
                            .get(value)
                            .copied()
                            .ok_or(FilterEvaluationError::MissingIndexedMapping(*value))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )
        }
        (FilterDefinition::Quantize8 { width }, PatchValue::Components(values)) => {
            check_width(*width, values.len())?;
            PatchValue::Slots(values.iter().map(|value| quantize8(*value)).collect())
        }
        (FilterDefinition::Quantize16 { width, byte_order }, PatchValue::Components(values)) => {
            check_width(*width, values.len())?;
            let mut slots = Vec::with_capacity(*width * 2);
            for value in values {
                let encoded = quantize16(*value).to_be_bytes();
                match byte_order {
                    ByteOrder::CoarseFine => slots.extend(encoded),
                    ByteOrder::FineCoarse => slots.extend([encoded[1], encoded[0]]),
                }
            }
            PatchValue::Slots(slots)
        }
        (
            FilterDefinition::FixtureProfileEncoding {
                profile,
                fixture_count,
                slot_count,
            },
            PatchValue::FixtureStates(states),
        ) => {
            check_width(*fixture_count, states.len())?;
            let profile_definition = profiles
                .definitions
                .get(profile)
                .ok_or_else(|| FilterEvaluationError::MissingFixtureProfile(profile.clone()))?;
            PatchValue::Slots(encode_fixture_states(
                profile_definition,
                states,
                *slot_count,
            )?)
        }
        _ => return Err(FilterEvaluationError::TypeMismatch),
    };
    Ok(vec![output])
}

pub fn apply_dimming_curve(curve: &DimmingCurve, value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    match curve {
        DimmingCurve::Linear => value,
        DimmingCurve::Gamma(gamma) => value.powf(*gamma),
        DimmingCurve::Custom(curve) => {
            let Some(first) = curve.points.first() else {
                return value;
            };
            if value <= first.position {
                return first.value;
            }
            for pair in curve.points.windows(2) {
                if value <= pair[1].position {
                    let span = pair[1].position - pair[0].position;
                    let amount = if span <= 0.0 {
                        0.0
                    } else {
                        (value - pair[0].position) / span
                    };
                    return pair[0].value + (pair[1].value - pair[0].value) * amount;
                }
            }
            curve.points.last().map_or(value, |point| point.value)
        }
    }
    .clamp(0.0, 1.0)
}

pub fn quantize8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
pub fn quantize16(value: f32) -> u16 {
    (value.clamp(0.0, 1.0) * 65_535.0).round() as u16
}

fn check_width(expected: usize, actual: usize) -> Result<(), FilterEvaluationError> {
    if expected == actual {
        Ok(())
    } else {
        Err(FilterEvaluationError::WidthMismatch { expected, actual })
    }
}

fn encode_fixture_states(
    profile: &crate::fixture_profile::FixtureProfile,
    states: &[FixtureState],
    slot_count: usize,
) -> Result<Vec<u8>, FilterEvaluationError> {
    let mut output = vec![0; states.len() * slot_count];
    let fine_functions = profile
        .channels
        .iter()
        .filter_map(|channel| match channel.role {
            FixtureChannelRole::Fine { function } => Some(function),
            _ => None,
        })
        .collect::<HashSet<_>>();
    for (fixture_index, state) in states.iter().enumerate() {
        for channel in &profile.channels {
            let slot = usize::from(channel.slot);
            if slot >= slot_count {
                return Err(FilterEvaluationError::InvalidFixtureSlot);
            }
            let encoded = match channel.role {
                FixtureChannelRole::Ignored => 0,
                FixtureChannelRole::ColorComponent {
                    function,
                    component,
                } => {
                    let FixtureControlValue::Color(color) = state
                        .functions
                        .get(&function)
                        .ok_or(FilterEvaluationError::MissingFixtureFunction)?
                    else {
                        return Err(FilterEvaluationError::TypeMismatch);
                    };
                    let rgb = [
                        f32::from(color.red) / 255.0,
                        f32::from(color.green) / 255.0,
                        f32::from(color.blue) / 255.0,
                    ];
                    let white = rgb[0].min(rgb[1]).min(rgb[2]);
                    let value = match component {
                        ColorComponent::Red => rgb[0] - white,
                        ColorComponent::Green => rgb[1] - white,
                        ColorComponent::Blue => rgb[2] - white,
                        ColorComponent::White => white,
                    };
                    quantize8(apply_dimming_curve(&channel.curve, value))
                }
                FixtureChannelRole::Coarse { function } | FixtureChannelRole::Fine { function } => {
                    let definition = profile
                        .functions
                        .get(&function)
                        .ok_or(FilterEvaluationError::MissingFixtureFunction)?;
                    let value = state
                        .functions
                        .get(&function)
                        .ok_or(FilterEvaluationError::MissingFixtureFunction)?;
                    let normalized = match value {
                        FixtureControlValue::Normalized(value) => {
                            apply_dimming_curve(&definition.curve, *value)
                        }
                        FixtureControlValue::Indexed { entry, range } => {
                            let entries = match &definition.kind {
                                crate::fixture_profile::FixtureFunctionKind::Indexed {
                                    entries,
                                }
                                | crate::fixture_profile::FixtureFunctionKind::ColorWheel {
                                    entries,
                                } => entries,
                                _ => return Err(FilterEvaluationError::TypeMismatch),
                            };
                            let entry = entries
                                .iter()
                                .find(|candidate| candidate.id == *entry)
                                .ok_or(FilterEvaluationError::MissingFixtureEntry)?;
                            let dmx = f32::from(entry.dmx_min)
                                + f32::from(entry.dmx_max - entry.dmx_min) * range.clamp(0.0, 1.0);
                            dmx / if fine_functions.contains(&function) {
                                65_535.0
                            } else {
                                255.0
                            }
                        }
                        FixtureControlValue::Color(_) => {
                            return Err(FilterEvaluationError::TypeMismatch);
                        }
                    };
                    let encoded =
                        quantize16(apply_dimming_curve(&channel.curve, normalized)).to_be_bytes();
                    match channel.role {
                        FixtureChannelRole::Coarse { .. } if fine_functions.contains(&function) => {
                            encoded[0]
                        }
                        FixtureChannelRole::Fine { .. } => encoded[1],
                        FixtureChannelRole::Coarse { .. } => quantize8(normalized),
                        _ => 0,
                    }
                }
            };
            output[fixture_index * slot_count + slot] = encoded;
        }
    }
    Ok(output)
}

#[derive(Clone, Debug, PartialEq)]
pub enum PatchValidationError {
    MissingNode(PatchNodeId),
    InvalidPort {
        node: PatchNodeId,
        port: PatchPortId,
    },
    SourceHasInput(PatchNodeId),
    SinkHasOutput(PatchNodeId),
    MissingInput {
        node: PatchNodeId,
        port: PatchPortId,
    },
    MultipleInputs {
        node: PatchNodeId,
        port: PatchPortId,
    },
    Cycle,
    TypeMismatch {
        from: PatchNodeId,
        to: PatchNodeId,
    },
    InvalidWidth(PatchNodeId),
    SinkWidthMismatch {
        node: PatchNodeId,
        value_width: usize,
        slot_count: u16,
    },
    DestinationOverlap {
        controller: ControllerId,
        port: ControllerPortId,
    },
    InvalidFilter(PatchNodeId),
}

impl FilterDefinition {
    pub fn input_type(&self) -> PatchValueType {
        match self {
            Self::ColorBreakdown { cell_count, .. } => PatchValueType::Color { width: *cell_count },
            Self::DimmingCurve { width, .. } | Self::ScaleInvert { width, .. } => {
                PatchValueType::Components { width: *width }
            }
            Self::FanOut { width, .. } => PatchValueType::Components { width: *width },
            Self::ComponentReorder {
                components_per_cell,
                cell_count,
                ..
            } => PatchValueType::Components {
                width: usize::from(*components_per_cell) * *cell_count,
            },
            Self::IndexedValueMapping { width, .. } => PatchValueType::Indexed { width: *width },
            Self::Quantize8 { width } | Self::Quantize16 { width, .. } => {
                PatchValueType::Components { width: *width }
            }
            Self::FixtureProfileEncoding {
                profile,
                fixture_count,
                ..
            } => PatchValueType::FixtureState {
                width: *fixture_count,
                profile: profile.clone(),
            },
        }
    }

    pub fn output_type(&self, port: PatchPortId) -> Option<PatchValueType> {
        match self {
            Self::ColorBreakdown {
                capability,
                cell_count,
            } => Some(PatchValueType::Components {
                width: color_component_count(capability) * *cell_count,
            }),
            Self::DimmingCurve { width, .. }
            | Self::ScaleInvert { width, .. }
            | Self::IndexedValueMapping { width, .. } => {
                Some(PatchValueType::Components { width: *width })
            }
            Self::FanOut { width, outputs } if port.0 < *outputs => {
                Some(PatchValueType::Components { width: *width })
            }
            Self::FanOut { .. } => None,
            Self::ComponentReorder {
                components_per_cell,
                cell_count,
                ..
            } => Some(PatchValueType::Components {
                width: usize::from(*components_per_cell) * *cell_count,
            }),
            Self::Quantize8 { width } => Some(PatchValueType::Slots { width: *width }),
            Self::Quantize16 { width, .. } => Some(PatchValueType::Slots { width: *width * 2 }),
            Self::FixtureProfileEncoding {
                fixture_count,
                slot_count,
                ..
            } => Some(PatchValueType::Slots {
                width: *fixture_count * *slot_count,
            }),
        }
    }

    pub fn output_port_count(&self) -> u16 {
        match self {
            Self::FanOut { outputs, .. } => *outputs,
            _ => 1,
        }
    }

    pub fn validate(&self) -> bool {
        match self {
            Self::ColorBreakdown {
                capability,
                cell_count,
            } => *cell_count > 0 && color_component_count(capability) > 0,
            Self::DimmingCurve { width, .. } => *width > 0,
            Self::ScaleInvert { scale, width, .. } => scale.is_finite() && *width > 0,
            Self::FanOut { width, outputs } => *width > 0 && *outputs > 0,
            Self::ComponentReorder {
                components_per_cell,
                order,
                cell_count,
            } => {
                *components_per_cell > 0
                    && *cell_count > 0
                    && order.len() == usize::from(*components_per_cell)
                    && order.iter().copied().collect::<HashSet<_>>().len() == order.len()
                    && order
                        .iter()
                        .all(|component| *component < *components_per_cell)
            }
            Self::IndexedValueMapping { entries, width } => {
                *width > 0
                    && !entries.is_empty()
                    && entries
                        .values()
                        .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
            }
            Self::Quantize8 { width } | Self::Quantize16 { width, .. } => *width > 0,
            Self::FixtureProfileEncoding {
                fixture_count,
                slot_count,
                ..
            } => *fixture_count > 0 && *slot_count > 0,
        }
    }
}

impl PatchGraph {
    pub fn validate(&self) -> Result<Vec<PatchNodeId>, PatchValidationError> {
        for (id, node) in &self.nodes {
            if node_width(node) == 0 {
                return Err(PatchValidationError::InvalidWidth(*id));
            }
            if let PatchNode::Filter(filter) = node
                && !filter.validate()
            {
                return Err(PatchValidationError::InvalidFilter(*id));
            }
        }
        let mut incoming: HashMap<(PatchNodeId, PatchPortId), usize> = HashMap::new();
        let mut outgoing: HashMap<PatchNodeId, Vec<PatchNodeId>> = HashMap::new();
        let mut indegree: HashMap<PatchNodeId, usize> =
            self.nodes.keys().map(|id| (*id, 0)).collect();
        for edge in &self.edges {
            let from = self
                .nodes
                .get(&edge.from)
                .ok_or(PatchValidationError::MissingNode(edge.from))?;
            let to = self
                .nodes
                .get(&edge.to)
                .ok_or(PatchValidationError::MissingNode(edge.to))?;
            if matches!(from, PatchNode::Sink(_)) {
                return Err(PatchValidationError::SinkHasOutput(edge.from));
            }
            if matches!(to, PatchNode::Source(_)) {
                return Err(PatchValidationError::SourceHasInput(edge.to));
            }
            let output =
                output_type(from, edge.from_port).ok_or(PatchValidationError::InvalidPort {
                    node: edge.from,
                    port: edge.from_port,
                })?;
            if edge.to_port != PatchPortId(0) {
                return Err(PatchValidationError::InvalidPort {
                    node: edge.to,
                    port: edge.to_port,
                });
            }
            let input = input_type(to).ok_or(PatchValidationError::InvalidPort {
                node: edge.to,
                port: edge.to_port,
            })?;
            if output != input {
                return Err(PatchValidationError::TypeMismatch {
                    from: edge.from,
                    to: edge.to,
                });
            }
            let count = incoming.entry((edge.to, edge.to_port)).or_default();
            *count += 1;
            if *count > 1 {
                return Err(PatchValidationError::MultipleInputs {
                    node: edge.to,
                    port: edge.to_port,
                });
            }
            outgoing.entry(edge.from).or_default().push(edge.to);
            *indegree.entry(edge.to).or_default() += 1;
        }
        for (id, node) in &self.nodes {
            if !matches!(node, PatchNode::Source(_))
                && incoming.get(&(*id, PatchPortId(0))).copied() != Some(1)
            {
                return Err(PatchValidationError::MissingInput {
                    node: *id,
                    port: PatchPortId(0),
                });
            }
            if let PatchNode::Sink(sink) = node {
                let input = input_type(node).unwrap_or(PatchValueType::Slots { width: 0 });
                if input.width() != usize::from(sink.slot_count) {
                    return Err(PatchValidationError::SinkWidthMismatch {
                        node: *id,
                        value_width: input.width(),
                        slot_count: sink.slot_count,
                    });
                }
            }
        }
        validate_destinations(&self.nodes)?;
        let mut ready = indegree
            .iter()
            .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
            .collect::<VecDeque<_>>();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(id) = ready.pop_front() {
            order.push(id);
            if let Some(next) = outgoing.get(&id) {
                for target in next {
                    let degree = indegree
                        .get_mut(target)
                        .ok_or(PatchValidationError::MissingNode(*target))?;
                    *degree -= 1;
                    if *degree == 0 {
                        ready.push_back(*target);
                    }
                }
            }
        }
        if order.len() != self.nodes.len() {
            return Err(PatchValidationError::Cycle);
        }
        Ok(order)
    }
}

fn node_width(node: &PatchNode) -> usize {
    match node {
        PatchNode::Source(source) => source.output.width(),
        PatchNode::Filter(filter) => filter
            .output_type(PatchPortId(0))
            .map_or(0, |value| value.width()),
        PatchNode::Sink(sink) => usize::from(sink.slot_count),
    }
}

fn input_type(node: &PatchNode) -> Option<PatchValueType> {
    match node {
        PatchNode::Source(_) => None,
        PatchNode::Filter(filter) => Some(filter.input_type()),
        PatchNode::Sink(sink) => Some(PatchValueType::Slots {
            width: usize::from(sink.slot_count),
        }),
    }
}

fn output_type(node: &PatchNode, port: PatchPortId) -> Option<PatchValueType> {
    match node {
        PatchNode::Source(source) if port == PatchPortId(0) => Some(source.output.clone()),
        PatchNode::Source(_) => None,
        PatchNode::Filter(filter) => filter.output_type(port),
        PatchNode::Sink(_) => None,
    }
}

fn validate_destinations(
    nodes: &IndexMap<PatchNodeId, PatchNode>,
) -> Result<(), PatchValidationError> {
    let mut destinations: HashMap<(ControllerId, ControllerPortId), Vec<(u16, u16)>> =
        HashMap::new();
    for node in nodes.values() {
        let PatchNode::Sink(sink) = node else {
            continue;
        };
        let end = sink
            .start_slot
            .checked_add(sink.slot_count)
            .ok_or_else(|| PatchValidationError::DestinationOverlap {
                controller: sink.controller.clone(),
                port: sink.port,
            })?;
        let ranges = destinations
            .entry((sink.controller.clone(), sink.port))
            .or_default();
        if ranges
            .iter()
            .any(|(start, existing_end)| sink.start_slot < *existing_end && *start < end)
        {
            return Err(PatchValidationError::DestinationOverlap {
                controller: sink.controller.clone(),
                port: sink.port,
            });
        }
        ranges.push((sink.start_slot, end));
    }
    Ok(())
}

pub fn color_component_count(capability: &ColorCapability) -> usize {
    match capability {
        ColorCapability::Rgb => 3,
        ColorCapability::Rgbw => 4,
        ColorCapability::Discrete { emitters, .. } => emitters.len(),
    }
}

pub fn discrete_mapping(
    mappings: &[DiscreteColorMapping],
    color: crate::values::Color,
) -> Option<&IndexMap<EmitterId, f32>> {
    mappings
        .iter()
        .find(|mapping| mapping.color == color)
        .map(|mapping| &mapping.levels)
}
