use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DawnTime(pub Duration);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DawnDuration(pub Duration);

impl DawnTime {
    pub fn as_seconds_f64(&self) -> f64 {
        self.0.as_secs_f64()
    }
}

impl DawnDuration {
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DistanceSpan {
    pub micrometers: u64,
}

impl DistanceSpan {
    pub const ZERO: Self = Self { micrometers: 0 };
}

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

#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub points: Vec<CurvePoint>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CurvePoint {
    pub position: f64,
    pub value: CurveValue,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CurveValue {
    Float(f64),
    Color(Color),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Marks {
    pub marks: Vec<DawnTime>,
}
