#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

pub mod automation;
pub mod dsl;
mod evaluation;
pub mod native_effect;
pub mod sampling;
pub mod sequence;
pub mod values;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BuiltinEffect {
    Pulse,
    Chase,
    Spin,
    MarkPulse,
    MarkChase,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BuiltinOperator {
    Max,
    Add,
    Multiply,
    IntensityModulate,
    Dim,
    Invert,
    Colorize,
    Delay,
    Echo,
}

impl BuiltinOperator {
    pub const ALL: [Self; 9] = [
        Self::Max,
        Self::Add,
        Self::Multiply,
        Self::IntensityModulate,
        Self::Dim,
        Self::Invert,
        Self::Colorize,
        Self::Delay,
        Self::Echo,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::Max => 0,
            Self::Add => 1,
            Self::Multiply => 2,
            Self::IntensityModulate => 3,
            Self::Dim => 4,
            Self::Invert => 5,
            Self::Colorize => 6,
            Self::Delay => 7,
            Self::Echo => 8,
        }
    }

    pub fn from_source_name(name: &str) -> Option<Self> {
        match name {
            "max" => Some(Self::Max),
            "add" => Some(Self::Add),
            "multiply" => Some(Self::Multiply),
            "intensity_modulate" => Some(Self::IntensityModulate),
            "dim" => Some(Self::Dim),
            "invert" => Some(Self::Invert),
            "colorize" => Some(Self::Colorize),
            "delay" => Some(Self::Delay),
            "echo" => Some(Self::Echo),
            _ => None,
        }
    }
}

impl BuiltinEffect {
    pub const ALL: [Self; 5] = [
        Self::Pulse,
        Self::Chase,
        Self::Spin,
        Self::MarkPulse,
        Self::MarkChase,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::Pulse => 0,
            Self::Chase => 1,
            Self::Spin => 2,
            Self::MarkPulse => 3,
            Self::MarkChase => 4,
        }
    }
}
