use std::sync::Arc;

use dawn_project_io::ProjectSession;

use super::{DesktopState, lock_unpoisoned};
use crate::state_tasks::{RenderRefreshPayload, RenderRefreshResult};

impl DesktopState {
    pub(super) fn refresh_render_session(
        &self,
        project: &dawn_language::model::DawnProject,
    ) -> Option<dawn_runtime::SequenceOutputPrepareError> {
        let mut sequence_render = lock_unpoisoned(&self.sequence_render);
        let result = sequence_render.refresh_project(project);
        if result.is_err() {
            sequence_render.unload();
        }
        result.err()
    }

    pub(super) fn schedule_render_refresh(&self, project: Arc<ProjectSession>) {
        let target = lock_unpoisoned(&self.sequence_render).active_target();
        let Some((setup_id, sequence_id)) = target else {
            return;
        };
        if !project.project.sequences.contains_key(&sequence_id) {
            self.unload_render_session();
            return;
        }
        let request = RenderRefreshPayload {
            project,
            setup_id,
            sequence_id,
        };
        self.enqueue_render_refresh(request);
    }

    pub(super) fn schedule_sequence_render_prepare(
        &self,
        project: Arc<ProjectSession>,
        sequence_id: dawn_language::sequence::SequenceId,
    ) {
        if !project.project.sequences.contains_key(&sequence_id) {
            self.unload_render_session();
            return;
        }
        let request = RenderRefreshPayload {
            setup_id: project.project.root.setup.clone(),
            project,
            sequence_id,
        };
        self.enqueue_render_refresh(request);
    }

    pub(super) fn enqueue_render_refresh(&self, request: RenderRefreshPayload) {
        let failed = !lock_unpoisoned(&self.render_refresh).schedule(request);
        if failed {
            self.set_render_error_if_changed("Render refresh worker is unavailable.".to_string());
        }
    }

    pub(super) fn drain_render_refresh_results(&self) {
        let results = lock_unpoisoned(&self.render_refresh).drain_current_results();
        for result in results {
            match result {
                RenderRefreshResult::Refreshed {
                    sequence: _,
                    session,
                } => {
                    lock_unpoisoned(&self.sequence_render).apply_prepared(*session);
                    self.clear_render_error_if_set();
                    self.resume_live_output_after_prepare();
                }
                RenderRefreshResult::Failed {
                    sequence: _,
                    message,
                } => {
                    self.suspend_live_output();
                    self.set_render_error_if_changed(format!("Render refresh failed: {message}"));
                }
            }
        }
    }

    pub(super) fn unload_render_session(&self) {
        self.disable_live_output();
        lock_unpoisoned(&self.sequence_render).unload();
    }
}
