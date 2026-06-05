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
mod documents;
mod editor;
mod gui_edits;
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
pub use tasks::{BackendTask, BackendTaskOutput};
pub use types::{
    ActiveDocumentView, ActiveGuiDocument, ActiveGuiDocumentBlocked, AnalysisTask, AnalysisTaskId,
    AnalysisTaskOutput, EditorViewMode, ExportFseqTask, ExportFseqTaskOutput, FileVersion,
    FixtureGuiEdit, FseqExportMetadata, FseqExportOptions, FseqExportReport, LayoutGuiEdit,
    RenderEffectPreviewRequestEffect, RenderEffectPreviewTask, RenderEffectPreviewTaskOutput,
    RenderFrameTask, RenderFrameTaskOutput, RenderTaskId, RenderView, RenderedFixtureFrame,
    RenderedFrame, RenderedFrameSource, RenderedFrameSourceKind, RenderedFrameStatus,
    RenderedPixelFrame, SequenceEffectPreview, SequenceEffectPreviewErrorResult,
    SequenceEffectPreviewReadyResult, SequenceEffectPreviewResult,
    SequenceEffectPreviewUnavailableResult, SequenceGuiEdit, SequenceMarkRef, SequencePasteAnchor,
    SequenceResizeEdge, SequenceSelection, SequenceSelectionEdit, SequenceSelectionEditResult,
    WorkspaceEntry, WorkspaceEntryKind,
};
pub use view::AppView;
