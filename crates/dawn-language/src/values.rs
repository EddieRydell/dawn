use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecondsError {
    NotFinite,
    Negative,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DawnTime(pub Duration);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DawnDuration(pub Duration);

impl DawnTime {
    pub fn try_from_seconds_f64(seconds: f64) -> Result<Self, SecondsError> {
        if !seconds.is_finite() {
            return Err(SecondsError::NotFinite);
        }
        if seconds < 0.0 {
            return Err(SecondsError::Negative);
        }
        Ok(Self(Duration::from_secs_f64(seconds)))
    }

    pub fn from_seconds_f64(seconds: f64) -> Self {
        Self(Duration::from_secs_f64(seconds))
    }

    pub fn as_seconds_f64(&self) -> f64 {
        self.0.as_secs_f64()
    }
}

impl DawnDuration {
    pub fn try_from_seconds_f64(seconds: f64) -> Result<Self, SecondsError> {
        if !seconds.is_finite() {
            return Err(SecondsError::NotFinite);
        }
        if seconds < 0.0 {
            return Err(SecondsError::Negative);
        }
        Ok(Self(Duration::from_secs_f64(seconds)))
    }

    pub fn from_seconds_f64(seconds: f64) -> Self {
        Self(Duration::from_secs_f64(seconds))
    }

    pub fn as_seconds_f64(&self) -> f64 {
        self.0.as_secs_f64()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Distance {
    pub micrometers: i64,
}

impl Distance {
    pub const ZERO: Self = Self { micrometers: 0 };

    pub fn from_meters(value: f64) -> Self {
        Self {
            micrometers: (value * 1_000_000.0).round() as i64,
        }
    }

    pub fn as_meters_f64(self) -> f64 {
        self.micrometers as f64 / 1_000_000.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DistanceSpan {
    pub micrometers: u64,
}

impl DistanceSpan {
    pub const ZERO: Self = Self { micrometers: 0 };

    pub fn from_meters(value: f64) -> Self {
        Self {
            micrometers: (value * 1_000_000.0).round() as u64,
        }
    }

    pub fn as_meters_f64(self) -> f64 {
        self.micrometers as f64 / 1_000_000.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point3 {
    pub x: Distance,
    pub y: Distance,
    pub z: Distance,
}

impl Default for Point3 {
    fn default() -> Self {
        Self {
            x: Distance::ZERO,
            y: Distance::ZERO,
            z: Distance::ZERO,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rotation3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Default for Rotation3 {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Default for Scale3 {
    fn default() -> Self {
        Self {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Color {
    pub fn from_hex(value: &str) -> Option<Self> {
        if value.len() != 7 || !value.starts_with('#') {
            return None;
        }
        Some(Self {
            red: u8::from_str_radix(&value[1..3], 16).ok()?,
            green: u8::from_str_radix(&value[3..5], 16).ok()?,
            blue: u8::from_str_radix(&value[5..7], 16).ok()?,
        })
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.red, self.green, self.blue)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub points: Vec<CurvePoint>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CurvePoint {
    pub position: f64,
    pub value: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CurveValidationError {
    Empty,
    NonFinitePoint,
    PositionOutOfRange,
    PositionsNotStrictlyIncreasing,
}

impl Curve {
    pub fn validate(&self) -> Result<(), CurveValidationError> {
        let Some(first) = self.points.first() else {
            return Err(CurveValidationError::Empty);
        };
        if !first.position.is_finite() || !first.value.is_finite() {
            return Err(CurveValidationError::NonFinitePoint);
        }
        if !(0.0..=1.0).contains(&first.position) {
            return Err(CurveValidationError::PositionOutOfRange);
        }
        let mut previous = first.position;
        for point in self.points.iter().skip(1) {
            if !point.position.is_finite() || !point.value.is_finite() {
                return Err(CurveValidationError::NonFinitePoint);
            }
            if !(0.0..=1.0).contains(&point.position) {
                return Err(CurveValidationError::PositionOutOfRange);
            }
            if point.position <= previous {
                return Err(CurveValidationError::PositionsNotStrictlyIncreasing);
            }
            previous = point.position;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub stops: Vec<GradientStop>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GradientStop {
    pub position: f64,
    pub color: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Marks {
    pub marks: Vec<DawnTime>,
}
