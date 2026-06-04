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

pub mod app_model;
pub mod contracts;
pub mod controller_output;
pub mod coordinator;
pub mod dto;
pub mod fseq_export;
pub mod layout_persistence;
pub mod logging;
pub mod output_runtime;
pub mod preview_session;
pub mod read_model;
pub mod runtime;
pub mod services;
pub mod workspace;
