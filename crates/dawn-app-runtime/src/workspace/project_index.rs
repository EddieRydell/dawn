use std::sync::Arc;

use dawn_language::analysis::{analyze_project_with_overlays, ProjectAnalysis, ProjectOverlay};
use dawn_language::fs::WorkspaceFs;
use dawn_language::path::Utf8PathBuf;

use crate::runtime::contracts::{Event, Revision, RuntimeResult};

#[derive(Debug, Clone)]
pub enum ProjectIndexCommand {
    Analyze {
        fs: WorkspaceFs,
        project_file: Utf8PathBuf,
        overlays: Vec<ProjectOverlay>,
        source_revision: Revision,
    },
}

#[derive(Debug, Default, Clone)]
pub struct ProjectIndexCore {
    latest: Option<Arc<ProjectAnalysis>>,
    revision: Revision,
}

impl ProjectIndexCore {
    pub fn latest(&self) -> Option<Arc<ProjectAnalysis>> {
        self.latest.clone()
    }

    pub fn handle(&mut self, command: ProjectIndexCommand) -> RuntimeResult<Vec<Event>> {
        match command {
            ProjectIndexCommand::Analyze {
                fs,
                project_file,
                overlays,
                source_revision,
            } => {
                let analysis = analyze_project_with_overlays(&fs, project_file, None, overlays);
                let diagnostic_count = analysis.diagnostics.len();
                self.latest = Some(Arc::new(analysis));
                self.revision = source_revision;
                Ok(vec![Event::AnalysisUpdated {
                    revision: self.revision,
                    diagnostic_count,
                }])
            }
        }
    }
}
