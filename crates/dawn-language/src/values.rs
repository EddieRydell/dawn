use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecondsError {
    NotFinite,
    Negative,
}

pub const NANOS_PER_SECOND: u64 = 1_000_000_000;
pub const MICROS_PER_SECOND: u32 = 1_000_000;

/// The clock used by the portable renderer. One tick is one microsecond and all
/// arithmetic is 32-bit, matching the ESP32's native word size.
pub type SampleTime = fugit::TimerInstantU32<MICROS_PER_SECOND>;
pub type SampleDuration = fugit::TimerDurationU32<MICROS_PER_SECOND>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SampleTimeError {
    InvalidFrameRate,
    NotFinite,
    Negative,
    OutOfRange,
}

pub fn sample_time_from_frame(frame: u32, frame_rate: u32) -> Result<SampleTime, SampleTimeError> {
    if frame_rate == 0 {
        return Err(SampleTimeError::InvalidFrameRate);
    }
    let whole_ticks = (frame / frame_rate)
        .checked_mul(MICROS_PER_SECOND)
        .ok_or(SampleTimeError::OutOfRange)?;
    let partial_ticks = (frame % frame_rate)
        .checked_mul(MICROS_PER_SECOND)
        .ok_or(SampleTimeError::OutOfRange)?
        / frame_rate;
    Ok(SampleTime::from_ticks(
        whole_ticks
            .checked_add(partial_ticks)
            .ok_or(SampleTimeError::OutOfRange)?,
    ))
}

/// Converts a desktop/audio API value at the boundary of the portable runtime.
pub fn sample_time_from_seconds_f32(seconds: f32) -> Result<SampleTime, SampleTimeError> {
    if !seconds.is_finite() {
        return Err(SampleTimeError::NotFinite);
    }
    if seconds < 0.0 {
        return Err(SampleTimeError::Negative);
    }
    let micros = seconds * MICROS_PER_SECOND as f32;
    if micros > u32::MAX as f32 {
        return Err(SampleTimeError::OutOfRange);
    }
    Ok(SampleTime::from_ticks(micros.round() as u32))
}

pub fn sample_time_from_dawn_time(time: &DawnTime) -> Result<SampleTime, SampleTimeError> {
    Ok(SampleTime::from_ticks(
        u32::try_from(time.as_micros_rounded()).map_err(|_| SampleTimeError::OutOfRange)?,
    ))
}

pub fn sample_duration_from_dawn_duration(
    duration: &DawnDuration,
) -> Result<SampleDuration, SampleTimeError> {
    Ok(SampleDuration::from_ticks(
        u32::try_from(duration.as_micros_rounded()).map_err(|_| SampleTimeError::OutOfRange)?,
    ))
}

pub fn sample_time_seconds_f32(time: SampleTime) -> f32 {
    time.ticks() as f32 / MICROS_PER_SECOND as f32
}

pub fn sample_duration_seconds_f32(duration: SampleDuration) -> f32 {
    duration.ticks() as f32 / MICROS_PER_SECOND as f32
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DawnTime(pub Duration);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DawnDuration(pub Duration);

impl DawnTime {
    pub fn try_from_seconds_f32(seconds: f32) -> Result<Self, SecondsError> {
        if !seconds.is_finite() {
            return Err(SecondsError::NotFinite);
        }
        if seconds < 0.0 {
            return Err(SecondsError::Negative);
        }
        Ok(Self(Duration::from_secs_f32(seconds)))
    }

    pub fn from_seconds_f32(seconds: f32) -> Self {
        Self(Duration::from_secs_f32(seconds))
    }

    pub const fn from_nanos(nanos: u64) -> Self {
        Self(Duration::from_nanos(nanos))
    }

    pub const fn from_micros(micros: u64) -> Self {
        Self(Duration::from_micros(micros))
    }

    pub fn as_nanos(&self) -> u128 {
        self.0.as_nanos()
    }

    pub fn as_micros_rounded(&self) -> u128 {
        (self.as_nanos() + 500) / 1_000
    }

    pub fn as_seconds_f32(&self) -> f32 {
        self.0.as_secs_f32()
    }
}

impl DawnDuration {
    pub fn try_from_seconds_f32(seconds: f32) -> Result<Self, SecondsError> {
        if !seconds.is_finite() {
            return Err(SecondsError::NotFinite);
        }
        if seconds < 0.0 {
            return Err(SecondsError::Negative);
        }
        Ok(Self(Duration::from_secs_f32(seconds)))
    }

    pub fn from_seconds_f32(seconds: f32) -> Self {
        Self(Duration::from_secs_f32(seconds))
    }

    pub const fn from_nanos(nanos: u64) -> Self {
        Self(Duration::from_nanos(nanos))
    }

    pub const fn from_micros(micros: u64) -> Self {
        Self(Duration::from_micros(micros))
    }

    pub fn as_nanos(&self) -> u128 {
        self.0.as_nanos()
    }

    pub fn as_micros_rounded(&self) -> u128 {
        (self.as_nanos() + 500) / 1_000
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    pub fn as_seconds_f32(&self) -> f32 {
        self.0.as_secs_f32()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Distance {
    pub micrometers: i32,
}

impl Distance {
    pub const ZERO: Self = Self { micrometers: 0 };

    pub fn from_meters(value: f32) -> Self {
        Self {
            micrometers: (value * 1_000_000.0).round() as i32,
        }
    }

    pub fn as_meters_f32(self) -> f32 {
        self.micrometers as f32 / 1_000_000.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DistanceSpan {
    pub micrometers: u32,
}

impl DistanceSpan {
    pub const ZERO: Self = Self { micrometers: 0 };

    pub fn from_meters(value: f32) -> Self {
        Self {
            micrometers: (value * 1_000_000.0).round() as u32,
        }
    }

    pub fn as_meters_f32(self) -> f32 {
        self.micrometers as f32 / 1_000_000.0
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
    pub x: f32,
    pub y: f32,
    pub z: f32,
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
    pub x: f32,
    pub y: f32,
    pub z: f32,
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
    pub position: f32,
    pub value: f32,
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
            if point.position < previous {
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
    pub position: f32,
    pub color: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Marks {
    pub marks: Vec<DawnTime>,
}
