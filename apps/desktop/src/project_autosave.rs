use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use dawn_app_core::app_model::{
    ProjectAutosaveCompletion, ProjectAutosaveJob, ProjectAutosaveSavedFile,
};

pub(crate) struct ProjectAutosaveRuntime {
    jobs: Sender<ProjectAutosaveJob>,
    completions: Receiver<ProjectAutosaveCompletion>,
}

impl Default for ProjectAutosaveRuntime {
    fn default() -> Self {
        let (jobs, job_receiver) = mpsc::channel();
        let (completions, completion_receiver) = mpsc::channel();
        start_worker(job_receiver, completions);
        Self {
            jobs,
            completions: completion_receiver,
        }
    }
}

impl ProjectAutosaveRuntime {
    pub(crate) fn request(&self, job: ProjectAutosaveJob) -> Result<(), String> {
        self.jobs
            .send(job)
            .map_err(|_| "project autosave worker is not available".to_string())
    }

    pub(crate) fn try_complete(&self) -> Result<Option<ProjectAutosaveCompletion>, String> {
        match self.completions.try_recv() {
            Ok(completion) => Ok(Some(completion)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                Err("project autosave worker is not available".to_string())
            }
        }
    }

    pub(crate) fn complete(&self) -> Result<ProjectAutosaveCompletion, String> {
        self.completions
            .recv()
            .map_err(|_| "project autosave worker is not available".to_string())
    }
}

fn start_worker(
    receiver: Receiver<ProjectAutosaveJob>,
    completions: Sender<ProjectAutosaveCompletion>,
) {
    thread::spawn(move || loop {
        let Ok(mut job) = receiver.recv() else {
            break;
        };
        while let Ok(candidate) = receiver.try_recv() {
            if candidate.revision > job.revision {
                job = candidate;
            }
        }
        if completions.send(save_job(job)).is_err() {
            break;
        }
    });
}

fn save_job(job: ProjectAutosaveJob) -> ProjectAutosaveCompletion {
    let result = job.workspace.save_project(&job.project);
    let saved_file_versions = match &result {
        Ok(save_result) if save_result.diagnostics.is_empty() => save_result
            .written_files
            .iter()
            .map(|path| {
                let text = job.workspace.read_file(path.clone())?;
                let disk_version = job
                    .workspace
                    .file_version(path, &text)?
                    .ok_or_else(|| format!("saved file `{path}` does not exist"))?;
                Ok(ProjectAutosaveSavedFile {
                    path: path.clone(),
                    text,
                    disk_version,
                })
            })
            .collect::<Result<Vec<_>, String>>(),
        _ => Ok(Vec::new()),
    };
    match saved_file_versions {
        Ok(saved_file_versions) => ProjectAutosaveCompletion {
            revision: job.revision,
            result,
            saved_file_versions,
        },
        Err(error) => ProjectAutosaveCompletion {
            revision: job.revision,
            result: Err(error),
            saved_file_versions: Vec::new(),
        },
    }
}
