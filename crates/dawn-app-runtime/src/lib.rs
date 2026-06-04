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

pub mod app_shell;
pub mod dto;
pub mod editor;
pub mod gui_edits;
pub mod logging;
pub mod output;
pub mod preview;
pub mod runtime;
pub mod workspace;
