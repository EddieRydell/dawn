use dawn_language::analysis::{ProjectAnalysis, ProjectOverlay};

use crate::types::{ActiveGuiDocumentRequest, AnalysisTask, AnalysisTaskId, AnalysisTaskOutput};

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
        overlays: Vec<ProjectOverlay>,
        active_gui_document: Option<ActiveGuiDocumentRequest>,
    ) -> AnalysisTask {
        let id = AnalysisTaskId(self.next_job_id);
        self.next_job_id = self.next_job_id.saturating_add(1);
        self.latest_requested = Some(id);
        AnalysisTask {
            id,
            project_root,
            project_file,
            overlays,
            active_gui_document,
        }
    }

    pub(crate) fn accept(&mut self, output: &AnalysisTaskOutput) -> bool {
        if self.latest_requested != Some(output.id) {
            return false;
        }
        self.latest_accepted = Some(output.analysis.clone());
        true
    }

    pub(crate) fn snapshot(&self) -> Option<ProjectAnalysis> {
        self.latest_accepted.clone()
    }
}
