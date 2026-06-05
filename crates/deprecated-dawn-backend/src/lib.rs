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

mod audio_runtime;
mod editor;
mod gui_edits;
mod logging;
mod output;
mod prefs;
mod preview;
mod runtime;
mod workspace;

pub use crate::editor::{BufferExternalState, BufferTab, EditorViewMode};
pub use crate::gui_edits::types::{
    FixtureGuiEdit, LayoutGuiEdit, SequenceGuiEdit, SequenceMarkRef, SequencePasteAnchor,
    SequenceResizeEdge, SequenceSelection, SequenceSelectionEdit, SequenceSelectionEditResult,
};
pub use crate::output::live_output::LiveOutputReadout;
pub use crate::prefs::WindowLayout;
pub use crate::preview::session::{
    AudioPlaybackStatus, PreviewRenderTiming, PreviewSnapshot, SequenceKey,
};
pub use crate::runtime::app_backend::{
    AppBackend, BackendError, BackendOutput, PreviewHostState, PreviewTickOutput,
    SequenceAudioDialog, SequenceEffectPreviewKey, SequenceEffectPreviewRequest,
};
pub use crate::runtime::app_view::AppView;
pub use crate::runtime::contracts::{RuntimeActivity, RuntimeNotice, RuntimeStatus};
pub use crate::runtime::rendered_frame::{
    RenderedFixtureFrame, RenderedFrame, RenderedFrameSource, RenderedFrameSourceKind,
    RenderedFrameStatus, RenderedPixelFrame,
};
pub use crate::runtime::workers::{
    SequenceEffectPreview, SequenceEffectPreviewErrorResult, SequenceEffectPreviewReadyResult,
    SequenceEffectPreviewRequestEffect, SequenceEffectPreviewResult,
    SequenceEffectPreviewUnavailableResult,
};
pub use crate::workspace::ActiveGuiDocument;
