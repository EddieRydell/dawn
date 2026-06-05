use dawn_language::analysis::ProjectAnalysis;

use crate::types::{AnalysisJobId, AnalysisJobRequest, AnalysisJobResult};

#[derive(Debug, Default)]
pub(crate) struct Analysis {
    next_job_id: u64,
    latest_requested: Option<AnalysisJobId>,
    latest_accepted: Option<ProjectAnalysis>,
}

impl Analysis {
    pub(crate) fn request(
        &mut self,
        project_root: camino::Utf8PathBuf,
        project_file: camino::Utf8PathBuf,
    ) -> AnalysisJobRequest {
        let id = AnalysisJobId(self.next_job_id);
        self.next_job_id = self.next_job_id.saturating_add(1);
        self.latest_requested = Some(id);
        AnalysisJobRequest {
            id,
            project_root,
            project_file,
        }
    }

    pub(crate) fn complete(&mut self, result: AnalysisJobResult) {
        if self.latest_requested != Some(result.id) {
            return;
        }
        self.latest_accepted = Some(result.analysis);
    }

    pub(crate) fn snapshot(&self) -> Option<ProjectAnalysis> {
        self.latest_accepted.clone()
    }
}
