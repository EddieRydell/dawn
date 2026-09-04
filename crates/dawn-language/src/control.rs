use crate::element::{ElementSelection, IndexedOptionId};
use crate::fixture_profile::{FixtureEntryId, FixtureFunctionId};
use crate::values::{Color, Curve, DawnDuration, DawnTime, Gradient};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ControlClipId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct ControlClip {
    pub id: ControlClipId,
    pub start: DawnTime,
    pub duration: DawnDuration,
    pub target: ControlTarget,
    pub value: ControlValue,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ControlTarget {
    Scalar(ElementSelection),
    Indexed(ElementSelection),
    FixtureFunction {
        selection: ElementSelection,
        function: FixtureFunctionId,
    },
}

impl ControlTarget {
    pub fn selection(&self) -> &ElementSelection {
        match self {
            Self::Scalar(selection) | Self::Indexed(selection) => selection,
            Self::FixtureFunction { selection, .. } => selection,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ControlValue {
    ConstantNormalized(f32),
    NormalizedCurve(Curve),
    Indexed {
        option: IndexedOptionId,
        range_curve: Option<Curve>,
    },
    FixtureIndexed {
        entry: FixtureEntryId,
        range_curve: Option<Curve>,
    },
    ConstantColor(Color),
    Gradient(Gradient),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControlValidationError {
    EmptyDuration(ControlClipId),
    InvalidNormalizedValue(ControlClipId),
    InvalidCurve(ControlClipId),
    TypeMismatch(ControlClipId),
    Conflict {
        first: ControlClipId,
        second: ControlClipId,
    },
}

pub fn validate_control_clip(clip: &ControlClip) -> Result<(), ControlValidationError> {
    if clip.duration.0.is_zero() {
        return Err(ControlValidationError::EmptyDuration(clip.id));
    }
    match &clip.value {
        ControlValue::ConstantNormalized(value)
            if !value.is_finite() || !(0.0..=1.0).contains(value) =>
        {
            Err(ControlValidationError::InvalidNormalizedValue(clip.id))
        }
        ControlValue::NormalizedCurve(curve)
        | ControlValue::Indexed {
            range_curve: Some(curve),
            ..
        }
        | ControlValue::FixtureIndexed {
            range_curve: Some(curve),
            ..
        } if !valid_curve(curve) => Err(ControlValidationError::InvalidCurve(clip.id)),
        ControlValue::Gradient(gradient) if gradient.stops.is_empty() => {
            Err(ControlValidationError::InvalidCurve(clip.id))
        }
        _ => Ok(()),
    }
}

pub fn controls_overlap(left: &ControlClip, right: &ControlClip) -> bool {
    if left.target != right.target {
        return false;
    }
    let left_start = left.start.0;
    let right_start = right.start.0;
    let left_end = left_start.saturating_add(left.duration.0);
    let right_end = right_start.saturating_add(right.duration.0);
    left_start < right_end && right_start < left_end
}

fn valid_curve(curve: &Curve) -> bool {
    curve.validate().is_ok()
        && curve
            .points
            .iter()
            .all(|point| (0.0..=1.0).contains(&point.value))
}
