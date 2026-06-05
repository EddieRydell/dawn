use dawn_language::{analysis::analyze_project_with_overlays, fs::WorkspaceFs};

use crate::{
    types::{AnalysisJobRequest, AnalysisJobResult},
    BackendError, BackendErrorKind, BackendResult,
};

#[derive(Debug, Clone)]
pub enum BackendJob {
    AnalyzeProject(AnalysisJobRequest),
}

#[derive(Debug, Clone)]
pub enum BackendJobResult {
    AnalyzeProject(AnalysisJobResult),
}

impl BackendJob {
    pub fn run(self) -> BackendResult<BackendJobResult> {
        match self {
            Self::AnalyzeProject(request) => run_analysis(request),
        }
    }
}

fn run_analysis(request: AnalysisJobRequest) -> BackendResult<BackendJobResult> {
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
    Ok(BackendJobResult::AnalyzeProject(AnalysisJobResult {
        id: request.id,
        analysis,
    }))
}
