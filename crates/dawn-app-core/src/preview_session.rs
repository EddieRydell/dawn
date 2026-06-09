use std::collections::{HashMap, HashSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;

use crate::document::{SequenceAudioDocument, SequenceEditorDocument};
use dawn_project::DawnProject;
use dawn_project::Utf8PathBuf;

use crate::output_runtime::{
    empty_frame, empty_geometry, OutputFrameStatus, OutputGeometryModel, RenderedOutputFrame,
    SequenceFrameRenderTiming,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AudioPlaybackStatus {
    None,
    Missing,
    Loading,
    LoadingToPlay,
    Ready,
    Playing,
    Ended,
    Error,
}

impl AudioPlaybackStatus {
    pub fn is_loading(self) -> bool {
        matches!(self, Self::Loading | Self::LoadingToPlay)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SequenceKey {
    pub path: Utf8PathBuf,
    pub object_key: String,
}

#[derive(Debug, Clone, Default)]
pub struct SequencePlaybackState {
    pub position_seconds: f64,
    pub home_seconds: f64,
}

#[derive(Debug, Clone)]
pub enum PreviewSource {
    None,
    Sequence {
        key: SequenceKey,
        document: Arc<SequenceEditorDocument>,
    },
}

#[derive(Debug, Clone)]
pub enum PreviewTransport {
    Stopped,
    Paused,
    NativeAudioPlaying,
    Playing {
        started_at: Instant,
        started_position_seconds: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackTransportState {
    Stopped,
    Paused,
    LoadingToPlay,
    Playing,
    SelectedEffects,
    Ended,
    Error,
}

impl PlaybackTransportState {
    pub fn is_active_playback(self) -> bool {
        matches!(self, Self::Playing | Self::SelectedEffects)
    }

    pub fn should_animate_position(self) -> bool {
        matches!(self, Self::Playing)
    }

    pub fn should_publish_continuously(self) -> bool {
        matches!(self, Self::Playing | Self::SelectedEffects)
    }
}

#[derive(Debug, Clone)]
pub struct PreviewSnapshot {
    pub source_label: String,
    pub source_key: Option<SequenceKey>,
    pub transport_state: PlaybackTransportState,
    pub preview_updating: bool,
    pub position_seconds: f64,
    pub home_seconds: f64,
    pub duration_seconds: f64,
    pub audio: Option<SequenceAudioDocument>,
    pub clock_source: String,
    pub audio_playback_status: AudioPlaybackStatus,
    pub geometry: Arc<OutputGeometryModel>,
    pub frame: Arc<RenderedOutputFrame>,
    pub status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewSyncMode {
    RenderNow,
    DeferRender,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PlaybackRenderTiming {
    pub total_ms: f64,
    pub renderer_build_ms: f64,
    pub frame_evaluate_ms: f64,
    pub render_buffer_clone_ms: f64,
    pub effect_loop_ms: f64,
    pub rgb_buffer_ms: f64,
    pub render_invalidation_ms: f64,
    pub render_cache_ms: f64,
    pub render_result_ms: f64,
    pub active_effects: u32,
    pub sampled_pixels: u32,
}

#[derive(Debug, Clone)]
pub struct PlaybackRenderRequest {
    pub id: u64,
    pub dirty_revision: u64,
    pub generation: u64,
    pub key: SequenceKey,
    pub document: Arc<SequenceEditorDocument>,
    pub kind: PlaybackRenderMode,
    pub status: String,
    pub cancellation: PreviewCancellationToken,
}

#[derive(Debug, Clone)]
pub enum PlaybackRenderMode {
    FullSequenceFrame {
        position_seconds: f64,
        frame_index: u64,
    },
    SelectedEffects {
        preview_seconds: f64,
        ids: HashSet<u32>,
    },
}

#[derive(Debug, Clone)]
pub struct PreviewCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl Default for PreviewCancellationToken {
    fn default() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl PreviewCancellationToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone)]
pub struct PlaybackRenderResult {
    pub request: PlaybackRenderRequest,
    pub geometry: OutputGeometryModel,
    pub frame: RenderedOutputFrame,
    pub timing: PlaybackRenderTiming,
}

#[derive(Debug, Clone)]
struct EffectPreviewState {
    ids: HashSet<u32>,
    started_at: Instant,
}

#[derive(Debug, Clone)]
pub struct PlaybackSession {
    source: PreviewSource,
    transport: PreviewTransport,
    sequence_states: HashMap<SequenceKey, SequencePlaybackState>,
    effect_preview: Option<EffectPreviewState>,
    last_native_audio_frame_index: Option<u64>,
    last_render_timing: PlaybackRenderTiming,
    generation: u64,
    dirty_revision: u64,
    next_deferred_render_id: u64,
    pending_deferred_render: Option<PendingDeferredRender>,
    snapshot: PreviewSnapshot,
}

#[derive(Debug, Clone)]
struct PendingDeferredRender {
    id: u64,
    dirty_revision: u64,
    generation: u64,
    cancellation: PreviewCancellationToken,
    started: bool,
}

impl PartialEq for PendingDeferredRender {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.dirty_revision == other.dirty_revision
            && self.generation == other.generation
    }
}

impl Default for PlaybackSession {
    fn default() -> Self {
        let geometry = Arc::new(empty_geometry());
        let frame = empty_frame(&geometry, 0, "No sequence preview source");
        Self {
            source: PreviewSource::None,
            transport: PreviewTransport::Stopped,
            sequence_states: HashMap::new(),
            effect_preview: None,
            last_native_audio_frame_index: None,
            last_render_timing: PlaybackRenderTiming::default(),
            generation: 0,
            dirty_revision: 0,
            next_deferred_render_id: 0,
            pending_deferred_render: None,
            snapshot: PreviewSnapshot {
                source_label: "No preview source".to_string(),
                source_key: None,
                transport_state: PlaybackTransportState::Stopped,
                preview_updating: false,
                position_seconds: 0.0,
                home_seconds: 0.0,
                duration_seconds: 0.0,
                audio: None,
                clock_source: "silent".to_string(),
                audio_playback_status: AudioPlaybackStatus::None,
                geometry,
                frame: Arc::new(frame),
                status: "No sequence preview source".to_string(),
            },
        }
    }
}

impl PlaybackSession {
    pub fn snapshot(&self) -> PreviewSnapshot {
        self.snapshot.clone()
    }

    pub fn last_render_timing(&self) -> PlaybackRenderTiming {
        self.last_render_timing
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn sync_source(
        &mut self,
        source: Option<(SequenceKey, SequenceEditorDocument)>,
        project: Option<&DawnProject>,
        mode: PreviewSyncMode,
    ) {
        let next_key = source.as_ref().map(|(key, _)| key);
        let source_changed = self.current_key().as_ref() != next_key;
        if source_changed && self.is_playing() {
            self.pause_current(project);
        }
        self.dirty_revision = self.dirty_revision.saturating_add(1);
        if let Some(pending) = self.pending_deferred_render.take() {
            pending.cancellation.cancel();
        }
        match source {
            Some((key, document)) => {
                self.sequence_states.entry(key.clone()).or_default();
                self.source = PreviewSource::Sequence {
                    key,
                    document: Arc::new(document),
                };
            }
            None => {
                self.source = PreviewSource::None;
                self.transport = PreviewTransport::Stopped;
            }
        }
        match mode {
            PreviewSyncMode::RenderNow | PreviewSyncMode::DeferRender => {
                self.schedule_render(project, self.status_for_source())
            }
        }
    }

    pub fn play(&mut self, project: Option<&DawnProject>) {
        self.effect_preview = None;
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            self.transport = PreviewTransport::Stopped;
            self.schedule_render(project, "No sequence preview source");
            return;
        };

        let state = self.sequence_states.entry(key).or_default();
        if state.position_seconds >= duration_seconds {
            state.position_seconds = clamp_position_seconds(state.home_seconds, duration_seconds);
        }
        self.transport = PreviewTransport::Playing {
            started_at: Instant::now(),
            started_position_seconds: state.position_seconds,
        };
        self.schedule_render(project, "Playing");
    }

    pub fn play_from_native_audio_clock(
        &mut self,
        position_seconds: f64,
        project: Option<&DawnProject>,
    ) {
        self.effect_preview = None;
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            self.transport = PreviewTransport::Stopped;
            self.schedule_render(project, "No sequence preview source");
            return;
        };
        let state = self.sequence_states.entry(key).or_default();
        state.position_seconds = clamp_position_seconds(position_seconds, duration_seconds);
        self.transport = PreviewTransport::NativeAudioPlaying;
        self.last_native_audio_frame_index = Some(sequence_frame_index(
            state.position_seconds,
            self.target_fps(),
        ));
        self.schedule_render(project, "Playing");
    }

    pub fn pause(&mut self, project: Option<&DawnProject>) {
        self.pause_current(project);
        self.schedule_render(project, "Paused");
    }

    pub fn pause_at(&mut self, position_seconds: f64, project: Option<&DawnProject>) {
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            self.schedule_render(project, "No active sequence");
            return;
        };
        let state = self.sequence_states.entry(key).or_default();
        state.position_seconds = clamp_position_seconds(position_seconds, duration_seconds);
        self.transport = PreviewTransport::Paused;
        self.last_native_audio_frame_index = None;
        self.schedule_render(project, "Paused");
    }

    pub fn stop(&mut self, project: Option<&DawnProject>) {
        self.capture_position();
        self.transport = PreviewTransport::Stopped;
        self.last_native_audio_frame_index = None;
        if let Some((key, duration_seconds)) = self.sequence_source_meta() {
            let state = self.sequence_states.entry(key).or_default();
            state.position_seconds = clamp_position_seconds(state.home_seconds, duration_seconds);
        }
        self.schedule_render(project, "Stopped");
    }

    pub fn stop_native_audio(&mut self, project: Option<&DawnProject>) {
        self.transport = PreviewTransport::Stopped;
        self.last_native_audio_frame_index = None;
        if let Some((key, duration_seconds)) = self.sequence_source_meta() {
            let state = self.sequence_states.entry(key).or_default();
            state.position_seconds = clamp_position_seconds(state.home_seconds, duration_seconds);
        }
        self.schedule_render(project, "Stopped");
    }

    pub fn seek(&mut self, position_seconds: f64, project: Option<&DawnProject>) {
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            self.schedule_render(project, "No active sequence");
            return;
        };
        let position_seconds = clamp_position_seconds(position_seconds, duration_seconds);
        let state = self.sequence_states.entry(key).or_default();
        state.position_seconds = position_seconds;
        state.home_seconds = position_seconds;
        if self.is_playing() {
            self.transport = PreviewTransport::Playing {
                started_at: Instant::now(),
                started_position_seconds: position_seconds,
            };
        }
        self.schedule_render(project, "Ready");
    }

    pub fn seek_native_audio(
        &mut self,
        position_seconds: f64,
        playing: bool,
        project: Option<&DawnProject>,
    ) {
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            self.schedule_render(project, "No active sequence");
            return;
        };
        let position_seconds = clamp_position_seconds(position_seconds, duration_seconds);
        let state = self.sequence_states.entry(key).or_default();
        state.position_seconds = position_seconds;
        state.home_seconds = position_seconds;
        self.transport = if playing {
            PreviewTransport::NativeAudioPlaying
        } else {
            PreviewTransport::Paused
        };
        self.last_native_audio_frame_index = None;
        self.schedule_render(project, "Ready");
    }

    pub fn set_sequence_playhead(&mut self, time_seconds: f64, project: Option<&DawnProject>) {
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            self.schedule_render(project, "No active sequence");
            return;
        };
        let position_seconds = clamp_position_seconds(time_seconds, duration_seconds);
        let state = self.sequence_states.entry(key).or_default();
        state.position_seconds = position_seconds;
        state.home_seconds = position_seconds;
        if self.is_playing() {
            self.transport = PreviewTransport::Playing {
                started_at: Instant::now(),
                started_position_seconds: position_seconds,
            };
        }
        self.schedule_render(project, "Sequence playhead moved");
    }

    pub fn go_to_sequence_beginning(&mut self, project: Option<&DawnProject>) {
        let Some((key, _)) = self.sequence_source_meta() else {
            self.schedule_render(project, "No active sequence");
            return;
        };
        let state = self.sequence_states.entry(key).or_default();
        state.position_seconds = 0.0;
        state.home_seconds = 0.0;
        self.transport = PreviewTransport::Stopped;
        self.schedule_render(project, "Sequence returned to beginning");
    }

    pub fn go_to_sequence_beginning_native_audio(&mut self, project: Option<&DawnProject>) {
        let Some((key, _)) = self.sequence_source_meta() else {
            self.schedule_render(project, "No active sequence");
            return;
        };
        let state = self.sequence_states.entry(key).or_default();
        state.position_seconds = 0.0;
        state.home_seconds = 0.0;
        self.transport = PreviewTransport::Paused;
        self.schedule_render(project, "Sequence returned to beginning");
    }

    pub fn tick(&mut self, project: Option<&DawnProject>) {
        if self.tick_clock() {
            self.schedule_render(project, self.snapshot.status.clone());
        }
    }

    pub fn tick_clock(&mut self) -> bool {
        if self.effect_preview.is_some() {
            self.refresh_snapshot_metadata("Previewing selected effect");
            return true;
        }
        if !self.is_playing() || matches!(self.transport, PreviewTransport::NativeAudioPlaying) {
            return false;
        }
        if let Some((key, duration_seconds)) = self.sequence_source_meta() {
            let position_seconds =
                clamp_position_seconds(self.playing_position_seconds(), duration_seconds);
            let state = self.sequence_states.entry(key).or_default();
            state.position_seconds = position_seconds;
            if position_seconds >= duration_seconds {
                self.transport = PreviewTransport::Stopped;
                self.refresh_snapshot_metadata("Sequence playback complete");
            } else {
                self.refresh_snapshot_metadata("Playing");
            }
            return true;
        }
        false
    }

    pub fn render_current_frame(&mut self, project: Option<&DawnProject>) {
        let status = self.snapshot.status.clone();
        self.schedule_render(project, status);
    }

    pub fn begin_deferred_render(&mut self) -> Option<PlaybackRenderRequest> {
        if !self.snapshot.preview_updating {
            return None;
        }
        let pending = PendingDeferredRender {
            id: self.next_deferred_render_id,
            dirty_revision: self.dirty_revision,
            generation: self.generation,
            cancellation: self
                .pending_deferred_render
                .as_ref()
                .map(|pending| pending.cancellation.clone())
                .unwrap_or_default(),
            started: false,
        };
        if self
            .pending_deferred_render
            .as_ref()
            .is_some_and(|pending| pending.started)
        {
            return None;
        }
        let PreviewSource::Sequence { key, document } = self.source.clone() else {
            self.snapshot.preview_updating = false;
            return None;
        };
        self.next_deferred_render_id = self.next_deferred_render_id.saturating_add(1);
        if let Some(active) = self.pending_deferred_render.as_mut() {
            active.started = true;
        } else {
            self.pending_deferred_render = Some(PendingDeferredRender {
                started: true,
                ..pending.clone()
            });
        }
        let position_seconds = self.current_position_seconds(&key, document.duration_seconds);
        let kind = match self.effect_preview.clone() {
            Some(effect_preview) => PlaybackRenderMode::SelectedEffects {
                preview_seconds: effect_preview.started_at.elapsed().as_secs_f64(),
                ids: effect_preview.ids,
            },
            None => {
                let frame_index = sequence_frame_index(position_seconds, document.frame_rate);
                PlaybackRenderMode::FullSequenceFrame {
                    position_seconds: frame_start(frame_index, document.frame_rate),
                    frame_index,
                }
            }
        };
        Some(PlaybackRenderRequest {
            id: pending.id,
            dirty_revision: pending.dirty_revision,
            generation: self.generation,
            key,
            document,
            kind,
            status: self.snapshot.status.clone(),
            cancellation: pending.cancellation.clone(),
        })
    }

    pub fn complete_deferred_render(&mut self, result: PlaybackRenderResult) -> bool {
        let pending = PendingDeferredRender {
            id: result.request.id,
            dirty_revision: result.request.dirty_revision,
            generation: result.request.generation,
            cancellation: result.request.cancellation.clone(),
            started: true,
        };
        if self.pending_deferred_render != Some(pending)
            || self.dirty_revision != result.request.dirty_revision
            || self.generation != result.request.generation
            || self.current_key().as_ref() != Some(&result.request.key)
        {
            return false;
        }
        self.pending_deferred_render = None;
        self.last_render_timing = result.timing;
        self.refresh_snapshot_metadata(result.request.status);
        let frame_status =
            status_from_frame(&result.frame.status).unwrap_or_else(|| self.snapshot.status.clone());
        self.snapshot.geometry = Arc::new(result.geometry);
        self.snapshot.frame = Arc::new(result.frame);
        self.snapshot.status = frame_status;
        self.snapshot.preview_updating = false;
        true
    }

    pub fn set_effect_preview_ids(&mut self, ids: Vec<u32>, project: Option<&DawnProject>) {
        let ids = ids.into_iter().collect::<HashSet<_>>();
        self.effect_preview = if ids.is_empty() {
            None
        } else {
            Some(EffectPreviewState {
                ids,
                started_at: Instant::now(),
            })
        };
        let status = self.snapshot.status.clone();
        self.schedule_render(project, status);
    }

    pub fn clear_effect_preview(&mut self, project: Option<&DawnProject>) {
        self.effect_preview = None;
        let status = self.snapshot.status.clone();
        self.schedule_render(project, status);
    }

    pub fn render_at_native_audio_clock(
        &mut self,
        position_seconds: f64,
        ended: bool,
        project: Option<&DawnProject>,
    ) {
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            self.schedule_render(project, "No active sequence");
            return;
        };
        let position_seconds = clamp_position_seconds(position_seconds, duration_seconds);
        let frame_index = sequence_frame_index(position_seconds, self.target_fps());
        self.sequence_states
            .entry(key)
            .or_default()
            .position_seconds = position_seconds;
        if ended || position_seconds >= duration_seconds {
            self.transport = PreviewTransport::Stopped;
            self.last_native_audio_frame_index = None;
            self.schedule_render(project, "Sequence playback complete");
        } else {
            self.transport = PreviewTransport::NativeAudioPlaying;
            self.refresh_snapshot_metadata("Playing");
            if self.last_native_audio_frame_index != Some(frame_index) {
                self.last_native_audio_frame_index = Some(frame_index);
                self.schedule_render(project, "Playing");
            }
        }
    }

    pub fn set_timing_status(
        &mut self,
        clock_source: impl Into<String>,
        audio_playback_status: AudioPlaybackStatus,
    ) {
        let clock_source = clock_source.into();
        self.snapshot.clock_source = clock_source;
        self.snapshot.audio_playback_status = audio_playback_status;
        self.snapshot.transport_state = self.transport_state();
    }

    pub fn is_playing(&self) -> bool {
        matches!(
            self.transport,
            PreviewTransport::Playing { .. } | PreviewTransport::NativeAudioPlaying
        )
    }

    pub fn transport_state(&self) -> PlaybackTransportState {
        if self.effect_preview.is_some() {
            return PlaybackTransportState::SelectedEffects;
        }
        match self.snapshot.audio_playback_status {
            AudioPlaybackStatus::LoadingToPlay => PlaybackTransportState::LoadingToPlay,
            AudioPlaybackStatus::Ended => PlaybackTransportState::Ended,
            AudioPlaybackStatus::Error => PlaybackTransportState::Error,
            _ => match self.transport {
                PreviewTransport::Playing { .. } | PreviewTransport::NativeAudioPlaying => {
                    PlaybackTransportState::Playing
                }
                PreviewTransport::Paused => PlaybackTransportState::Paused,
                PreviewTransport::Stopped => PlaybackTransportState::Stopped,
            },
        }
    }

    pub fn target_fps(&self) -> u32 {
        match &self.source {
            PreviewSource::Sequence { document, .. } => document.frame_rate.max(1),
            PreviewSource::None => 30,
        }
    }

    fn pause_current(&mut self, project: Option<&DawnProject>) {
        self.capture_position();
        if self.is_playing() {
            self.transport = PreviewTransport::Paused;
            self.schedule_render(project, "Paused");
        }
    }

    fn capture_position(&mut self) {
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            return;
        };
        let position_seconds =
            clamp_position_seconds(self.playing_position_seconds(), duration_seconds);
        self.sequence_states
            .entry(key)
            .or_default()
            .position_seconds = position_seconds;
    }

    fn schedule_render(&mut self, project: Option<&DawnProject>, status: impl Into<String>) {
        self.last_render_timing = PlaybackRenderTiming::default();
        self.dirty_revision = self.dirty_revision.saturating_add(1);
        if let Some(pending) = self.pending_deferred_render.take() {
            pending.cancellation.cancel();
        }
        self.generation = self.generation.saturating_add(1);
        let status = status.into();
        let (
            source_label,
            source_key,
            position_seconds,
            home_seconds,
            duration_seconds,
            audio,
            geometry,
            frame,
        ) = match self.source.clone() {
            PreviewSource::None => {
                let geometry = Arc::new(empty_geometry());
                let frame = Arc::new(empty_frame(&geometry, self.generation, status.clone()));
                (
                    "No preview source".to_string(),
                    None,
                    0.0,
                    0.0,
                    0.0,
                    None,
                    geometry,
                    frame,
                )
            }
            PreviewSource::Sequence { key, document } => {
                let duration_seconds = document.duration_seconds;
                let position_seconds = self.current_position_seconds(&key, duration_seconds);
                let home_seconds = self
                    .sequence_states
                    .get(&key)
                    .map(|state| clamp_position_seconds(state.home_seconds, duration_seconds))
                    .unwrap_or_default();
                let geometry = project
                    .and_then(|project| OutputGeometryModel::from_project(project).ok())
                    .map(Arc::new)
                    .unwrap_or_else(|| Arc::new(empty_geometry()));
                let frame = if project.is_some()
                    && self.snapshot.frame.geometry_id == geometry.geometry_id
                {
                    self.snapshot.frame.clone()
                } else {
                    Arc::new(empty_frame(
                        &geometry,
                        self.generation,
                        if project.is_some() {
                            status.as_str()
                        } else {
                            "No project"
                        },
                    ))
                };
                (
                    format!("Sequence {}", document.object_key),
                    Some(key),
                    position_seconds,
                    home_seconds,
                    duration_seconds,
                    document.audio.clone(),
                    geometry,
                    frame,
                )
            }
        };
        let frame_status = status_from_frame(&frame.status).unwrap_or(status);
        let (clock_source, audio_playback_status) =
            timing_status_for(audio.as_ref(), self.is_playing());
        self.snapshot = PreviewSnapshot {
            source_label,
            source_key,
            transport_state: self.transport_state_for(audio_playback_status),
            preview_updating: false,
            position_seconds,
            home_seconds,
            duration_seconds,
            audio,
            clock_source,
            audio_playback_status,
            geometry,
            frame,
            status: frame_status,
        };
        if matches!(self.source, PreviewSource::Sequence { .. }) && project.is_some() {
            self.pending_deferred_render = Some(PendingDeferredRender {
                id: self.next_deferred_render_id,
                dirty_revision: self.dirty_revision,
                generation: self.generation,
                cancellation: PreviewCancellationToken::default(),
                started: false,
            });
            self.snapshot.preview_updating = true;
        }
    }

    fn refresh_snapshot_metadata(&mut self, status: impl Into<String>) {
        let status = status.into();
        match &self.source {
            PreviewSource::None => {
                self.snapshot.source_label = "No preview source".to_string();
                self.snapshot.source_key = None;
                self.snapshot.transport_state = PlaybackTransportState::Stopped;
                self.snapshot.preview_updating = false;
                self.snapshot.position_seconds = 0.0;
                self.snapshot.home_seconds = 0.0;
                self.snapshot.duration_seconds = 0.0;
                self.snapshot.audio = None;
                self.snapshot.clock_source = "silent".to_string();
                self.snapshot.audio_playback_status = AudioPlaybackStatus::None;
                self.snapshot.status = status;
            }
            PreviewSource::Sequence { key, document } => {
                let duration_seconds = document.duration_seconds;
                self.snapshot.source_label = format!("Sequence {}", document.object_key);
                self.snapshot.source_key = Some(key.clone());
                self.snapshot.position_seconds =
                    self.current_position_seconds(key, duration_seconds);
                self.snapshot.home_seconds = self
                    .sequence_states
                    .get(key)
                    .map(|state| clamp_position_seconds(state.home_seconds, duration_seconds))
                    .unwrap_or_default();
                self.snapshot.duration_seconds = duration_seconds;
                self.snapshot.audio = document.audio.clone();
                let (clock_source, audio_playback_status) =
                    timing_status_for(self.snapshot.audio.as_ref(), self.is_playing());
                self.snapshot.clock_source = clock_source;
                self.snapshot.audio_playback_status = audio_playback_status;
                self.snapshot.transport_state = self.transport_state_for(audio_playback_status);
                self.snapshot.status = status;
            }
        }
    }

    fn transport_state_for(
        &self,
        audio_playback_status: AudioPlaybackStatus,
    ) -> PlaybackTransportState {
        if self.effect_preview.is_some() {
            return PlaybackTransportState::SelectedEffects;
        }
        match audio_playback_status {
            AudioPlaybackStatus::LoadingToPlay => PlaybackTransportState::LoadingToPlay,
            AudioPlaybackStatus::Ended => PlaybackTransportState::Ended,
            AudioPlaybackStatus::Error => PlaybackTransportState::Error,
            _ => match self.transport {
                PreviewTransport::Playing { .. } | PreviewTransport::NativeAudioPlaying => {
                    PlaybackTransportState::Playing
                }
                PreviewTransport::Paused => PlaybackTransportState::Paused,
                PreviewTransport::Stopped => PlaybackTransportState::Stopped,
            },
        }
    }

    fn current_position_seconds(&self, key: &SequenceKey, duration_seconds: f64) -> f64 {
        if self.is_playing() && self.current_key().as_ref() == Some(key) {
            clamp_position_seconds(self.playing_position_seconds(), duration_seconds)
        } else {
            self.sequence_states
                .get(key)
                .map(|state| clamp_position_seconds(state.position_seconds, duration_seconds))
                .unwrap_or_default()
        }
    }

    fn playing_position_seconds(&self) -> f64 {
        match self.transport {
            PreviewTransport::NativeAudioPlaying => self
                .current_key()
                .and_then(|key| {
                    self.sequence_states
                        .get(&key)
                        .map(|state| state.position_seconds)
                })
                .unwrap_or_default(),
            PreviewTransport::Playing {
                started_at,
                started_position_seconds,
            } => started_position_seconds + started_at.elapsed().as_secs_f64(),
            PreviewTransport::Stopped | PreviewTransport::Paused => self
                .current_key()
                .and_then(|key| {
                    self.sequence_states
                        .get(&key)
                        .map(|state| state.position_seconds)
                })
                .unwrap_or_default(),
        }
    }

    fn sequence_source_meta(&self) -> Option<(SequenceKey, f64)> {
        match &self.source {
            PreviewSource::Sequence { key, document } => {
                Some((key.clone(), document.duration_seconds))
            }
            PreviewSource::None => None,
        }
    }

    fn current_key(&self) -> Option<SequenceKey> {
        match &self.source {
            PreviewSource::Sequence { key, .. } => Some(key.clone()),
            PreviewSource::None => None,
        }
    }

    fn status_for_source(&self) -> &'static str {
        match self.source {
            PreviewSource::None => "No sequence preview source",
            PreviewSource::Sequence { .. } => "Ready",
        }
    }
}

