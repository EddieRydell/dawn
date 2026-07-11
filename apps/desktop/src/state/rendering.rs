use super::*;

impl DesktopState {
    pub(super) fn refresh_render_session(
        &self,
        project: &dawn_language::model::DawnProject,
    ) -> Option<dawn_runtime::RenderError> {
        match self.show_render.lock() {
            Ok(mut show_render) => {
                let result = show_render.refresh_project(project);
                if result.is_err() {
                    show_render.unload();
                }
                result.err()
            }
            Err(poisoned) => {
                let mut show_render = poisoned.into_inner();
                let result = show_render.refresh_project(project);
                if result.is_err() {
                    show_render.unload();
                }
                result.err()
            }
        }
    }

    pub(super) fn schedule_render_refresh(&self, project: Arc<ProjectSession>) {
        let target = match self.show_render.lock() {
            Ok(show_render) => show_render.active_target(),
            Err(poisoned) => poisoned.into_inner().active_target(),
        };
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
        let failed = match self.render_refresh.lock() {
            Ok(mut scheduler) => scheduler.schedule(request).is_err(),
            Err(poisoned) => poisoned.into_inner().schedule(request).is_err(),
        };
        if failed {
            self.set_render_error_if_changed("Render refresh worker is unavailable.".to_string());
        }
    }

    pub(super) fn drain_render_refresh_results(&self) {
        let results = match self.render_refresh.lock() {
            Ok(scheduler) => scheduler.drain_current_results(),
            Err(poisoned) => poisoned.into_inner().drain_current_results(),
        };
        for result in results {
            match result {
                RenderRefreshResult::Refreshed {
                    sequence: _,
                    session,
                } => {
                    match self.show_render.lock() {
                        Ok(mut show_render) => show_render.apply_prepared(*session),
                        Err(poisoned) => poisoned.into_inner().apply_prepared(*session),
                    }
                    self.clear_render_error_if_set();
                }
                RenderRefreshResult::Failed {
                    sequence: _,
                    message,
                } => {
                    self.set_render_error_if_changed(format!("Render refresh failed: {message}"));
                }
            }
        }
    }

    pub(super) fn unload_render_session(&self) {
        match self.show_render.lock() {
            Ok(mut show_render) => show_render.unload(),
            Err(poisoned) => poisoned.into_inner().unload(),
        }
    }
}
