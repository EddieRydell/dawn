use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum BackendJob {
    AnalyzeProject {
        project_root: PathBuf,
        project_file: PathBuf,
    },
}