impl PlaybackRenderTiming {
    pub fn apply_evaluation(
        &mut self,
        renderer_build_ms: f64,
        evaluation: SequenceFrameRenderTiming,
    ) {
        self.total_ms = renderer_build_ms + evaluation.total_ms;
        self.renderer_build_ms = renderer_build_ms;
        self.frame_evaluate_ms = evaluation.total_ms;
        self.render_buffer_clone_ms = evaluation.render_buffer_clone_ms;
        self.effect_loop_ms = evaluation.effect_loop_ms;
        self.rgb_buffer_ms = evaluation.rgb_buffer_ms;
        self.active_effects = evaluation.active_effects;
        self.sampled_pixels = evaluation.sampled_pixels;
    }
}

fn status_from_frame(status: &OutputFrameStatus) -> Option<String> {
    match status {
        OutputFrameStatus::Live => None,
        OutputFrameStatus::Idle(message) | OutputFrameStatus::Error(message) => {
            Some(message.clone())
        }
    }
}

fn timing_status_for(
    audio: Option<&SequenceAudioDocument>,
    is_playing: bool,
) -> (String, AudioPlaybackStatus) {
    match audio {
        Some(audio) if audio.exists => (
            "nativeAudio".to_string(),
            if is_playing {
                AudioPlaybackStatus::Playing
            } else {
                AudioPlaybackStatus::Ready
            },
        ),
        Some(_) => ("silent".to_string(), AudioPlaybackStatus::Missing),
        None => ("silent".to_string(), AudioPlaybackStatus::None),
    }
}

