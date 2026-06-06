use dawn_language::{analysis::analyze_project_with_overlays, fs::WorkspaceFs};

use crate::{
    active_document, output,
    types::{
        ActiveGuiDocumentOutput, AnalysisTask, AnalysisTaskOutput, ExportFseqTask,
        ExportFseqTaskOutput,
    },
    BackendError, BackendErrorKind, BackendResult,
};

#[derive(Debug, Clone)]
pub enum BackendTask {
    AnalyzeProject(Box<AnalysisTask>),
    ExportFseq(Box<ExportFseqTask>),
}

#[derive(Debug, Clone)]
pub enum BackendTaskOutput {
    AnalyzeProject(Box<AnalysisTaskOutput>),
    ExportFseq(ExportFseqTaskOutput),
}

impl BackendTask {
    pub fn run(self) -> BackendResult<BackendTaskOutput> {
        match self {
            Self::AnalyzeProject(request) => run_analysis(*request),
            Self::ExportFseq(task) => run_export_fseq(*task),
        }
    }
}

//TODO move to correct files
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
    let analysis = analyze_project_with_overlays(
        &fs,
        request.project_file.clone(),
        None,
        request.overlays.clone(),
    );
    let active_gui_document = request.active_gui_document.map(|active| {
        let document = active_document::build_active_gui_document_from_analysis(
            &fs,
            active.cache_key.path.clone(),
            &active.descriptor,
            request.overlays,
            &analysis,
        );
        ActiveGuiDocumentOutput {
            cache_key: active.cache_key,
            document: Box::new(document),
        }
    });
    Ok(BackendTaskOutput::AnalyzeProject(Box::new(
        AnalysisTaskOutput {
            id: request.id,
            analysis,
            active_gui_document,
        },
    )))
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
