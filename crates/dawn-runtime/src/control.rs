use crate::element::{ElementNodeId, RenderedElementState};
use crate::fixture::{FixtureBehaviors, FixtureControlValue, FixtureEntryId, FixtureFunctionId};
use crate::values::{Color, CurvePoint, GradientStop, SampleDuration, SampleTime};
use alloc::{boxed::Box, vec::Vec};

#[derive(Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedControl {
    pub id: u32,
    #[rkyv(with = crate::wire::Microseconds)]
    pub start: SampleTime,
    #[rkyv(with = crate::wire::Microseconds)]
    pub duration: SampleDuration,
    pub kind: PreparedControlKind,
    pub value: PreparedControlValue,
    pub addresses: Box<[PreparedControlAddress]>,
}

#[derive(Clone, Copy, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum PreparedControlKind {
    Scalar,
    Indexed,
    Fixture(FixtureFunctionId),
}

impl PreparedControl {
    pub fn explicit_fixture_count(&self) -> usize {
        if matches!(self.kind, PreparedControlKind::Fixture(_)) {
            self.addresses.len()
        } else {
            0
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PreparedControlAddress {
    pub element: u32,
    pub cell: u32,
}

#[derive(Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum PreparedControlValue {
    ConstantNormalized(f32),
    NormalizedCurve(Box<[CurvePoint]>),
    Indexed {
        option: u32,
    },
    FixtureIndexed {
        entry: FixtureEntryId,
        range_curve: Option<Box<[CurvePoint]>>,
    },
    ConstantColor(Color),
    Gradient(Box<[GradientStop]>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ControlErrorKind {
    MissingTarget,
    CellOutOfRange,
    TypeMismatch,
    FixtureTypeMismatch,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ControlError {
    Control { clip: u32, reason: ControlErrorKind },
    UnsupportedFixtureColor { node: ElementNodeId, color: Color },
}

pub fn apply_controls(
    elements: &mut [RenderedElementState],
    controls: &[PreparedControl],
    sample_time: SampleTime,
    explicit: &mut Vec<(u32, FixtureFunctionId)>,
) -> Result<(), ControlError> {
    explicit.clear();
    for prepared in controls {
        let Some(elapsed) = sample_time.checked_duration_since(prepared.start) else {
            continue;
        };
        if elapsed >= prepared.duration {
            continue;
        }
        let position = elapsed.ticks() as f32 / prepared.duration.ticks() as f32;
        // Every selected cell receives the same sampled control value.
        let scalar = match (prepared.kind, &prepared.value) {
            (PreparedControlKind::Scalar, PreparedControlValue::NormalizedCurve(curve)) => {
                sample_curve(curve, position)
            }
            _ => 0.0,
        };
        let fixture = if matches!(prepared.kind, PreparedControlKind::Fixture(_)) {
            fixture_control_value(&prepared.value, position)
        } else {
            None
        };
        for address in &prepared.addresses {
            let state =
                elements
                    .get_mut(address.element as usize)
                    .ok_or(ControlError::Control {
                        clip: prepared.id,
                        reason: ControlErrorKind::MissingTarget,
                    })?;
            match (prepared.kind, &prepared.value, state) {
                (
                    PreparedControlKind::Scalar,
                    PreparedControlValue::ConstantNormalized(value),
                    RenderedElementState::Scalar { cells, .. },
                ) => set_cell(cells, address.cell, *value, prepared.id)?,
                (
                    PreparedControlKind::Scalar,
                    PreparedControlValue::NormalizedCurve(_),
                    RenderedElementState::Scalar { cells, .. },
                ) => set_cell(cells, address.cell, scalar, prepared.id)?,
                (
                    PreparedControlKind::Indexed,
                    PreparedControlValue::Indexed { option, .. },
                    RenderedElementState::Indexed { cells, .. },
                ) => set_cell(cells, address.cell, *option, prepared.id)?,
                (
                    PreparedControlKind::Fixture(function),
                    _,
                    RenderedElementState::Fixture { state, .. },
                ) => {
                    let value = fixture.clone().ok_or(ControlError::Control {
                        clip: prepared.id,
                        reason: ControlErrorKind::FixtureTypeMismatch,
                    })?;
                    state.insert(function, value);
                    explicit.push((address.element, function));
                }
                _ => {
                    return Err(ControlError::Control {
                        clip: prepared.id,
                        reason: ControlErrorKind::TypeMismatch,
                    });
                }
            }
        }
    }
    Ok(())
}

fn fixture_control_value(
    value: &PreparedControlValue,
    position: f32,
) -> Option<FixtureControlValue> {
    match value {
        PreparedControlValue::ConstantNormalized(value) => {
            Some(FixtureControlValue::Normalized(*value))
        }
        PreparedControlValue::NormalizedCurve(curve) => Some(FixtureControlValue::Normalized(
            sample_curve(curve, position),
        )),
        PreparedControlValue::FixtureIndexed { entry, range_curve } => {
            Some(FixtureControlValue::Indexed {
                entry: *entry,
                range: range_curve
                    .as_deref()
                    .map_or(0.0, |curve| sample_curve(curve, position)),
            })
        }
        PreparedControlValue::ConstantColor(color) => Some(FixtureControlValue::Color(*color)),
        PreparedControlValue::Gradient(gradient) => {
            sample_gradient(gradient, position).map(FixtureControlValue::Color)
        }
        PreparedControlValue::Indexed { .. } => None,
    }
}

pub fn apply_fixture_behavior_rules(
    elements: &mut [RenderedElementState],
    behaviors: &FixtureBehaviors,
    explicit: &[(u32, FixtureFunctionId)],
) -> Result<(), ControlError> {
    for (element, range) in &behaviors.bindings {
        let RenderedElementState::Fixture {
            node, color, state, ..
        } = &mut elements[*element as usize]
        else {
            unreachable!("prepared fixture behavior targets a fixture");
        };
        for (function, behavior) in &behaviors.rules[range.start as usize..range.end as usize] {
            if explicit.contains(&(*element, *function)) {
                continue;
            }
            let value = behavior
                .sample(*color)
                .ok_or(ControlError::UnsupportedFixtureColor {
                    node: *node,
                    color: *color,
                })?;
            state.insert(*function, value);
        }
    }
    Ok(())
}

fn set_cell<T: Copy>(cells: &mut [T], cell: u32, value: T, clip: u32) -> Result<(), ControlError> {
    let target = cells.get_mut(cell as usize).ok_or(ControlError::Control {
        clip,
        reason: ControlErrorKind::CellOutOfRange,
    })?;
    *target = value;
    Ok(())
}
fn sample_curve(curve: &[CurvePoint], position: f32) -> f32 {
    let Some(first) = curve.first() else {
        return 0.0;
    };
    if position <= first.position {
        return first.value;
    }
    for pair in curve.windows(2) {
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
    curve.last().map_or(0.0, |point| point.value)
}
fn sample_gradient(gradient: &[GradientStop], position: f32) -> Option<Color> {
    let first = gradient.first()?;
    if position <= first.position {
        return Some(first.color);
    }
    for pair in gradient.windows(2) {
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
    gradient.last().map(|stop| stop.color)
}
fn lerp_u8(left: u8, right: u8, amount: f32) -> u8 {
    libm::roundf(f32::from(left) + (f32::from(right) - f32::from(left)) * amount.clamp(0.0, 1.0))
        as u8
}
