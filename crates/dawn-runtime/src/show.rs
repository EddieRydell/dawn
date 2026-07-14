use std::collections::{HashMap, HashSet};

use dawn_language::control::{ControlClip, ControlTarget, ControlValue, controls_overlap};
use dawn_language::controller::{ControllerId, ControllerPortId};
use dawn_language::element::{
    ColorCapability, ElementCellAddress, ElementNodeId, ElementNodeKind, ElementSelection,
    ElementTree,
};
use dawn_language::fixture_profile::{
    FixtureBehaviorRule, FixtureControlValue, FixtureFunctionId, FixtureFunctionKind,
    FixtureProfileId, FixtureProfileStore, FixtureState,
};
use dawn_language::model::DawnProject;
use dawn_language::patch::{
    PatchGraph, PatchNode, PatchNodeId, PatchPortId, PatchValue, evaluate_filter,
};
use dawn_language::sequence::SequenceId;
use dawn_language::setup::SetupId;
use dawn_language::validation::validate_project;
use dawn_language::values::{Color, Curve, Gradient};
use indexmap::IndexMap;

use crate::{PreparedSequenceRenderer, RenderError, RenderedFrame, SequenceRenderScratch};

#[derive(Clone, Debug, PartialEq)]
pub enum RenderedElementState {
    Color {
        node: ElementNodeId,
        capability: ColorCapability,
        cells: Vec<Color>,
    },
    Scalar {
        node: ElementNodeId,
        cells: Vec<f64>,
    },
    Indexed {
        node: ElementNodeId,
        cells: Vec<u32>,
    },
    Fixture {
        node: ElementNodeId,
        profile: FixtureProfileId,
        color: Color,
        state: FixtureState,
    },
}

impl RenderedElementState {
    pub fn node(&self) -> ElementNodeId {
        match self {
            Self::Color { node, .. }
            | Self::Scalar { node, .. }
            | Self::Indexed { node, .. }
            | Self::Fixture { node, .. } => *node,
        }
    }

