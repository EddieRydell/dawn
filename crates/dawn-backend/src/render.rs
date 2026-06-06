use dawn_language::{analysis::ProjectAnalysis, sequence_render::SequenceRenderCache};

use crate::{
    types::{ExportFseqTask, FseqExportOptions, RenderTaskId, RenderView},
    BackendError, BackendErrorKind, BackendResult,
};

#[derive(Debug, Default)]
pub(crate) struct Renderer {
    next_task_id: u64,
    latest_export_task: Option<RenderTaskId>,
    cache: SequenceRenderCache,
    snapshot: RenderView,
}

impl Renderer {
    pub(crate) fn request_export_fseq(
        &mut self,
        analysis: ProjectAnalysis,
        document: dawn_language::document::SequenceDocument,
        output_path: camino::Utf8PathBuf,
        options: FseqExportOptions,
    ) -> ExportFseqTask {
        let id = self.next_id();
        self.latest_export_task = Some(id);
        ExportFseqTask {
            id,
            analysis,
            document,
            output_path,
            options,
            cache: self.cache.clone(),
        }
    }

    pub(crate) fn accept_export(&mut self, output: crate::types::ExportFseqTaskOutput) {
        if self.latest_export_task != Some(output.id) {
            return;
        }
        self.cache = output.cache;
        self.snapshot.export_report = Some(output.report);
    }

    pub(crate) fn snapshot(&self) -> RenderView {
        self.snapshot.clone()
    }

    fn next_id(&mut self) -> RenderTaskId {
        let id = RenderTaskId(self.next_task_id);
        self.next_task_id = self.next_task_id.saturating_add(1);
        id
    }
}

pub(crate) fn require_analysis(
    analysis: Option<ProjectAnalysis>,
) -> BackendResult<ProjectAnalysis> {
    analysis.ok_or_else(|| {
        BackendError::new(
            BackendErrorKind::InvalidInput,
            "project analysis is not available",
        )
    })
}
