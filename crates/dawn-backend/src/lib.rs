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

mod active_document;
mod analysis;
mod app_backend;
mod audio;
mod document_editing;
mod editor;
mod effect_preview;
mod output;
mod preferences;
mod preview;
mod preview_render;
mod project;
mod render;
mod tasks;
mod types;
mod view;

pub use app_backend::{AppBackend, AppUpdate, BackendError, BackendErrorKind, BackendResult};
pub use editor::{
    EditorBufferView, EditorTabView, EditorView, LoadedEditorTabView, Revision,
    UnloadedEditorTabView,
};
pub use effect_preview::{EffectPreviewExecutor, EffectPreviewExecutorHandle, EffectPreviewKey};
pub use preview::{
    AudioPlaybackStatus, PreviewFrameDemand, PreviewRenderTiming, PreviewSnapshot, SequenceKey,
};
pub use preview_render::{
    PreviewFrameExecutor, PreviewFrameRenderOutput, PreviewFrameRenderTask, PreviewRenderMode,
};
pub use tasks::{BackendTask, BackendTaskOutput};
pub use types::{
    ActiveDocumentView, ActiveGuiDocument, ActiveGuiDocumentBlocked, AnalysisTask, AnalysisTaskId,
    AnalysisTaskOutput, EditorViewMode, EffectPreviewRequest, ExportFseqTask, ExportFseqTaskOutput,
    FileVersion, FseqExportMetadata, FseqExportOptions, FseqExportReport, PreviewAudioClock,
    PreviewHostState, PreviewTickOutput, RenderEffectPreviewRequestEffect, RenderTaskId,
    RenderView, RenderedFixtureFrame, RenderedFrame, RenderedFrameSource, RenderedFrameSourceKind,
    RenderedFrameStatus, RenderedPixelFrame, SequenceAudioDialog, SequenceEffectPreview,
    SequenceEffectPreviewErrorResult, SequenceEffectPreviewReadyResult,
    SequenceEffectPreviewResult, SequenceEffectPreviewResultBatch,
    SequenceEffectPreviewUnavailableResult, WorkspaceEntry, WorkspaceEntryKind,
};
pub use view::AppView;
