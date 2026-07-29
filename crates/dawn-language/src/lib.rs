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

pub mod control;
pub mod controller;
pub mod dsl;
pub mod effect;
pub mod element;
pub mod fixture_profile;
pub mod identity;
pub mod model;
pub mod native_effect;
pub mod operator;
pub mod operator_rewrite;
pub mod patch;
pub mod preview;
pub mod sampling;
pub mod sequence;
pub mod setup;
pub mod source_remap;
pub mod validation;
pub mod values;
