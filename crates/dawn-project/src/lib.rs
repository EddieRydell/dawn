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

pub mod analysis;
pub mod document;
pub mod effect_script;
pub mod fs;
pub mod load;
pub mod lower;
pub mod model;
pub mod path;
pub mod render;
