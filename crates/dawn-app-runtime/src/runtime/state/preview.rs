use dawn_language::document::SequenceDocument;
use dawn_language::path::Utf8PathBuf;

use crate::editor::EditorViewMode;
use crate::preview::session::{
    AudioPlaybackStatus, PreviewRenderRequest, PreviewRenderResult, PreviewSnapshot,
    PreviewSyncMode, SequenceKey,
};
use crate::runtime::contracts::RuntimeStatus;

use super::CoordinatorState;

impl CoordinatorState {
    pub(crate) fn set_effect_preview_enabled(&mut self, enabled: bool) -> Result<(), String> {
        if !enabled {
            self.preview.clear_effect_preview(self.workspace.analysis());
        } else {
            self.preview.render_current_frame(self.workspace.analysis());
        }
        Ok(())
    }

    pub(crate) fn set_effect_preview_effects(
        &mut self,
        ids: Vec<u32>,
        effect_preview_enabled: bool,
    ) {
        let ids = if effect_preview_enabled {
            ids
        } else {
            Vec::new()
        };
        self.preview
            .set_effect_preview_ids(ids, self.workspace.analysis());
    }

    pub(crate) fn preview_play(&mut self) {
        self.preview.play(self.workspace.analysis());
        self.status = RuntimeStatus::message("Preview playing");
    }

    pub(crate) fn preview_pause(&mut self) {
        self.preview.pause(self.workspace.analysis());
        self.status = RuntimeStatus::message("Preview paused");
    }

    pub(crate) fn preview_stop(&mut self) {
        self.preview.stop(self.workspace.analysis());
        self.status = RuntimeStatus::message("Preview stopped");
    }

    pub(crate) fn preview_rewind_to_zero(&mut self) {
        self.preview
            .go_to_sequence_beginning(self.workspace.analysis());
        self.status = RuntimeStatus::message("Preview rewound");
    }

    pub(crate) fn preview_seek(&mut self, position_seconds: f64) {
        self.preview
            .seek(position_seconds, self.workspace.analysis());
        self.status = RuntimeStatus::message("Preview seeked");
    }

    pub(crate) fn tick_preview(&mut self) {
        self.preview.tick(self.workspace.analysis());
    }

    pub(crate) fn tick_preview_clock(&mut self) {
        self.preview.tick_clock();
    }

    pub(crate) fn render_preview_frame(&mut self) {
        self.preview.render_current_frame(self.workspace.analysis());
    }

    pub(crate) fn begin_deferred_preview_render(&mut self) -> Option<PreviewRenderRequest> {
        self.preview.begin_deferred_render()
    }

    pub(crate) fn complete_deferred_preview_render(&mut self, result: PreviewRenderResult) -> bool {
        self.preview.complete_deferred_render(result)
    }

    pub(crate) fn preview_target_fps(&self) -> u32 {
        self.preview.target_fps()
    }

    pub(crate) fn preview_snapshot(&self) -> PreviewSnapshot {
        self.preview.snapshot()
    }

    pub(crate) fn preview_last_render_timing(
        &self,
    ) -> crate::preview::session::PreviewRenderTiming {
        self.preview.last_render_timing()
    }

    pub(crate) fn preview_pause_at_native_audio(
        &mut self,
        position_seconds: f64,
        status: AudioPlaybackStatus,
    ) {
        self.preview
            .pause_at(position_seconds, self.workspace.analysis());
        self.preview.set_timing_status("nativeAudio", status);
        self.status = RuntimeStatus::message("Preview paused");
    }

    pub(crate) fn preview_stop_native_audio(&mut self, status: AudioPlaybackStatus) {
        self.preview.stop_native_audio(self.workspace.analysis());
        self.preview.set_timing_status("nativeAudio", status);
        self.status = RuntimeStatus::message("Preview stopped");
    }

    pub(crate) fn preview_rewind_native_audio(&mut self, status: AudioPlaybackStatus) {
        self.preview
            .go_to_sequence_beginning_native_audio(self.workspace.analysis());
        self.preview.set_timing_status("nativeAudio", status);
        self.status = RuntimeStatus::message("Preview rewound");
    }

    pub(crate) fn preview_seek_native_audio(
        &mut self,
        position_seconds: f64,
        playing: bool,
        status: AudioPlaybackStatus,
    ) {
        self.preview
            .seek_native_audio(position_seconds, playing, self.workspace.analysis());
        self.preview.set_timing_status("nativeAudio", status);
        self.status = RuntimeStatus::message("Preview seeked");
    }

