use dawn_language::analysis::ProjectAnalysis;

use crate::types::{AnalysisTask, AnalysisTaskId, AnalysisTaskOutput};

#[derive(Debug, Default)]
pub(crate) struct Analysis {
    next_job_id: u64,
    latest_requested: Option<AnalysisTaskId>,
    latest_accepted: Option<ProjectAnalysis>,
}

impl Analysis {
    pub(crate) fn request(
        &mut self,
        project_root: camino::Utf8PathBuf,
        project_file: camino::Utf8PathBuf,
    ) -> AnalysisTask {
        let id = AnalysisTaskId(self.next_job_id);
        self.next_job_id = self.next_job_id.saturating_add(1);
        self.latest_requested = Some(id);
        AnalysisTask {
            id,
            project_root,
            project_file,
        }
    }

    pub(crate) fn accept(&mut self, output: AnalysisTaskOutput) {
        if self.latest_requested != Some(output.id) {
            return;
        }
        self.latest_accepted = Some(output.analysis);
    }

    pub(crate) fn snapshot(&self) -> Option<ProjectAnalysis> {
        self.latest_accepted.clone()
    }
}
