use crate::{preferences::ProjectSessionPreferences, project::Project, BackendResult};

#[derive(Debug, Default)]
pub(crate) struct Editor;

impl Editor {
    pub(crate) fn restore_for_project(
        &mut self,
        _project: &Project,
        preferences: ProjectSessionPreferences,
    ) -> BackendResult<()> {
        let _ = (preferences.open_files, preferences.active_file);

        Ok(())
    }
}
