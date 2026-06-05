use dawn_language::document::SequenceDocument;
use dawn_language::path::Utf8PathBuf;

use crate::editor::EditorViewMode;
use crate::preview::session::{
    AudioPlaybackStatus, PreviewRenderRequest, PreviewRenderResult, PreviewSnapshot,
    PreviewSyncMode, SequenceKey,
};
use crate::runtime::contracts::{RuntimeActivity, RuntimeNotice, RuntimeStatus};
use crate::runtime::workers::{SequenceEffectPreviewRequestEffect, SequenceEffectPreviewResult};

use super::AppBackend;

impl AppBackend {
    pub(crate) fn sync_effect_preview_enabled(&mut self, enabled: bool) -> Result<(), String> {
        if !enabled {
            self.preview.clear_effect_preview(self.workspace.analysis());
        } else {
            self.preview.render_current_frame(self.workspace.analysis());
        }
        Ok(())
    }

    pub(crate) fn sync_effect_preview_effects(
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

    pub(crate) fn start_preview_playback(&mut self) {
        self.preview.play(self.workspace.analysis());
        self.status.clear_activity(RuntimeActivity::LoadingAudio);
        self.status
            .clear_activity(RuntimeActivity::LoadingAudioToPlay);
        self.status.set_notice(RuntimeNotice::PreviewPlaying);
    }

    pub(super) fn preview_play_command(&mut self) -> Result<(), String> {
        if self.app_prefs.effect_preview_enabled() {
            self.app_prefs.set_effect_preview_enabled(false)?;
            self.sync_effect_preview_enabled(false)?;
        }
        self.start_preview_playback();
        Ok(())
    }

    pub(super) fn preview_pause_command(&mut self) {
        self.preview.pause(self.workspace.analysis());
        self.status.set_notice(RuntimeNotice::PreviewPaused);
    }

    pub(super) fn preview_stop_command(&mut self) {
        self.preview.stop(self.workspace.analysis());
        self.status.set_notice(RuntimeNotice::PreviewStopped);
    }

    pub(super) fn preview_rewind_to_zero_command(&mut self) {
        self.preview
            .go_to_sequence_beginning(self.workspace.analysis());
        self.status.set_notice(RuntimeNotice::PreviewRewound);
    }

    pub(super) fn preview_seek_command(&mut self, position_seconds: f64) {
        self.preview
            .seek(position_seconds, self.workspace.analysis());
        self.status.set_notice(RuntimeNotice::PreviewSeeked);
    }

    pub(super) fn tick_preview(&mut self) {
        self.preview.tick(self.workspace.analysis());
    }

    pub(super) fn tick_preview_clock(&mut self) {
        self.preview.tick_clock();
    }

    pub(super) fn render_preview_frame(&mut self) {
        self.preview.render_current_frame(self.workspace.analysis());
    }

    pub(super) fn request_deferred_preview_render(&mut self) -> Result<bool, String> {
        let Some(request) = self.preview.begin_deferred_render() else {
            return Ok(false);
        };
        self.workers
            .request_preview_frame(self.workspace.analysis().cloned(), request)?;
        Ok(true)
    }

    pub(super) fn begin_deferred_preview_render(&mut self) -> Option<PreviewRenderRequest> {
        self.preview.begin_deferred_render()
    }

    pub(super) fn complete_deferred_preview_render(&mut self, result: PreviewRenderResult) -> bool {
        self.preview.complete_deferred_render(result)
    }

    pub(super) fn preview_target_fps(&self) -> u32 {
        self.preview.target_fps()
    }

    pub(super) fn preview_snapshot(&self) -> PreviewSnapshot {
        self.preview.snapshot()
    }

    pub(super) fn preview_last_render_timing(
        &self,
    ) -> crate::preview::session::PreviewRenderTiming {
        self.preview.last_render_timing()
    }

    pub(super) fn active_sequence_audio_context(
        &self,
    ) -> Result<(Option<String>, Utf8PathBuf), String> {
        let Some(sequence_path) = self.document_store.active_file().cloned() else {
            return Err("no active sequence file is selected".to_string());
        };
        if !self
            .active_gui_document()
            .as_ref()
            .is_some_and(|document| document.is_sequence())
        {
            return Err("active document is not a sequence".to_string());
        }
        Ok((self.workspace.project_root(), sequence_path))
    }

    pub(super) fn effect_preview_request_source(
        &self,
        path: Utf8PathBuf,
        object_key: &str,
    ) -> Result<(dawn_language::analysis::ProjectAnalysis, SequenceDocument), String> {
        let analysis = self
            .workspace
            .analysis()
            .ok_or_else(|| "project analysis is not available".to_string())?
            .clone();
        let document = self.active_sequence_document_for_preview_request(&path, object_key)?;
        Ok((analysis, document))
    }

    pub(super) fn request_sequence_effect_previews_command(
        &mut self,
        path: Utf8PathBuf,
        object_key: String,
        request_id: u32,
        effects: Vec<SequenceEffectPreviewRequestEffect>,
    ) -> Result<(), String> {
        let path_string = path.to_string();
        let (analysis, document) = self.effect_preview_request_source(path, &object_key)?;
        self.workers.request_effect_previews(
            path_string,
            object_key,
            request_id,
            effects,
            analysis,
            document,
        )
    }

    pub(super) fn take_sequence_effect_preview_results_command(
        &self,
        path: String,
        object_key: String,
    ) -> Result<Vec<SequenceEffectPreviewResult>, String> {
        self.workers.take_effect_preview_results(path, object_key)
    }

    pub(super) fn preview_pause_at_native_audio(
        &mut self,
        position_seconds: f64,
        status: AudioPlaybackStatus,
    ) {
        self.preview
            .pause_at(position_seconds, self.workspace.analysis());
        self.preview.set_timing_status("nativeAudio", status);
        self.status.set_notice(RuntimeNotice::PreviewPaused);
    }

    pub(super) fn preview_stop_native_audio(&mut self, status: AudioPlaybackStatus) {
        self.preview.stop_native_audio(self.workspace.analysis());
        self.preview.set_timing_status("nativeAudio", status);
        self.status.set_notice(RuntimeNotice::PreviewStopped);
    }

    pub(super) fn preview_rewind_native_audio(&mut self, status: AudioPlaybackStatus) {
        self.preview
            .go_to_sequence_beginning_native_audio(self.workspace.analysis());
        self.preview.set_timing_status("nativeAudio", status);
        self.status.set_notice(RuntimeNotice::PreviewRewound);
    }

    pub(super) fn preview_seek_native_audio(
        &mut self,
        position_seconds: f64,
        playing: bool,
        status: AudioPlaybackStatus,
    ) {
        self.preview
            .seek_native_audio(position_seconds, playing, self.workspace.analysis());
        self.preview.set_timing_status("nativeAudio", status);
        self.status.set_notice(RuntimeNotice::PreviewSeeked);
    }

    pub(super) fn apply_audio_clock_state(
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
            self.status.clear_activity(RuntimeActivity::LoadingAudio);
            self.status
                .clear_activity(RuntimeActivity::LoadingAudioToPlay);
            self.status = RuntimeStatus::error(format!("Audio error: {error}"));
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
            self.status.clear_activity(RuntimeActivity::LoadingAudio);
            self.status
                .clear_activity(RuntimeActivity::LoadingAudioToPlay);
            self.status.set_notice(RuntimeNotice::PreviewComplete);
            return;
        }
        match status {
            AudioPlaybackStatus::Loading => {
                self.preview
                    .pause_at(position_seconds, self.workspace.analysis());
                self.preview
                    .set_timing_status("nativeAudio", AudioPlaybackStatus::Loading);
                self.status.set_activity(RuntimeActivity::LoadingAudio);
                self.status
                    .clear_activity(RuntimeActivity::LoadingAudioToPlay);
            }
            AudioPlaybackStatus::LoadingToPlay => {
                self.preview
                    .pause_at(position_seconds, self.workspace.analysis());
                self.preview
                    .set_timing_status("nativeAudio", AudioPlaybackStatus::LoadingToPlay);
                self.status.clear_activity(RuntimeActivity::LoadingAudio);
                self.status
                    .set_activity(RuntimeActivity::LoadingAudioToPlay);
            }
            AudioPlaybackStatus::Playing => {
                self.preview
                    .play_from_native_audio_clock(position_seconds, self.workspace.analysis());
                self.preview
                    .set_timing_status("nativeAudio", AudioPlaybackStatus::Playing);
                self.status.clear_activity(RuntimeActivity::LoadingAudio);
                self.status
                    .clear_activity(RuntimeActivity::LoadingAudioToPlay);
                self.status.set_notice(RuntimeNotice::PreviewPlaying);
            }
            AudioPlaybackStatus::Ended => {
                self.preview.render_at_native_audio_clock(
                    position_seconds,
                    true,
                    self.workspace.analysis(),
                );
                self.preview
                    .set_timing_status("nativeAudio", AudioPlaybackStatus::Ended);
                self.status.clear_activity(RuntimeActivity::LoadingAudio);
                self.status
                    .clear_activity(RuntimeActivity::LoadingAudioToPlay);
                self.status.set_notice(RuntimeNotice::PreviewComplete);
            }
            AudioPlaybackStatus::Missing => {
                self.preview
                    .pause_at(position_seconds, self.workspace.analysis());
                self.preview
                    .set_timing_status("silent", AudioPlaybackStatus::Missing);
                self.status.clear_activity(RuntimeActivity::LoadingAudio);
                self.status
                    .clear_activity(RuntimeActivity::LoadingAudioToPlay);
                self.status.set_notice(RuntimeNotice::AudioMissing);
            }
            AudioPlaybackStatus::None => {
                self.preview
                    .pause_at(position_seconds, self.workspace.analysis());
                self.preview
                    .set_timing_status("silent", AudioPlaybackStatus::None);
                self.status.clear_activity(RuntimeActivity::LoadingAudio);
                self.status
                    .clear_activity(RuntimeActivity::LoadingAudioToPlay);
                self.status.set_notice(RuntimeNotice::PreviewReady);
            }
            AudioPlaybackStatus::Ready | AudioPlaybackStatus::Error => {
                self.preview
                    .pause_at(position_seconds, self.workspace.analysis());
                self.preview.set_timing_status("nativeAudio", status);
                self.status.clear_activity(RuntimeActivity::LoadingAudio);
                self.status
                    .clear_activity(RuntimeActivity::LoadingAudioToPlay);
                self.status.set_notice(RuntimeNotice::PreviewReady);
            }
        }
    }

    pub(crate) fn active_sequence_document_for_preview_request(
        &self,
        path: &Utf8PathBuf,
        object_key: &str,
    ) -> Result<SequenceDocument, String> {
        let Some(buffer) = self.document_store.active_buffer() else {
            return Err("sequence preview request does not match active sequence".to_string());
        };
        if buffer.view_mode != EditorViewMode::Gui || buffer.is_conflicted() {
            return Err("sequence preview request does not match active sequence".to_string());
        }
        let document = self.workspace.sequence_document(
            buffer.path.clone(),
            object_key,
            self.document_store.dirty_overlays(),
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