    pub(crate) fn apply_audio_clock_state(
        &mut self,
        position_seconds: f64,
        status: AudioPlaybackStatus,
        ended: bool,
        error: Option<&str>,
    ) {
        if let Some(error) = error {
            self.preview
                .pause_at(position_seconds, self.workspace.analysis());
            self.preview
                .set_timing_status("nativeAudio", AudioPlaybackStatus::Error);
            self.status = RuntimeStatus::message(format!("Audio error: {error}"));
            return;
        }
        if ended {
            self.preview.render_at_native_audio_clock(
                position_seconds,
                true,
                self.workspace.analysis(),
            );
            self.preview
                .set_timing_status("nativeAudio", AudioPlaybackStatus::Ended);
            self.status = RuntimeStatus::message("Preview complete");
            return;
        }
        match status {
            AudioPlaybackStatus::Loading => {
                self.preview
                    .pause_at(position_seconds, self.workspace.analysis());
                self.preview
                    .set_timing_status("nativeAudio", AudioPlaybackStatus::Loading);
                self.status = RuntimeStatus::message("Loading audio");
            }
            AudioPlaybackStatus::LoadingToPlay => {
                self.preview
                    .pause_at(position_seconds, self.workspace.analysis());
                self.preview
                    .set_timing_status("nativeAudio", AudioPlaybackStatus::LoadingToPlay);
                self.status = RuntimeStatus::message("Loading audio - will play");
            }
            AudioPlaybackStatus::Playing => {
                self.preview
                    .play_from_native_audio_clock(position_seconds, self.workspace.analysis());
                self.preview
                    .set_timing_status("nativeAudio", AudioPlaybackStatus::Playing);
                self.status = RuntimeStatus::message("Preview playing");
            }
            AudioPlaybackStatus::Ended => {
                self.preview.render_at_native_audio_clock(
                    position_seconds,
                    true,
                    self.workspace.analysis(),
                );
                self.preview
                    .set_timing_status("nativeAudio", AudioPlaybackStatus::Ended);
                self.status = RuntimeStatus::message("Preview complete");
            }
            AudioPlaybackStatus::Missing => {
                self.preview
                    .pause_at(position_seconds, self.workspace.analysis());
                self.preview
                    .set_timing_status("silent", AudioPlaybackStatus::Missing);
                self.status = RuntimeStatus::message("Audio missing");
            }
            AudioPlaybackStatus::None => {
                self.preview
                    .pause_at(position_seconds, self.workspace.analysis());
                self.preview
                    .set_timing_status("silent", AudioPlaybackStatus::None);
                self.status = RuntimeStatus::message("Preview ready");
            }
            AudioPlaybackStatus::Ready | AudioPlaybackStatus::Error => {
                self.preview
                    .pause_at(position_seconds, self.workspace.analysis());
                self.preview.set_timing_status("nativeAudio", status);
                self.status = RuntimeStatus::message("Preview ready");
            }
        }
    }

    pub(crate) fn active_sequence_document_for_preview_request(
        &self,
        path: &Utf8PathBuf,
        object_key: &str,
    ) -> Result<SequenceDocument, String> {
        let Some(buffer) = self.editor.active_buffer() else {
            return Err("sequence preview request does not match active sequence".to_string());
        };
        if buffer.view_mode != EditorViewMode::Gui || buffer.is_conflicted() {
            return Err("sequence preview request does not match active sequence".to_string());
        }
        let document = self.workspace.sequence_document(
            buffer.path.clone(),
            object_key,
            self.editor.dirty_overlays(),
        )?;
        if buffer.path != *path && document.path != path.as_str() {
            return Err("sequence preview request does not match active sequence".to_string());
        }
        Ok(document)
    }

    pub(super) fn sync_preview_source(&mut self, mode: PreviewSyncMode) {
        let source = self.active_sequence_source();
        self.preview
            .sync_source(source, self.workspace.analysis(), mode);
    }

    pub(super) fn sync_preview_source_from_document(
        &mut self,
        path: Utf8PathBuf,
        document: SequenceDocument,
        mode: PreviewSyncMode,
    ) {
        let source = Some((
            SequenceKey {
                path,
                object_key: document.object_key.clone(),
            },
            document,
        ));
        self.preview
            .sync_source(source, self.workspace.analysis(), mode);
    }
}
