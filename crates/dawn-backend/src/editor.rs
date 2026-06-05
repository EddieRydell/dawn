use crate::{project::Project, BackendResult};

#[derive(Debug, Default)]
pub(crate) struct Editor;

impl Editor {
    pub(crate) fn restore_for_project(&mut self, _project: &Project) -> BackendResult<()> {
        Ok(())
    }
}
