use std::collections::HashSet;

use dawn_language::control::{ControlClip, ControlTarget, ControlValue, controls_overlap};
use dawn_language::element::{ElementCellAddress, ElementNodeId, ElementNodeKind, ElementTree};
use dawn_language::fixture_profile::{
    FixtureBehaviorRule, FixtureControlValue, FixtureFunctionId, FixtureFunctionKind,
    FixtureProfileStore,
};

use super::errors::{SequenceOutputPrepareError, SequenceOutputRenderError};
use super::frame::RenderedElementState;
use super::values::{black, sample_curve, sample_gradient, set_cell};

#[derive(Clone)]
pub(crate) struct PreparedControl {
    clip: ControlClip,
    addresses: Vec<ElementCellAddress>,
}

pub(crate) fn prepare_controls(
    tree: &ElementTree,
    profiles: &FixtureProfileStore,
    clips: &[ControlClip],
) -> Result<Vec<PreparedControl>, SequenceOutputPrepareError> {
    let mut prepared = Vec::new();
    for clip in clips {
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
                return Err(SequenceOutputPrepareError::ControlConflict {
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

pub(crate) fn apply_controls(
    elements: &mut [RenderedElementState],
    controls: &[PreparedControl],
    seconds: f64,
) -> Result<HashSet<(ElementNodeId, FixtureFunctionId)>, SequenceOutputRenderError> {
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
                .ok_or_else(|| SequenceOutputRenderError::Control {
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
                        SequenceOutputRenderError::Control {
                            clip: prepared.clip.id.0,
                            reason: "control value does not match the fixture function".to_string(),
                        }
                    })?;
                    state.functions.insert(*function, value);
                    explicit.insert((address.node, *function));
                }
                _ => {
                    return Err(SequenceOutputRenderError::Control {
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

pub(crate) fn apply_fixture_behavior_rules(
    elements: &mut [RenderedElementState],
    profiles: &FixtureProfileStore,
    explicit: &HashSet<(ElementNodeId, FixtureFunctionId)>,
) -> Result<(), SequenceOutputRenderError> {
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
        let profile = profiles.definitions.get(profile).ok_or_else(|| {
            SequenceOutputRenderError::Patch("fixture profile is missing".to_string())
        })?;
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
            state.functions.insert(function, value);
        }
    }
    Ok(())
}
