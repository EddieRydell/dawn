#![deny(clippy::disallowed_methods)]
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

pub mod actions;
pub mod app_model;
pub mod dto;
pub mod editor_session;
pub mod layout_persistence;
pub mod output_runtime;
pub mod preview_session;
pub mod workspace;
