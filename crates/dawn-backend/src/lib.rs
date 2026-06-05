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
mod output;
mod preferences;
mod preview;
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
pub use preview::{
    AudioPlaybackStatus, PreviewRenderTiming, PreviewSnapshot, PreviewSyncMode, SequenceKey,
};
pub use tasks::{BackendTask, BackendTaskOutput};
pub use types::{
    ActiveDocumentView, ActiveGuiDocument, ActiveGuiDocumentBlocked, AnalysisTask, AnalysisTaskId,
    AnalysisTaskOutput, EditorViewMode, ExportFseqTask, ExportFseqTaskOutput, FileVersion,
    FseqExportMetadata, FseqExportOptions, FseqExportReport, PreviewAudioClock, PreviewHostState,
    PreviewTickOutput, RenderEffectPreviewRequestEffect, RenderEffectPreviewTask,
    RenderEffectPreviewTaskOutput, RenderFrameTask, RenderFrameTaskOutput, RenderTaskId,
    RenderView, RenderedFixtureFrame, RenderedFrame, RenderedFrameSource, RenderedFrameSourceKind,
    RenderedFrameStatus, RenderedPixelFrame, SequenceAudioDialog, SequenceEffectPreview,
    SequenceEffectPreviewErrorResult, SequenceEffectPreviewReadyResult,
    SequenceEffectPreviewResult, SequenceEffectPreviewResultBatch,
    SequenceEffectPreviewUnavailableResult, WorkspaceEntry, WorkspaceEntryKind,
};
pub use view::AppView;
