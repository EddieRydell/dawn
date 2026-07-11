use super::*;

impl DesktopState {
    pub fn load_sequence_audio(&self, request: GuiDocumentRequest) -> AppSnapshot {
        let audio = self.resolve_sequence_audio(&request);
        let sequence_id = self.resolve_sequence_id(&request);
        let audio_transport = match self.audio.lock() {
            Ok(mut engine) => engine.load(audio),
            Err(poisoned) => poisoned.into_inner().load(audio),
        };
        match (self.project_session(), sequence_id) {
            (Some(project), Some(sequence_id)) => {
                self.unload_render_session();
                self.schedule_sequence_render_prepare(project, sequence_id);
            }
            _ => {
                self.unload_render_session();
            }
        };
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
            snapshot.render_error = None;
        })
    }

    pub fn unload_audio(&self) -> AppSnapshot {
        let audio_transport = match self.audio.lock() {
            Ok(mut engine) => engine.unload(),
            Err(poisoned) => poisoned.into_inner().unload(),
        };
        self.unload_render_session();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
            snapshot.render_error = None;
        })
    }

    pub fn audio_play(&self) -> AppSnapshot {
        let audio_transport = match self.audio.lock() {
            Ok(mut engine) => engine.play(),
            Err(poisoned) => poisoned.into_inner().play(),
        };
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_pause(&self) -> AppSnapshot {
        let audio_transport = match self.audio.lock() {
            Ok(mut engine) => engine.pause(),
            Err(poisoned) => poisoned.into_inner().pause(),
        };
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_stop(&self) -> AppSnapshot {
        let audio_transport = match self.audio.lock() {
            Ok(mut engine) => engine.stop(),
            Err(poisoned) => poisoned.into_inner().stop(),
        };
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_rewind_to_zero(&self) -> AppSnapshot {
        let audio_transport = match self.audio.lock() {
            Ok(mut engine) => engine.rewind_to_zero(),
            Err(poisoned) => poisoned.into_inner().rewind_to_zero(),
        };
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_seek(&self, position_seconds: f64) -> AppSnapshot {
        let audio_transport = match self.audio.lock() {
            Ok(mut engine) => engine.seek(position_seconds),
            Err(poisoned) => poisoned.into_inner().seek(position_seconds),
        };
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn render_current_sequence_frame(
        &self,
    ) -> Result<crate::show_render::AudioClockRenderedFrame, crate::show_render::ShowRenderError>
    {
        self.drain_render_refresh_results();
        let audio_transport = self.audio_snapshot();
        match self.show_render.lock() {
            Ok(mut show_render) => show_render.render_current_sequence_frame(&audio_transport),
            Err(poisoned) => poisoned
                .into_inner()
                .render_current_sequence_frame(&audio_transport),
        }
    }

    pub fn active_preview_render_identity(
        &self,
    ) -> Result<crate::show_render::AudioClockRenderIdentity, crate::show_render::ShowRenderError>
    {
        self.drain_render_refresh_results();
        let audio_transport = self.audio_snapshot();
        match self.show_render.lock() {
            Ok(show_render) => show_render.active_render_identity(&audio_transport),
            Err(poisoned) => poisoned
                .into_inner()
                .active_render_identity(&audio_transport),
        }
    }

    pub fn preview_scene(&self) -> Option<crate::preview::PreviewScene> {
        match self.project.lock() {
            Ok(project) => {
                let session = project.as_ref()?;
                Some(crate::preview::PreviewScene::from_project(
                    self.project_revision(),
                    &session.project,
                ))
            }
            Err(poisoned) => {
                let project = poisoned.into_inner();
                let session = project.as_ref()?;
                Some(crate::preview::PreviewScene::from_project(
                    self.project_revision(),
                    &session.project,
                ))
            }
        }
    }

    pub fn preview_scene_revision(&self) -> Option<u64> {
        match self.project.lock() {
            Ok(project) => project.as_ref().map(|_| self.project_revision()),
            Err(poisoned) => poisoned
                .into_inner()
                .as_ref()
                .map(|_| self.project_revision()),
        }
    }
}
