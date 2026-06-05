use dawn_language::{analysis::analyze_project_with_overlays, fs::WorkspaceFs};

use crate::{
    output, render,
    types::{
        AnalysisTask, AnalysisTaskOutput, ExportFseqTask, ExportFseqTaskOutput,
        RenderEffectPreviewTask, RenderEffectPreviewTaskOutput, RenderFrameTask,
        RenderFrameTaskOutput,
    },
    BackendError, BackendErrorKind, BackendResult,
};

#[derive(Debug, Clone)]
pub enum BackendTask {
    AnalyzeProject(AnalysisTask),
    RenderFrame(RenderFrameTask),
    RenderEffectPreviews(RenderEffectPreviewTask),
    ExportFseq(ExportFseqTask),
}

#[derive(Debug, Clone)]
pub enum BackendTaskOutput {
    AnalyzeProject(AnalysisTaskOutput),
    RenderFrame(RenderFrameTaskOutput),
    RenderEffectPreviews(RenderEffectPreviewTaskOutput),
    ExportFseq(ExportFseqTaskOutput),
}

impl BackendTask {
    pub fn run(self) -> BackendResult<BackendTaskOutput> {
        match self {
            Self::AnalyzeProject(request) => run_analysis(request),
            Self::RenderFrame(task) => Ok(BackendTaskOutput::RenderFrame(
                render::execute_render_frame(task),
            )),
            Self::RenderEffectPreviews(task) => Ok(BackendTaskOutput::RenderEffectPreviews(
                render::execute_effect_previews(task),
            )),
            Self::ExportFseq(task) => run_export_fseq(task),
        }
    }
}

fn run_analysis(request: AnalysisTask) -> BackendResult<BackendTaskOutput> {
    let fs = WorkspaceFs::open(request.project_root.as_std_path()).map_err(|error| {
        BackendError::new(
            BackendErrorKind::Io,
            format!(
                "failed to open project root '{}': {error}",
                request.project_root
            ),
        )
    })?;
    let analysis =
        analyze_project_with_overlays(&fs, request.project_file.clone(), None, Vec::new());
    Ok(BackendTaskOutput::AnalyzeProject(AnalysisTaskOutput {
        id: request.id,
        analysis,
    }))
}

fn run_export_fseq(mut task: ExportFseqTask) -> BackendResult<BackendTaskOutput> {
    let report = output::export_fseq_file_with_cache(
        &task.analysis,
        &task.document,
        task.output_path.as_std_path(),
        task.options,
        &mut task.cache,
    )?;
    Ok(BackendTaskOutput::ExportFseq(ExportFseqTaskOutput {
        id: task.id,
        report,
        cache: task.cache,
    }))
}
