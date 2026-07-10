#![deny(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

pub mod dsl;
pub mod effect;
pub mod model;
pub mod operator;
pub mod sequence;
pub mod setup;
pub mod values;