fn clamp_position_seconds(position_seconds: f64, duration_seconds: f64) -> f64 {
    if !position_seconds.is_finite() || position_seconds <= 0.0 {
        0.0
    } else {
        position_seconds.min(duration_seconds.max(0.0))
    }
}

pub fn frame_start(frame_index: u64, frame_rate: u32) -> f64 {
    frame_index as f64 / frame_rate.max(1) as f64
}

fn sequence_frame_index(position_seconds: f64, frame_rate: u32) -> u64 {
    if !position_seconds.is_finite() || position_seconds <= 0.0 {
        0
    } else {
        (position_seconds * frame_rate.max(1) as f64).floor() as u64
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::document::SequenceEditorDocument;
    use crate::workspace::WorkspaceService;
    use dawn_project::{DawnProject, Utf8PathBuf};

    use super::{PlaybackSession, PreviewSyncMode, SequenceKey};

    fn christmas_house_project_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/christmas-house/project.dawn")
    }

    fn christmas_house_project_and_sequence() -> (DawnProject, SequenceEditorDocument, SequenceKey)
    {
        let mut workspace = WorkspaceService::default();
        workspace
            .open_project(
                std::fs::canonicalize(christmas_house_project_path())
                    .expect("christmas house project path should exist"),
            )
            .expect("christmas house project should open");
        let result = workspace.load_project();
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let project = result.project.expect("christmas house project should load");
        let document = workspace
            .sequence_document(
                &project,
                Utf8PathBuf::from("sequences/christmas.sequence.dawn"),
                "christmas",
            )
            .expect("christmas house sequence should load");
        let key = SequenceKey {
            path: document.path.clone().into(),
            object_key: document.object_key.clone(),
        };
        (project, document, key)
    }

    fn edited_sequence_document(document: &SequenceEditorDocument) -> SequenceEditorDocument {
        let mut edited = document.clone();
        edited.frame_rate = edited.frame_rate.saturating_add(1);
        edited
    }

    #[test]
    fn sequence_source_refresh_schedules_latest_deferred_render() {
        let (project, document, key) = christmas_house_project_and_sequence();
        let mut session = PlaybackSession::default();
        session.sync_source(
            Some((key.clone(), document)),
            Some(&project),
            PreviewSyncMode::RenderNow,
        );
        session.seek(2.0, Some(&project));
        let first_request = session
            .begin_deferred_render()
            .expect("initial source should schedule preview render");

        let edited_document = edited_sequence_document(&first_request.document);
        let edited_key = key.clone();
        assert_eq!(key, edited_key);
        session.sync_source(
            Some((edited_key, edited_document)),
            Some(&project),
            PreviewSyncMode::RenderNow,
        );
        let edited_request = session
            .begin_deferred_render()
            .expect("edited source should schedule preview render");

        assert_ne!(first_request.dirty_revision, edited_request.dirty_revision);
        assert_ne!(first_request.generation, edited_request.generation);
        assert!(first_request.cancellation.is_cancelled());
        assert_eq!(edited_request.key, key);
    }

    #[test]
    fn effect_preview_id_changes_schedule_effect_preview_render() {
        let (project, document, key) = christmas_house_project_and_sequence();
        let mut session = PlaybackSession::default();
        session.sync_source(
            Some((key, document)),
            Some(&project),
            PreviewSyncMode::RenderNow,
        );

        session.set_effect_preview_ids(vec![1], Some(&project));
        let first_request = session
            .begin_deferred_render()
            .expect("first effect selection should schedule preview render");
        let super::PlaybackRenderMode::SelectedEffects { ids, .. } = &first_request.kind else {
            panic!("effect preview selection should schedule an effect preview render");
        };
        assert!(ids.contains(&1));

        session.set_effect_preview_ids(vec![23], Some(&project));
        let second_request = session
            .begin_deferred_render()
            .expect("second effect selection should schedule preview render");
        let super::PlaybackRenderMode::SelectedEffects { ids, .. } = &second_request.kind else {
            panic!("effect preview selection should schedule an effect preview render");
        };
        assert!(ids.contains(&23));
        assert!(first_request.cancellation.is_cancelled());
    }

    #[test]
    fn native_audio_same_frame_ticks_do_not_reschedule_render() {
        let (project, mut document, key) = christmas_house_project_and_sequence();
        document.frame_rate = 144;
        let mut session = PlaybackSession::default();
        session.sync_source(
            Some((key, document)),
            Some(&project),
            PreviewSyncMode::RenderNow,
        );
        session.play_from_native_audio_clock(0.001, Some(&project));
        let request = session
            .begin_deferred_render()
            .expect("native audio play should schedule a preview render");

        session.render_at_native_audio_clock(0.002, false, Some(&project));

        assert!(!request.cancellation.is_cancelled());
        assert!(session.begin_deferred_render().is_none());
    }

    #[test]
    fn native_audio_frame_boundary_schedules_latest_frame_start() {
        let (project, mut document, key) = christmas_house_project_and_sequence();
        document.frame_rate = 144;
        let mut session = PlaybackSession::default();
        session.sync_source(
            Some((key, document)),
            Some(&project),
            PreviewSyncMode::RenderNow,
        );
        session.play_from_native_audio_clock(0.001, Some(&project));
        let first_request = session
            .begin_deferred_render()
            .expect("native audio play should schedule a preview render");

        session.render_at_native_audio_clock((1.0 / 144.0) + 0.0001, false, Some(&project));
        let second_request = session
            .begin_deferred_render()
            .expect("frame boundary should schedule the next preview render");

        assert!(first_request.cancellation.is_cancelled());
        let super::PlaybackRenderMode::FullSequenceFrame {
            position_seconds,
            frame_index,
        } = second_request.kind
        else {
            panic!("frame boundary should schedule a sequence frame render");
        };
        assert_eq!(frame_index, 1);
        assert_eq!(position_seconds, super::frame_start(1, 144));

        session.render_at_native_audio_clock((1.0 / 144.0) + 0.0002, false, Some(&project));
        assert!(!second_request.cancellation.is_cancelled());
        assert!(session.begin_deferred_render().is_none());
    }
}
