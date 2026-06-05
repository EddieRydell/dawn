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

mod analysis;
mod app_backend;
mod audio;
mod editor;
mod filesystem;
mod jobs;
mod output;
mod preferences;
mod preview;
mod project;
mod render;
mod view;

pub use app_backend::{AppBackend, BackendError, BackendResult, BackendUpdate};
pub use jobs::BackendJob;
pub use view::AppView;
