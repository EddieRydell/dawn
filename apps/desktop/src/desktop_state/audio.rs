use super::{DesktopState, lock_unpoisoned};
use crate::dto::{AppSnapshot, GuiDocumentRequest};

impl DesktopState {
    pub fn load_sequence_audio(&self, request: GuiDocumentRequest) -> AppSnapshot {
        let audio = self.resolve_sequence_audio(&request);
        let sequence_id = self.resolve_sequence_id(&request);
        let audio_transport = lock_unpoisoned(&self.audio).load(audio);
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
        let audio_transport = lock_unpoisoned(&self.audio).unload();
        self.unload_render_session();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
            snapshot.render_error = None;
        })
    }

    pub fn audio_play(&self) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).play();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_pause(&self) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).pause();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_stop(&self) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).stop();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_rewind_to_zero(&self) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).rewind_to_zero();
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn audio_seek(&self, position_seconds: f64) -> AppSnapshot {
        let audio_transport = lock_unpoisoned(&self.audio).seek(position_seconds);
        self.update_snapshot(|snapshot| {
            snapshot.audio_transport = audio_transport;
        })
    }

    pub fn render_current_sequence_frame(
        &self,
    ) -> Result<
        crate::sequence_render::AudioClockRenderedFrame,
        crate::sequence_render::SequenceRenderError,
    > {
        self.drain_render_refresh_results();
        let audio_transport = self.audio_snapshot();
        lock_unpoisoned(&self.sequence_render).render_current_sequence_frame(&audio_transport)
    }

    pub fn active_preview_render_identity(
        &self,
    ) -> Result<
        crate::sequence_render::AudioClockRenderIdentity,
        crate::sequence_render::SequenceRenderError,
    > {
        self.drain_render_refresh_results();
        let audio_transport = self.audio_snapshot();
        lock_unpoisoned(&self.sequence_render).active_render_identity(&audio_transport)
    }

    pub fn preview_scene(&self) -> Option<crate::preview::PreviewScene> {
        let session = self.project_session()?;
        Some(crate::preview::PreviewScene::from_project(
            self.project_revision(),
            &session.project,
        ))
    }

    pub fn preview_scene_revision(&self) -> Option<u64> {
        self.project_session().map(|_| self.project_revision())
    }
}
