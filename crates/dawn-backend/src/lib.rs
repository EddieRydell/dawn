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
mod jobs;
mod output;
mod preferences;
mod preview;
mod project;
mod render;
mod types;
mod view;

pub use app_backend::{AppBackend, BackendError, BackendErrorKind, BackendResult, BackendUpdate};
pub use editor::{
    EditorBufferView, EditorTabView, EditorView, LoadedEditorTabView, Revision,
    UnloadedEditorTabView,
};
pub use jobs::BackendJob;
pub use types::{EditorViewMode, FileVersion};
pub use view::AppView;
