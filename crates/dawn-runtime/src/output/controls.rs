use std::collections::HashMap;

use dawn_language::control::{ControlClip, ControlTarget, ControlValue, controls_overlap};
use dawn_language::element::{ElementNodeId, ElementNodeKind, ElementTree};
use dawn_language::fixture_profile::{
    FixtureBehaviorRule, FixtureControlValue, FixtureFunctionId, FixtureFunctionKind,
    FixtureProfileStore,
};
use dawn_language::values::{
    SampleDuration, SampleTime, sample_duration_from_dawn_duration, sample_time_from_dawn_time,
};

use super::errors::{SequenceOutputPrepareError, SequenceOutputRenderError};
use super::frame::RenderedElementState;
use super::values::{black, sample_curve, sample_gradient, set_cell};

#[derive(Clone)]
pub(crate) struct PreparedControl {
    id: u32,
    start: SampleTime,
    duration: SampleDuration,
    kind: PreparedControlKind,
    value: ControlValue,
    addresses: Box<[PreparedControlAddress]>,
}

#[derive(Clone, Copy)]
enum PreparedControlKind {
    Scalar,
    Indexed,
    Fixture(FixtureFunctionId),
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct PreparedControlAddress {
    element: u32,
    cell: u32,
}

pub(crate) fn prepare_controls(
    tree: &ElementTree,
    profiles: &FixtureProfileStore,
    clips: &[ControlClip],
) -> Result<Vec<PreparedControl>, SequenceOutputPrepareError> {
    let rendered_nodes = tree
        .nodes
        .iter()
        .filter(|(_, node)| !matches!(node.kind, ElementNodeKind::Group { .. }))
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    let element_indexes = rendered_nodes
        .iter()
        .enumerate()
        .map(|(index, id)| (*id, index as u32))
        .collect::<HashMap<_, _>>();
    let mut prepared = Vec::with_capacity(clips.len());
    for clip in clips {
        let start = sample_time_from_dawn_time(&clip.start).map_err(|_| {
            SequenceOutputPrepareError::InvalidControl {
                clip: clip.id.0,
                reason: "control start exceeds the runtime clock range".to_string(),
            }
        })?;
        let duration = sample_duration_from_dawn_duration(&clip.duration).map_err(|_| {
            SequenceOutputPrepareError::InvalidControl {
                clip: clip.id.0,
                reason: "control duration exceeds the runtime clock range".to_string(),
            }
        })?;
        let addresses = tree
            .flatten_selection(clip.target.selection())
            .map_err(|error| SequenceOutputPrepareError::InvalidControl {
                clip: clip.id.0,
                reason: format!("{error:?}"),
            })?;
        for address in &addresses {
            let node = tree.nodes.get(&address.node).ok_or_else(|| {
                SequenceOutputPrepareError::InvalidControl {
                    clip: clip.id.0,
                    reason: "target node is missing".to_string(),
                }
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
                        return Err(SequenceOutputPrepareError::InvalidControl {
                            clip: clip.id.0,
                            reason: "fixture function is missing".to_string(),
                        });
                    }
                }
                _ => {
                    return Err(SequenceOutputPrepareError::InvalidControl {
                        clip: clip.id.0,
                        reason: "group leaves do not share the requested control type".to_string(),
                    });
                }
            }
        }
        let kind = match clip.target {
            ControlTarget::Scalar(_) => PreparedControlKind::Scalar,
            ControlTarget::Indexed(_) => PreparedControlKind::Indexed,
            ControlTarget::FixtureFunction { function, .. } => {
                PreparedControlKind::Fixture(function)
            }
        };
        let addresses = addresses
            .into_iter()
            .map(|address| {
                Ok(PreparedControlAddress {
                    element: *element_indexes.get(&address.node).ok_or_else(|| {
                        SequenceOutputPrepareError::InvalidControl {
                            clip: clip.id.0,
                            reason: "control target is not a renderable element".to_string(),
                        }
                    })?,
                    cell: address.cell,
                })
            })
            .collect::<Result<Box<[_]>, _>>()?;
        prepared.push(PreparedControl {
            id: clip.id.0,
            start,
            duration,
            kind,
            value: clip.value.clone(),
            addresses,
        });
    }
    for (index, left) in prepared.iter().enumerate() {
        for (right_index, right) in prepared.iter().enumerate().skip(index + 1) {
            if controls_overlap(&clips[index], &clips[right_index])
                && left
                    .addresses
                    .iter()
                    .any(|address| right.addresses.contains(address))
                && control_function(&clips[index].target)
                    == control_function(&clips[right_index].target)
            {
                let address = left
                    .addresses
                    .iter()
                    .find(|address| right.addresses.contains(address))
                    .copied()
                    .unwrap_or(PreparedControlAddress {
                        element: 0,
                        cell: 0,
                    });
                return Err(SequenceOutputPrepareError::ControlConflict {
                    first: left.id,
                    second: right.id,
                    node: rendered_nodes
                        .get(address.element as usize)
                        .copied()
                        .unwrap_or(ElementNodeId(0)),
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

pub(crate) fn apply_controls(
    elements: &mut [RenderedElementState],
    controls: &[PreparedControl],
    sample_time: SampleTime,
    explicit: &mut Vec<(u32, FixtureFunctionId)>,
) -> Result<(), SequenceOutputRenderError> {
    explicit.clear();
    for prepared in controls {
        let Some(elapsed) = sample_time.checked_duration_since(prepared.start) else {
            continue;
        };
        if elapsed >= prepared.duration {
            continue;
        }
        let position = if prepared.duration.ticks() == 0 {
            0.0
        } else {
            elapsed.ticks() as f32 / prepared.duration.ticks() as f32
        };
        for address in &prepared.addresses {
            let state = elements.get_mut(address.element as usize).ok_or_else(|| {
                SequenceOutputRenderError::Control {
                    clip: prepared.id,
                    reason: "rendered target is missing".to_string(),
                }
            })?;
            match (prepared.kind, &prepared.value, state) {
                (
                    PreparedControlKind::Scalar,
                    ControlValue::ConstantNormalized(value),
                    RenderedElementState::Scalar { cells, .. },
                ) => set_cell(cells, address.cell, *value, prepared.id)?,
                (
                    PreparedControlKind::Scalar,
                    ControlValue::NormalizedCurve(curve),
                    RenderedElementState::Scalar { cells, .. },
                ) => set_cell(
                    cells,
                    address.cell,
                    sample_curve(curve, position),
                    prepared.id,
                )?,
                (
                    PreparedControlKind::Indexed,
                    ControlValue::Indexed { option, .. },
                    RenderedElementState::Indexed { cells, .. },
                ) => set_cell(cells, address.cell, option.0, prepared.id)?,
                (
                    PreparedControlKind::Fixture(function),
                    value,
                    RenderedElementState::Fixture { state, .. },
                ) => {
                    let value = fixture_control_value(value, position).ok_or_else(|| {
                        SequenceOutputRenderError::Control {
                            clip: prepared.id,
                            reason: "control value does not match the fixture function".to_string(),
                        }
                    })?;
                    state.insert(function, value);
                    explicit.push((address.element, function));
                }
                _ => {
                    return Err(SequenceOutputRenderError::Control {
                        clip: prepared.id,
                        reason: "control value does not match its target".to_string(),
                    });
                }
            }
        }
    }
    Ok(())
}

fn fixture_control_value(value: &ControlValue, position: f32) -> Option<FixtureControlValue> {
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

pub(crate) fn apply_fixture_behavior_rules(
    elements: &mut [RenderedElementState],
    profiles: &FixtureProfileStore,
    explicit: &[(u32, FixtureFunctionId)],
) -> Result<(), SequenceOutputRenderError> {
    for (element_index, element) in elements.iter_mut().enumerate() {
        let RenderedElementState::Fixture {
            node,
            profile,
            color,
            state,
        } = element
        else {
            continue;
        };
        let profile = profiles.definitions.get(profile).ok_or_else(|| {
            SequenceOutputRenderError::Patch("fixture profile is missing".to_string())
        })?;
        let active = *color != black();
        for (function, definition) in &profile.functions {
            if explicit.contains(&(element_index as u32, *function)) {
                continue;
            }
            if matches!(definition.kind, FixtureFunctionKind::ColorMixing { .. }) {
                state.insert(*function, FixtureControlValue::Color(*color));
            }
        }
        for rule in &profile.behavior_rules {
            let function = match rule {
                FixtureBehaviorRule::Shutter { function, .. }
                | FixtureBehaviorRule::Dimmer { function, .. }
                | FixtureBehaviorRule::ColorWheel { function, .. }
                | FixtureBehaviorRule::PrismGate { function, .. } => *function,
            };
            if explicit.contains(&(element_index as u32, function)) {
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
                        SequenceOutputRenderError::UnsupportedFixtureColor {
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
            state.insert(function, value);
        }
    }
    Ok(())
}
