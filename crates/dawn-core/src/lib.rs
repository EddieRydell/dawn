#![no_std]
#![deny(unsafe_code)]

extern crate alloc;

pub mod automation;
pub mod dsl;
pub mod native_effect;
pub mod sampling;
pub mod values;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BuiltinEffect {
    Pulse,
    Chase,
    Spin,
    MarkPulse,
    MarkChase,
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
