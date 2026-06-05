mod analysis;
mod filesystem;
mod live_output;
mod render;

use analysis::AnalysisWorker;
use filesystem::FilesystemWorker;
use live_output::LiveOutputWorker;
use render::RenderWorker;
pub(super) use render::RenderWorkerResult;

pub use render::{
    SequenceEffectPreview, SequenceEffectPreviewErrorResult, SequenceEffectPreviewReadyResult,
    SequenceEffectPreviewRequestEffect, SequenceEffectPreviewResult,
    SequenceEffectPreviewUnavailableResult,
};

#[derive(Debug, Default)]
pub(super) struct AsyncWorkers {
    analysis: AnalysisWorker,
    filesystem: FilesystemWorker,
    render: RenderWorker,
    live_output: LiveOutputWorker,
}

impl AsyncWorkers {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn sync_filesystem_root(
        &mut self,
        root: Option<std::path::PathBuf>,
    ) -> Result<(), String> {
        self.filesystem.sync_project_root(root)
    }

    pub(super) fn drain(&mut self) -> Vec<WorkerResult> {
        let mut results = Vec::new();
        for paths in self.filesystem.drain() {
            results.push(WorkerResult::Filesystem { paths });
        }
        for result in self.render.drain() {
            results.push(WorkerResult::Render(result));
        }
        results
    }

    pub(super) fn request_preview_frame(
        &self,
        analysis: Option<dawn_language::analysis::ProjectAnalysis>,
        request: crate::preview::session::PreviewRenderRequest,
    ) -> Result<(), String> {
        self.render.request_preview_frame(analysis, request)
    }

    pub(super) fn request_effect_previews(
        &self,
        path: String,
        object_key: String,
        request_id: u32,
        effects: Vec<SequenceEffectPreviewRequestEffect>,
        analysis: dawn_language::analysis::ProjectAnalysis,
        document: dawn_language::document::SequenceDocument,
    ) -> Result<(), String> {
        self.render
            .request_effect_previews(path, object_key, request_id, effects, analysis, document)
    }

    pub(super) fn take_effect_preview_results(
        &self,
        path: String,
        object_key: String,
    ) -> Result<Vec<SequenceEffectPreviewResult>, String> {
        self.render.take_effect_preview_results(path, object_key)
    }
}

#[derive(Debug)]
pub(super) enum WorkerResult {
    Analysis,
    Filesystem {
        paths: Vec<dawn_language::path::Utf8PathBuf>,
    },
    Render(RenderWorkerResult),
    LiveOutput,
}
