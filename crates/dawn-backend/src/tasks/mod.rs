use dawn_language::{analysis::analyze_project_with_overlays, fs::WorkspaceFs};

use crate::{
    types::{AnalysisTask, AnalysisTaskOutput},
    BackendError, BackendErrorKind, BackendResult,
};

#[derive(Debug, Clone)]
pub enum BackendTask {
    AnalyzeProject(AnalysisTask),
}

#[derive(Debug, Clone)]
pub enum BackendTaskOutput {
    AnalyzeProject(AnalysisTaskOutput),
}

impl BackendTask {
    pub fn run(self) -> BackendResult<BackendTaskOutput> {
        match self {
            Self::AnalyzeProject(request) => run_analysis(request),
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
