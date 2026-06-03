use std::thread;
use std::time::{Duration, Instant};

use dawn_app_runtime::contracts::Revision;
use dawn_app_runtime::coordinator::AppCoordinator;
use dawn_app_runtime::read_model::AppReadModels;
use dawn_app_runtime::services::document_store::DocumentStoreCommand;
use dawn_project::path::Utf8PathBuf;

const RUNTIME_DOCUMENT_TIMEOUT: Duration = Duration::from_millis(500);
const RUNTIME_DOCUMENT_POLL_INTERVAL: Duration = Duration::from_millis(5);

pub(crate) struct ActiveRuntimeBuffer {
    pub(crate) project_root: Option<String>,
    pub(crate) path: Utf8PathBuf,
    pub(crate) text: String,
    pub(crate) conflicted: bool,
}

#[derive(Default)]
pub(crate) struct RuntimeHost {
    coordinator: AppCoordinator,
}

impl RuntimeHost {
    pub(crate) fn read_models(&self) -> &AppReadModels {
        self.coordinator.read_models()
    }

    pub(crate) fn drain_events(&mut self) -> Result<usize, String> {
        self.coordinator
            .drain_events()
            .map_err(|error| error.to_string())
    }

    pub(crate) fn update_active_text(
        &mut self,
        active_buffer: ActiveRuntimeBuffer,
        text: String,
    ) -> Result<(), String> {
        if active_buffer.conflicted {
            return Err("active document has external disk changes".to_string());
        }

        self.ensure_project_open(active_buffer.project_root.as_deref())?;
        self.ensure_buffer_open(&active_buffer)?;

        let revision = self
            .coordinator
            .read_models()
            .editor
            .buffers
            .get(&active_buffer.path)
            .map(|buffer| buffer.revision)
            .ok_or_else(|| format!("runtime buffer is not open: {}", active_buffer.path))?;
        let expected_revision = revision.next();
        self.coordinator
            .submit_document_store(DocumentStoreCommand::UpdateBufferText {
                path: active_buffer.path.clone(),
                expected_revision: revision,
                text,
            })
            .map_err(|error| error.to_string())?;
        self.drain_until_buffer_revision(&active_buffer.path, expected_revision)
    }

    pub(crate) fn open_project(&mut self, root: String) -> Result<(), String> {
        self.coordinator
            .submit_document_store(DocumentStoreCommand::OpenProject { root: root.clone() })
            .map_err(|error| error.to_string())?;
        self.drain_until_project_root(&root)
    }

    pub(crate) fn open_buffer(&mut self, path: Utf8PathBuf, text: String) -> Result<(), String> {
        self.coordinator
            .submit_document_store(DocumentStoreCommand::OpenBuffer {
                path: path.clone(),
                text,
                disk_revision: Revision::INITIAL,
            })
            .map_err(|error| error.to_string())?;
        self.drain_until_buffer_open(&path)
    }

    fn ensure_project_open(&mut self, project_root: Option<&str>) -> Result<(), String> {
        if self
            .coordinator
            .read_models()
            .workspace
            .project_root
            .as_deref()
            == project_root
        {
            return Ok(());
        }
        let Some(project_root) = project_root else {
            return Ok(());
        };
        self.open_project(project_root.to_string())
    }

    fn ensure_buffer_open(&mut self, active_buffer: &ActiveRuntimeBuffer) -> Result<(), String> {
        if self
            .coordinator
            .read_models()
            .editor
            .buffers
            .contains_key(&active_buffer.path)
        {
            return Ok(());
        }
        self.open_buffer(active_buffer.path.clone(), active_buffer.text.clone())
    }

    fn drain_until_project_root(&mut self, project_root: &str) -> Result<(), String> {
        self.drain_until(|host| {
            host.coordinator
                .read_models()
                .workspace
                .project_root
                .as_deref()
                == Some(project_root)
        })
    }

    fn drain_until_buffer_open(&mut self, path: &Utf8PathBuf) -> Result<(), String> {
        self.drain_until(|host| {
            host.coordinator
                .read_models()
                .editor
                .buffers
                .contains_key(path)
        })
    }

    fn drain_until_buffer_revision(
        &mut self,
        path: &Utf8PathBuf,
        revision: Revision,
    ) -> Result<(), String> {
        self.drain_until(|host| {
            host.coordinator
                .read_models()
                .editor
                .buffers
                .get(path)
                .is_some_and(|buffer| buffer.revision == revision)
        })
    }

    fn drain_until(&mut self, done: impl Fn(&Self) -> bool) -> Result<(), String> {
        let deadline = Instant::now() + RUNTIME_DOCUMENT_TIMEOUT;
        loop {
            self.drain_events()?;
            if let Some(error) = self.coordinator.read_models().status.fatal_error.as_ref() {
                return Err(error.clone());
            }
            if done(self) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("runtime timed out waiting for document store update".to_string());
            }
            thread::sleep(RUNTIME_DOCUMENT_POLL_INTERVAL);
        }
    }
}

impl Drop for RuntimeHost {
    fn drop(&mut self) {
        let _ = self.coordinator.shutdown();
    }
}