    pub fn preview_colors(&self) -> Vec<Color> {
        match self {
            Self::Color { cells, .. } => cells.clone(),
            Self::Fixture { color, .. } => vec![*color],
            Self::Scalar { cells, .. } => cells.iter().map(|level| grayscale(*level)).collect(),
            Self::Indexed { cells, .. } => vec![black(); cells.len()],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControllerPortFrame {
    pub controller: ControllerId,
    pub port: ControllerPortId,
    pub slots: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderedShowFrame {
    pub frame_index: u64,
    pub frame_rate: u32,
    pub clock_seconds: f64,
    pub sample_seconds: f64,
    pub elements: Vec<RenderedElementState>,
    pub controller_frames: Vec<ControllerPortFrame>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ShowPrepareError {
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
pub enum ShowRenderError {
    Render(RenderError),
    Control { clip: u32, reason: String },
    UnsupportedFixtureColor { node: ElementNodeId, color: Color },
    Patch(String),
}

pub struct PreparedRenderSession {
    sequence: PreparedSequenceRenderer,
    tree: ElementTree,
    profiles: FixtureProfileStore,
    controls: Vec<PreparedControl>,
    patch: PatchGraph,
    controller_ports: Vec<ControllerPortFrame>,
}

#[derive(Clone)]
struct PreparedControl {
    clip: ControlClip,
    addresses: Vec<ElementCellAddress>,
}

#[derive(Debug, Default)]
pub struct RenderSessionScratch {
    sequence: SequenceRenderScratch,
}

impl PreparedRenderSession {
    pub fn prepare(
        project: &DawnProject,
        setup_id: &SetupId,
        sequence_id: &SequenceId,
    ) -> Result<Self, ShowPrepareError> {
        validate_project(project)
            .map_err(|error| ShowPrepareError::ProjectValidation(format!("{error:?}")))?;
        let setup = project
            .setups
            .get(setup_id)
            .ok_or(ShowPrepareError::MissingSetup)?;
        let tree = project
            .element_trees
            .get(&setup.elements)
            .ok_or(ShowPrepareError::MissingElementTree)?
            .clone();
        let sequence_definition = project
            .sequences
            .get(sequence_id)
            .ok_or(ShowPrepareError::MissingSequence)?;
        if sequence_definition
            .effects
            .iter()
            .any(|effect| effect.target.tree != tree.id)
        {
            return Err(ShowPrepareError::InvalidEffectTree);
        }
        let controls = prepare_controls(
            &tree,
            &project.definitions.fixture_profiles,
            &sequence_definition.control_clips,
        )?;
        let sequence = PreparedSequenceRenderer::prepare(project, setup_id, sequence_id)
            .map_err(ShowPrepareError::Render)?;
        let patch = project
            .patches
            .get(&setup.patch)
            .ok_or_else(|| ShowPrepareError::InvalidPatch("setup patch is missing".to_string()))?
            .clone();
        patch
            .validate()
            .map_err(|error| ShowPrepareError::InvalidPatch(format!("{error:?}")))?;
        validate_patch_sources(&tree, &patch)?;
        let mut controller_ports = Vec::new();
        for controller_id in &setup.controllers {
            let controller = project.controllers.get(controller_id).ok_or_else(|| {
                ShowPrepareError::InvalidPatch("setup controller is missing".to_string())
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
                        ShowPrepareError::InvalidPatch(
                            "patch sink references a controller port outside the active setup"
                                .to_string(),
                        )
                    })?;
                let end = usize::from(sink.start_slot)
                    .checked_add(usize::from(sink.slot_count))
                    .ok_or_else(|| {
                        ShowPrepareError::InvalidPatch(
                            "patch sink slot range overflowed".to_string(),
                        )
                    })?;
                if end > frame.slots.len() {
                    return Err(ShowPrepareError::InvalidPatch(
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

    pub fn render_seconds(&self, seconds: f64) -> Result<RenderedShowFrame, ShowRenderError> {
        self.render_seconds_with_scratch(seconds, &mut RenderSessionScratch::default())
    }

    pub fn render_seconds_with_scratch(
        &self,
        seconds: f64,
        scratch: &mut RenderSessionScratch,
    ) -> Result<RenderedShowFrame, ShowRenderError> {
        let rendered = self
            .sequence
            .render_seconds_with_scratch(seconds, &mut scratch.sequence)
            .map_err(ShowRenderError::Render)?;
        self.finish_frame(rendered)
    }

    pub fn render_frame(&self, frame: u64) -> Result<RenderedShowFrame, ShowRenderError> {
        let rendered = self
            .sequence
            .render_frame(frame)
            .map_err(ShowRenderError::Render)?;
        self.finish_frame(rendered)
    }

    pub fn frame_rate(&self) -> u32 {
        self.sequence.frame_rate()
    }
    pub fn frame_count(&self) -> u64 {
        self.sequence.frame_count()
    }

    fn finish_frame(&self, rendered: RenderedFrame) -> Result<RenderedShowFrame, ShowRenderError> {
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
        let explicit = apply_controls(&mut elements, &self.controls, rendered.sample_seconds)?;
        apply_fixture_behavior_rules(&mut elements, &self.profiles, &explicit)?;
        let controller_frames = evaluate_patch(
            &self.tree,
            &self.patch,
            &self.profiles,
            &elements,
            &self.controller_ports,
        )?;
        Ok(RenderedShowFrame {
            frame_index: rendered.frame_index,
            frame_rate: rendered.frame_rate,
            clock_seconds: rendered.clock_seconds,
            sample_seconds: rendered.sample_seconds,
            elements,
            controller_frames,
        })
    }
}

fn prepare_controls(
    tree: &ElementTree,
    profiles: &FixtureProfileStore,
    clips: &[ControlClip],
) -> Result<Vec<PreparedControl>, ShowPrepareError> {
    let mut prepared = Vec::new();
    for clip in clips {
        let addresses = tree
            .flatten_selection(clip.target.selection())
            .map_err(|error| ShowPrepareError::InvalidControl {
                clip: clip.id.0,
                reason: format!("{error:?}"),
            })?;
        for address in &addresses {
            let node =
                tree.nodes
                    .get(&address.node)
                    .ok_or_else(|| ShowPrepareError::InvalidControl {
                        clip: clip.id.0,
                        reason: "target node is missing".to_string(),
                    })?;
            match (&clip.target, &node.kind) {
                (ControlTarget::Scalar(_), ElementNodeKind::Scalar { .. })
                | (ControlTarget::Indexed(_), ElementNodeKind::Indexed { .. }) => {}
                (
                    ControlTarget::FixtureFunction { function, .. },
                    ElementNodeKind::Fixture { profile },
                ) => {
                    if !profiles
                        .definitions
                        .get(profile)
                        .is_some_and(|profile| profile.functions.contains_key(function))
                    {
                        return Err(ShowPrepareError::InvalidControl {
                            clip: clip.id.0,
                            reason: "fixture function is missing".to_string(),
                        });
                    }
                }
                _ => {
                    return Err(ShowPrepareError::InvalidControl {
                        clip: clip.id.0,
                        reason: "group leaves do not share the requested control type".to_string(),
                    });
                }
            }
        }
        prepared.push(PreparedControl {
            clip: clip.clone(),
            addresses,
        });
    }
    for (index, left) in prepared.iter().enumerate() {
        for right in prepared.iter().skip(index + 1) {
            if controls_overlap(&left.clip, &right.clip)
                && left
                    .addresses
                    .iter()
                    .any(|address| right.addresses.contains(address))
                && control_function(&left.clip.target) == control_function(&right.clip.target)
            {
                let address = left
                    .addresses
                    .iter()
                    .find(|address| right.addresses.contains(address))
                    .copied()
                    .unwrap_or(ElementCellAddress {
                        node: ElementNodeId(0),
                        cell: 0,
                    });
                return Err(ShowPrepareError::ControlConflict {
                    first: left.clip.id.0,
                    second: right.clip.id.0,
                    node: address.node,
                    cell: address.cell,
                });
            }
        }
    }
    Ok(prepared)
}

fn control_function(target: &ControlTarget) -> Option<FixtureFunctionId> {
    match target {
        ControlTarget::FixtureFunction { function, .. } => Some(*function),
        _ => None,
    }
}

fn apply_controls(
    elements: &mut [RenderedElementState],
    controls: &[PreparedControl],
    seconds: f64,
) -> Result<HashSet<(ElementNodeId, FixtureFunctionId)>, ShowRenderError> {
    let mut explicit = HashSet::new();
    for prepared in controls {
        let start = prepared.clip.start.as_seconds_f64();
        let duration = prepared.clip.duration.as_seconds_f64();
        if seconds < start || seconds >= start + duration {
            continue;
        }
        let position = if duration <= 0.0 {
            0.0
        } else {
            ((seconds - start) / duration).clamp(0.0, 1.0)
        };
        for address in &prepared.addresses {
            let state = elements
                .iter_mut()
                .find(|state| state.node() == address.node)
                .ok_or_else(|| ShowRenderError::Control {
                    clip: prepared.clip.id.0,
                    reason: "rendered target is missing".to_string(),
                })?;
            match (&prepared.clip.target, &prepared.clip.value, state) {
                (
                    ControlTarget::Scalar(_),
                    ControlValue::ConstantNormalized(value),
                    RenderedElementState::Scalar { cells, .. },
                ) => set_cell(cells, address.cell, *value, prepared.clip.id.0)?,
                (
                    ControlTarget::Scalar(_),
                    ControlValue::NormalizedCurve(curve),
                    RenderedElementState::Scalar { cells, .. },
                ) => set_cell(
                    cells,
                    address.cell,
                    sample_curve(curve, position),
                    prepared.clip.id.0,
                )?,
                (
                    ControlTarget::Indexed(_),
                    ControlValue::Indexed { option, .. },
                    RenderedElementState::Indexed { cells, .. },
                ) => set_cell(cells, address.cell, option.0, prepared.clip.id.0)?,
                (
                    ControlTarget::FixtureFunction { function, .. },
                    value,
                    RenderedElementState::Fixture { state, .. },
                ) => {
                    let value = fixture_control_value(value, position).ok_or_else(|| {
                        ShowRenderError::Control {
                            clip: prepared.clip.id.0,
                            reason: "control value does not match the fixture function".to_string(),
                        }
                    })?;
                    state.functions.insert(*function, value);
                    explicit.insert((address.node, *function));
                }
                _ => {
                    return Err(ShowRenderError::Control {
                        clip: prepared.clip.id.0,
                        reason: "control value does not match its target".to_string(),
                    });
                }
            }
        }
    }
    Ok(explicit)
}

fn fixture_control_value(value: &ControlValue, position: f64) -> Option<FixtureControlValue> {
    match value {
        ControlValue::ConstantNormalized(value) => Some(FixtureControlValue::Normalized(*value)),
        ControlValue::NormalizedCurve(curve) => Some(FixtureControlValue::Normalized(
            sample_curve(curve, position),
        )),
        ControlValue::FixtureIndexed { entry, range_curve } => Some(FixtureControlValue::Indexed {
            entry: *entry,
            range: range_curve
                .as_ref()
                .map_or(0.0, |curve| sample_curve(curve, position)),
        }),
        ControlValue::ConstantColor(color) => Some(FixtureControlValue::Color(*color)),
        ControlValue::Gradient(gradient) => {
            sample_gradient(gradient, position).map(FixtureControlValue::Color)
        }
        ControlValue::Indexed { .. } => None,
    }
}

fn apply_fixture_behavior_rules(
    elements: &mut [RenderedElementState],
    profiles: &FixtureProfileStore,
    explicit: &HashSet<(ElementNodeId, FixtureFunctionId)>,
) -> Result<(), ShowRenderError> {
    for element in elements {
        let RenderedElementState::Fixture {
            node,
            profile,
            color,
            state,
        } = element
        else {
            continue;
        };
        let profile = profiles
            .definitions
            .get(profile)
            .ok_or_else(|| ShowRenderError::Patch("fixture profile is missing".to_string()))?;
        let active = *color != black();
        for (function, definition) in &profile.functions {
            if explicit.contains(&(*node, *function)) {
                continue;
            }
            if matches!(definition.kind, FixtureFunctionKind::ColorMixing { .. }) {
                state
                    .functions
                    .insert(*function, FixtureControlValue::Color(*color));
            }
        }
        for rule in &profile.behavior_rules {
            let function = match rule {
                FixtureBehaviorRule::Shutter { function, .. }
                | FixtureBehaviorRule::Dimmer { function, .. }
                | FixtureBehaviorRule::ColorWheel { function, .. }
                | FixtureBehaviorRule::PrismGate { function, .. } => *function,
            };
            if explicit.contains(&(*node, function)) {
                continue;
            }
            let value = match rule {
                FixtureBehaviorRule::Shutter { closed, open, .. } => FixtureControlValue::Indexed {
                    entry: if active { *open } else { *closed },
                    range: 0.0,
                },
                FixtureBehaviorRule::Dimmer { off, on, .. } => {
                    FixtureControlValue::Normalized(if active { *on } else { *off })
                }
                FixtureBehaviorRule::ColorWheel { entries, .. } => {
                    let entry = entries.iter().find(|entry| entry.color == *color).ok_or(
                        ShowRenderError::UnsupportedFixtureColor {
                            node: *node,
                            color: *color,
                        },
                    )?;
                    FixtureControlValue::Indexed {
                        entry: entry.entry,
                        range: 0.0,
                    }
                }
                FixtureBehaviorRule::PrismGate {
                    disabled, enabled, ..
                } => FixtureControlValue::Indexed {
                    entry: if active { *enabled } else { *disabled },
                    range: 0.0,
                },
            };
            state.functions.insert(function, value);
        }
    }
    Ok(())
}

fn evaluate_patch(
    tree: &ElementTree,
    patch: &PatchGraph,
    profiles: &FixtureProfileStore,
    elements: &[RenderedElementState],
    empty_frames: &[ControllerPortFrame],
) -> Result<Vec<ControllerPortFrame>, ShowRenderError> {
    let order = patch
        .validate()
        .map_err(|error| ShowRenderError::Patch(format!("{error:?}")))?;
    let incoming = patch
        .edges
        .iter()
        .map(|edge| (edge.to, (edge.from, edge.from_port)))
        .collect::<HashMap<_, _>>();
    let mut values: HashMap<(PatchNodeId, PatchPortId), PatchValue> = HashMap::new();
    let mut frames = empty_frames.to_vec();
    for id in order {
        match patch
            .nodes
            .get(&id)
            .ok_or_else(|| ShowRenderError::Patch("prepared patch node disappeared".to_string()))?
        {
            PatchNode::Source(source) => {
                values.insert(
                    (id, PatchPortId(0)),
                    source_value(tree, &source.selection, &source.output, elements)?,
                );
            }
            PatchNode::Filter(filter) => {
                let source = incoming
                    .get(&id)
                    .ok_or_else(|| ShowRenderError::Patch("filter input is missing".to_string()))?;
                let input = values.get(source).ok_or_else(|| {
                    ShowRenderError::Patch("filter source value is missing".to_string())
                })?;
                for (port, output) in evaluate_filter(filter, input, profiles)
                    .map_err(|error| ShowRenderError::Patch(format!("{error:?}")))?
                    .into_iter()
                    .enumerate()
                {
                    values.insert((id, PatchPortId(port as u16)), output);
                }
            }
            PatchNode::Sink(sink) => {
                let source = incoming
                    .get(&id)
                    .ok_or_else(|| ShowRenderError::Patch("sink input is missing".to_string()))?;
                let PatchValue::Slots(slots) = values.get(source).ok_or_else(|| {
                    ShowRenderError::Patch("sink source value is missing".to_string())
                })?
                else {
                    return Err(ShowRenderError::Patch(
                        "sink input is not a slot vector".to_string(),
                    ));
                };
                let frame = frames
                    .iter_mut()
                    .find(|frame| frame.controller == sink.controller && frame.port == sink.port)
                    .ok_or_else(|| {
                        ShowRenderError::Patch("sink controller port is missing".to_string())
                    })?;
                let start = usize::from(sink.start_slot);
                let end = start + usize::from(sink.slot_count);
                if slots.len() != usize::from(sink.slot_count) || end > frame.slots.len() {
                    return Err(ShowRenderError::Patch(
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
) -> Result<PatchValue, ShowRenderError> {
    let addresses = tree
        .flatten_selection(selection)
        .map_err(|error| ShowRenderError::Patch(format!("invalid source selection: {error:?}")))?;
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
                    ShowRenderError::Patch("color source targets a non-color element".to_string())
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
                    ShowRenderError::Patch("scalar source targets a non-scalar element".to_string())
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
                    ShowRenderError::Patch(
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
                    ShowRenderError::Patch(
                        "fixture source targets an incompatible profile".to_string(),
                    )
                })?;
            check_source_width(*width, values.len())?;
            Ok(PatchValue::FixtureStates(values))
        }
        _ => Err(ShowRenderError::Patch(
            "patch source declares a derived value type".to_string(),
        )),
    }
}

fn validate_patch_sources(tree: &ElementTree, patch: &PatchGraph) -> Result<(), ShowPrepareError> {
    for node in patch.nodes.values() {
        if let PatchNode::Source(source) = node {
            let addresses = tree
                .flatten_selection(&source.selection)
                .map_err(|error| ShowPrepareError::InvalidPatch(format!("{error:?}")))?;
            if addresses.len() != source.output.width() {
                return Err(ShowPrepareError::InvalidPatch(
                    "patch source width does not match its selected element span".to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn set_cell<T: Copy>(
    cells: &mut [T],
    cell: u32,
    value: T,
    clip: u32,
) -> Result<(), ShowRenderError> {
    let target = cells
        .get_mut(cell as usize)
        .ok_or_else(|| ShowRenderError::Control {
            clip,
            reason: "control cell is out of range".to_string(),
        })?;
    *target = value;
    Ok(())
}
fn check_source_width(expected: usize, actual: usize) -> Result<(), ShowRenderError> {
    if expected == actual {
        Ok(())
    } else {
        Err(ShowRenderError::Patch(format!(
            "source width {actual} does not match declared width {expected}"
        )))
    }
}
fn sample_curve(curve: &Curve, position: f64) -> f64 {
    let Some(first) = curve.points.first() else {
        return 0.0;
    };
    if position <= first.position {
        return first.value;
    }
    for pair in curve.points.windows(2) {
        if position <= pair[1].position {
            let span = pair[1].position - pair[0].position;
            let amount = if span <= 0.0 {
                0.0
            } else {
                (position - pair[0].position) / span
            };
            return pair[0].value + (pair[1].value - pair[0].value) * amount;
        }
    }
    curve.points.last().map_or(0.0, |point| point.value)
}
fn sample_gradient(gradient: &Gradient, position: f64) -> Option<Color> {
    let first = gradient.stops.first()?;
    if position <= first.position {
        return Some(first.color);
    }
    for pair in gradient.stops.windows(2) {
        if position <= pair[1].position {
            let span = pair[1].position - pair[0].position;
            let amount = if span <= 0.0 {
                0.0
            } else {
                (position - pair[0].position) / span
            };
            return Some(Color {
                red: lerp_u8(pair[0].color.red, pair[1].color.red, amount),
                green: lerp_u8(pair[0].color.green, pair[1].color.green, amount),
                blue: lerp_u8(pair[0].color.blue, pair[1].color.blue, amount),
            });
        }
    }
    gradient.stops.last().map(|stop| stop.color)
}
fn lerp_u8(left: u8, right: u8, amount: f64) -> u8 {
    (f64::from(left) + (f64::from(right) - f64::from(left)) * amount.clamp(0.0, 1.0)).round() as u8
}
fn black() -> Color {
    Color {
        red: 0,
        green: 0,
        blue: 0,
    }
}
fn grayscale(value: f64) -> Color {
    let channel = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color {
        red: channel,
        green: channel,
        blue: channel,
    }
}
