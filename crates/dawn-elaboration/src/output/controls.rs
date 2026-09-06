use std::collections::HashMap;

use dawn_language::control::{ControlClip, ControlTarget, ControlValue, controls_overlap};
use dawn_language::element::{ElementNodeKind, ElementTree};
use dawn_language::fixture_profile::{
    FixtureBehaviorRule, FixtureControlValue, FixtureFunctionId, FixtureFunctionKind,
    FixtureProfileId, FixtureProfileStore,
};
use dawn_language::values::{sample_duration_from_dawn_duration, sample_time_from_dawn_time};

use super::elements::OutputElements;
use super::errors::SequenceOutputPrepareError;
use dawn_runtime::control::{
    PreparedControl, PreparedControlAddress, PreparedControlKind, PreparedControlValue,
};
use dawn_runtime::fixture::{FixtureBehavior, FixtureBehaviors};

pub(crate) fn prepare_controls(
    tree: &ElementTree,
    profiles: &FixtureProfileStore,
    clips: &[ControlClip],
    elements: &OutputElements,
) -> Result<Vec<PreparedControl>, SequenceOutputPrepareError> {
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
                    element: *elements.indexes.get(&address.node).ok_or_else(|| {
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
            value: match &clip.value {
                ControlValue::ConstantNormalized(value) => {
                    PreparedControlValue::ConstantNormalized(*value)
                }
                ControlValue::NormalizedCurve(curve) => {
                    PreparedControlValue::NormalizedCurve(curve.points.clone().into_boxed_slice())
                }
                ControlValue::Indexed { option, .. } => {
                    PreparedControlValue::Indexed { option: option.0 }
                }
                ControlValue::FixtureIndexed { entry, range_curve } => {
                    PreparedControlValue::FixtureIndexed {
                        entry: *entry,
                        range_curve: range_curve
                            .as_ref()
                            .map(|curve| curve.points.clone().into_boxed_slice()),
                    }
                }
                ControlValue::ConstantColor(color) => PreparedControlValue::ConstantColor(*color),
                ControlValue::Gradient(gradient) => {
                    PreparedControlValue::Gradient(gradient.stops.clone().into_boxed_slice())
                }
            },
            addresses,
        });
    }
    for (index, left) in prepared.iter().enumerate() {
        for (right_index, right) in prepared.iter().enumerate().skip(index + 1) {
            if controls_overlap(&clips[index], &clips[right_index])
                && control_function(&clips[index].target)
                    == control_function(&clips[right_index].target)
                && let Some(address) = left
                    .addresses
                    .iter()
                    .find(|address| right.addresses.contains(address))
            {
                return Err(SequenceOutputPrepareError::ControlConflict {
                    first: left.id,
                    second: right.id,
                    node: elements.layouts[address.element as usize].0,
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

pub(crate) fn prepare_fixture_behaviors(
    tree: &ElementTree,
    profiles: &FixtureProfileStore,
    elements: &OutputElements,
) -> Result<FixtureBehaviors, SequenceOutputPrepareError> {
    let mut rules = Vec::new();
    let mut bindings = Vec::new();
    let mut ranges = HashMap::<FixtureProfileId, core::ops::Range<u32>>::new();
    for (element, (id, _)) in elements.layouts.iter().enumerate() {
        let node = &tree.nodes[id];
        let ElementNodeKind::Fixture { profile } = &node.kind else {
            continue;
        };
        let element = u32::try_from(element).map_err(|_| {
            SequenceOutputPrepareError::InvalidPatch(
                "fixture element index exceeds u32".to_string(),
            )
        })?;
        if let Some(range) = ranges.get(profile) {
            if !range.is_empty() {
                bindings.push((element, range.clone()));
            }
            continue;
        }
        let profile_id = profile.clone();
        // Every previously appended block has already passed the checked end conversion.
        let start = rules.len() as u32;
        let profile = profiles.definitions.get(profile).ok_or_else(|| {
            SequenceOutputPrepareError::InvalidPatch("fixture profile is missing".to_string())
        })?;
        for (function, definition) in &profile.functions {
            if matches!(definition.kind, FixtureFunctionKind::ColorMixing { .. }) {
                rules.push((*function, FixtureBehavior::Color));
            }
        }
        for rule in &profile.behavior_rules {
            let (function, behavior) = match rule {
                FixtureBehaviorRule::Shutter {
                    function,
                    closed: off,
                    open: on,
                }
                | FixtureBehaviorRule::PrismGate {
                    function,
                    disabled: off,
                    enabled: on,
                } => (
                    *function,
                    FixtureBehavior::Switch {
                        off: FixtureControlValue::Indexed {
                            entry: *off,
                            range: 0.0,
                        },
                        on: FixtureControlValue::Indexed {
                            entry: *on,
                            range: 0.0,
                        },
                    },
                ),
                FixtureBehaviorRule::Dimmer { function, off, on } => (
                    *function,
                    FixtureBehavior::Switch {
                        off: FixtureControlValue::Normalized(*off),
                        on: FixtureControlValue::Normalized(*on),
                    },
                ),
                FixtureBehaviorRule::ColorWheel { function, entries } => (
                    *function,
                    FixtureBehavior::ColorWheel(
                        entries
                            .iter()
                            .map(|entry| (entry.color, entry.entry))
                            .collect(),
                    ),
                ),
            };
            rules.push((function, behavior));
        }
        let end = u32::try_from(rules.len()).map_err(|_| {
            SequenceOutputPrepareError::InvalidPatch(
                "fixture behavior table exceeds u32".to_string(),
            )
        })?;
        let range = start..end;
        if !range.is_empty() {
            bindings.push((element, range.clone()));
        }
        ranges.insert(profile_id, range);
    }
    Ok(FixtureBehaviors {
        bindings: bindings.into_boxed_slice(),
        rules: rules.into_boxed_slice(),
    })
}
