use std::collections::HashMap;
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
    Ready,
    Playing,
    Ended,
    Error,
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
pub enum SequenceTransportSource {
    None,
    Sequence {
        key: SequenceKey,
        document: Arc<SequenceEditorDocument>,
    },
}

#[derive(Debug, Clone)]
pub enum SequenceTransport {
    Stopped,
    Paused,
    Playing {
        started_at: Instant,
        started_position_seconds: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SequenceTransportState {
    Stopped,
    Paused,
    Playing,
    Ended,
    Error,
}

impl SequenceTransportState {
    pub fn is_active_playback(self) -> bool {
        matches!(self, Self::Playing)
    }

    pub fn should_animate_position(self) -> bool {
        matches!(self, Self::Playing)
    }

    pub fn should_publish_continuously(self) -> bool {
        matches!(self, Self::Playing)
    }
}

#[derive(Debug, Clone)]
pub struct SequenceTransportSnapshot {
    pub source_label: String,
    pub source_key: Option<SequenceKey>,
    pub render_generation: u64,
    pub render_dirty_revision: u64,
    pub transport_state: SequenceTransportState,
    pub render_updating: bool,
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
pub enum SequenceTransportSyncMode {
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
    pub cancellation: RenderCancellationToken,
}

#[derive(Debug, Clone)]
pub enum PlaybackRenderMode {
    FullSequenceFrame {
        position_seconds: f64,
        frame_index: u64,
    },
}

#[derive(Debug, Clone)]
pub struct RenderCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl Default for RenderCancellationToken {
    fn default() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl RenderCancellationToken {
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

#[derive(Debug, Clone, Copy)]
pub struct NativeAudioClockProjection {
    pub position_seconds: f64,
    pub status: AudioPlaybackStatus,
    pub ended: bool,
}

#[derive(Debug, Clone)]
pub struct SequenceTransportSession {
    source: SequenceTransportSource,
    transport: SequenceTransport,
    sequence_states: HashMap<SequenceKey, SequencePlaybackState>,
    last_native_audio_frame_index: Option<u64>,
    last_render_timing: PlaybackRenderTiming,
    generation: u64,
    dirty_revision: u64,
    next_deferred_render_id: u64,
    pending_deferred_render: Option<PendingDeferredRender>,
    snapshot: SequenceTransportSnapshot,
}

#[derive(Debug, Clone)]
struct PendingDeferredRender {
    id: u64,
    dirty_revision: u64,
    generation: u64,
    cancellation: RenderCancellationToken,
    started: bool,
}

impl PartialEq for PendingDeferredRender {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.dirty_revision == other.dirty_revision
            && self.generation == other.generation
    }
}

impl Default for SequenceTransportSession {
    fn default() -> Self {
        let geometry = Arc::new(empty_geometry());
        let frame = empty_frame(&geometry, 0, "No sequence source");
        Self {
            source: SequenceTransportSource::None,
            transport: SequenceTransport::Stopped,
            sequence_states: HashMap::new(),
            last_native_audio_frame_index: None,
            last_render_timing: PlaybackRenderTiming::default(),
            generation: 0,
            dirty_revision: 0,
            next_deferred_render_id: 0,
            pending_deferred_render: None,
            snapshot: SequenceTransportSnapshot {
                source_label: "No sequence source".to_string(),
                source_key: None,
                render_generation: 0,
                render_dirty_revision: 0,
                transport_state: SequenceTransportState::Stopped,
                render_updating: false,
                position_seconds: 0.0,
                home_seconds: 0.0,
                duration_seconds: 0.0,
                audio: None,
                clock_source: "silent".to_string(),
                audio_playback_status: AudioPlaybackStatus::None,
                geometry,
                frame: Arc::new(frame),
                status: "No sequence source".to_string(),
            },
        }
    }
}

impl SequenceTransportSession {
    pub fn snapshot(&self) -> SequenceTransportSnapshot {
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
        mode: SequenceTransportSyncMode,
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
                self.source = SequenceTransportSource::Sequence {
                    key,
                    document: Arc::new(document),
                };
            }
            None => {
                self.source = SequenceTransportSource::None;
                self.transport = SequenceTransport::Stopped;
            }
        }
        match mode {
            SequenceTransportSyncMode::RenderNow | SequenceTransportSyncMode::DeferRender => {
                self.schedule_render(project, self.status_for_source())
            }
        }
    }

    pub fn play(&mut self, project: Option<&DawnProject>) {
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            self.transport = SequenceTransport::Stopped;
            self.schedule_render(project, "No sequence source");
            return;
        };

        let state = self.sequence_states.entry(key).or_default();
        if state.position_seconds >= duration_seconds {
            state.position_seconds = clamp_position_seconds(state.home_seconds, duration_seconds);
        }
        self.transport = SequenceTransport::Playing {
            started_at: Instant::now(),
            started_position_seconds: state.position_seconds,
        };
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
        self.transport = SequenceTransport::Paused;
        self.last_native_audio_frame_index = None;
        self.schedule_render(project, "Paused");
    }

    pub fn stop(&mut self, project: Option<&DawnProject>) {
        self.capture_position();
        self.transport = SequenceTransport::Stopped;
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
            self.transport = SequenceTransport::Playing {
                started_at: Instant::now(),
                started_position_seconds: position_seconds,
            };
        }
        self.schedule_render(project, "Ready");
    }

    pub fn set_playhead_home(&mut self, position_seconds: f64) {
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            return;
        };
        let state = self.sequence_states.entry(key).or_default();
        state.home_seconds = clamp_position_seconds(position_seconds, duration_seconds);
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
            self.transport = SequenceTransport::Playing {
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
        self.transport = SequenceTransport::Stopped;
        self.schedule_render(project, "Sequence returned to beginning");
    }

    pub fn tick(&mut self, project: Option<&DawnProject>) {
        if self.tick_clock() {
            self.schedule_render(project, self.snapshot.status.clone());
        }
    }

    pub fn tick_clock(&mut self) -> bool {
        if !self.is_playing() {
            return false;
        }
        if let Some((key, duration_seconds)) = self.sequence_source_meta() {
            let position_seconds =
                clamp_position_seconds(self.playing_position_seconds(), duration_seconds);
            let state = self.sequence_states.entry(key).or_default();
            state.position_seconds = position_seconds;
            if position_seconds >= duration_seconds {
                self.transport = SequenceTransport::Stopped;
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
        if !self.snapshot.render_updating {
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
        let SequenceTransportSource::Sequence { key, document } = self.source.clone() else {
            self.snapshot.render_updating = false;
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
        let frame_index = sequence_frame_index(position_seconds, document.frame_rate);
        let kind = PlaybackRenderMode::FullSequenceFrame {
            position_seconds: frame_start(frame_index, document.frame_rate),
            frame_index,
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
        self.snapshot.render_updating = false;
        true
    }

    pub fn apply_native_audio_clock(
        &mut self,
        clock: NativeAudioClockProjection,
        project: Option<&DawnProject>,
    ) {
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            self.schedule_render(project, "No active sequence");
            return;
        };
        let position_seconds = clamp_position_seconds(clock.position_seconds, duration_seconds);
        let frame_index = sequence_frame_index(position_seconds, self.target_fps());
        self.sequence_states
            .entry(key.clone())
            .or_default()
            .position_seconds = position_seconds;
        self.snapshot.clock_source = "nativeAudio".to_string();
        self.snapshot.audio_playback_status = clock.status;
        if clock.ended || position_seconds >= duration_seconds {
            self.transport = SequenceTransport::Stopped;
            self.last_native_audio_frame_index = None;
            self.schedule_render_with_audio_status(
                project,
                "Sequence playback complete",
                "nativeAudio",
                AudioPlaybackStatus::Ended,
            );
            return;
        }
        match clock.status {
            AudioPlaybackStatus::Ended => {
                self.transport = SequenceTransport::Stopped;
                self.last_native_audio_frame_index = None;
                self.schedule_render_with_audio_status(
                    project,
                    "Sequence playback complete",
                    "nativeAudio",
                    AudioPlaybackStatus::Ended,
                );
            }
            AudioPlaybackStatus::Playing => {
                self.transport = SequenceTransport::Paused;
                self.refresh_snapshot_metadata_with_audio_status(
                    "Playing",
                    "nativeAudio",
                    AudioPlaybackStatus::Playing,
                );
                if self.last_native_audio_frame_index != Some(frame_index) {
                    self.last_native_audio_frame_index = Some(frame_index);
                    self.schedule_render_with_audio_status(
                        project,
                        "Playing",
                        "nativeAudio",
                        AudioPlaybackStatus::Playing,
                    );
                }
            }
            AudioPlaybackStatus::Ready => {
                self.transport = SequenceTransport::Paused;
                self.last_native_audio_frame_index = None;
                self.schedule_render_with_audio_status(
                    project,
                    "Ready",
                    "nativeAudio",
                    AudioPlaybackStatus::Ready,
                );
            }
            AudioPlaybackStatus::Missing => {
                self.transport = SequenceTransport::Stopped;
                self.last_native_audio_frame_index = None;
                self.refresh_snapshot_metadata_with_audio_status(
                    "Audio missing",
                    "silent",
                    AudioPlaybackStatus::Missing,
                );
            }
            AudioPlaybackStatus::None => {
                self.transport = SequenceTransport::Stopped;
                self.last_native_audio_frame_index = None;
                self.refresh_snapshot_metadata_with_audio_status(
                    "Ready",
                    "silent",
                    AudioPlaybackStatus::None,
                );
            }
            AudioPlaybackStatus::Error => {
                self.transport = SequenceTransport::Paused;
                self.last_native_audio_frame_index = None;
                self.schedule_render_with_audio_status(
                    project,
                    "Audio error",
                    "nativeAudio",
                    AudioPlaybackStatus::Error,
                );
            }
        }
    }

    pub fn is_playing(&self) -> bool {
        matches!(self.transport, SequenceTransport::Playing { .. })
    }

    pub fn transport_state(&self) -> SequenceTransportState {
        match self.snapshot.audio_playback_status {
            AudioPlaybackStatus::Playing => SequenceTransportState::Playing,
            AudioPlaybackStatus::Ended => SequenceTransportState::Ended,
            AudioPlaybackStatus::Error => SequenceTransportState::Error,
            _ => match self.transport {
                SequenceTransport::Playing { .. } => SequenceTransportState::Playing,
                SequenceTransport::Paused => SequenceTransportState::Paused,
                SequenceTransport::Stopped => SequenceTransportState::Stopped,
            },
        }
    }

    pub fn target_fps(&self) -> u32 {
        match &self.source {
            SequenceTransportSource::Sequence { document, .. } => document.frame_rate.max(1),
            SequenceTransportSource::None => 30,
        }
    }

    fn pause_current(&mut self, project: Option<&DawnProject>) {
        self.capture_position();
        if self.is_playing() {
            self.transport = SequenceTransport::Paused;
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
            SequenceTransportSource::None => {
                let geometry = Arc::new(empty_geometry());
                let frame = Arc::new(empty_frame(&geometry, self.generation, status.clone()));
                (
                    "No sequence source".to_string(),
                    None,
                    0.0,
                    0.0,
                    0.0,
                    None,
                    geometry,
                    frame,
                )
            }
            SequenceTransportSource::Sequence { key, document } => {
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
            timing_status_for(audio.as_ref(), self.snapshot.audio_playback_status);
        self.snapshot = SequenceTransportSnapshot {
            source_label,
            source_key,
            render_generation: self.generation,
            render_dirty_revision: self.dirty_revision,
            transport_state: self.transport_state_for(audio_playback_status),
            render_updating: false,
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
        if matches!(self.source, SequenceTransportSource::Sequence { .. }) && project.is_some() {
            self.pending_deferred_render = Some(PendingDeferredRender {
                id: self.next_deferred_render_id,
                dirty_revision: self.dirty_revision,
                generation: self.generation,
                cancellation: RenderCancellationToken::default(),
                started: false,
            });
            self.snapshot.render_updating = true;
        }
    }

    fn schedule_render_with_audio_status(
        &mut self,
        project: Option<&DawnProject>,
        status: impl Into<String>,
        clock_source: impl Into<String>,
        audio_playback_status: AudioPlaybackStatus,
    ) {
        self.schedule_render(project, status);
        self.snapshot.clock_source = clock_source.into();
        self.snapshot.audio_playback_status = audio_playback_status;
        self.snapshot.transport_state = self.transport_state_for(audio_playback_status);
    }

    fn refresh_snapshot_metadata(&mut self, status: impl Into<String>) {
        let status = status.into();
        match &self.source {
            SequenceTransportSource::None => {
                self.snapshot.source_label = "No sequence source".to_string();
                self.snapshot.source_key = None;
                self.snapshot.transport_state = SequenceTransportState::Stopped;
                self.snapshot.render_updating = false;
                self.snapshot.position_seconds = 0.0;
                self.snapshot.home_seconds = 0.0;
                self.snapshot.duration_seconds = 0.0;
                self.snapshot.audio = None;
                self.snapshot.clock_source = "silent".to_string();
                self.snapshot.audio_playback_status = AudioPlaybackStatus::None;
                self.snapshot.status = status;
            }
            SequenceTransportSource::Sequence { key, document } => {
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
                let (clock_source, audio_playback_status) = timing_status_for(
                    self.snapshot.audio.as_ref(),
                    self.snapshot.audio_playback_status,
                );
                self.snapshot.clock_source = clock_source;
                self.snapshot.audio_playback_status = audio_playback_status;
                self.snapshot.transport_state = self.transport_state_for(audio_playback_status);
                self.snapshot.status = status;
            }
        }
    }

    fn refresh_snapshot_metadata_with_audio_status(
        &mut self,
        status: impl Into<String>,
        clock_source: impl Into<String>,
        audio_playback_status: AudioPlaybackStatus,
    ) {
        self.refresh_snapshot_metadata(status);
        self.snapshot.clock_source = clock_source.into();
        self.snapshot.audio_playback_status = audio_playback_status;
        self.snapshot.transport_state = self.transport_state_for(audio_playback_status);
    }

    fn transport_state_for(
        &self,
        audio_playback_status: AudioPlaybackStatus,
    ) -> SequenceTransportState {
        match audio_playback_status {
            AudioPlaybackStatus::Playing => SequenceTransportState::Playing,
            AudioPlaybackStatus::Ended => SequenceTransportState::Ended,
            AudioPlaybackStatus::Error => SequenceTransportState::Error,
            _ => match self.transport {
                SequenceTransport::Playing { .. } => SequenceTransportState::Playing,
                SequenceTransport::Paused => SequenceTransportState::Paused,
                SequenceTransport::Stopped => SequenceTransportState::Stopped,
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
            SequenceTransport::Playing {
                started_at,
                started_position_seconds,
            } => started_position_seconds + started_at.elapsed().as_secs_f64(),
            SequenceTransport::Stopped | SequenceTransport::Paused => self
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
            SequenceTransportSource::Sequence { key, document } => {
                Some((key.clone(), document.duration_seconds))
            }
            SequenceTransportSource::None => None,
        }
    }

    fn current_key(&self) -> Option<SequenceKey> {
        match &self.source {
            SequenceTransportSource::Sequence { key, .. } => Some(key.clone()),
            SequenceTransportSource::None => None,
        }
    }

    fn status_for_source(&self) -> &'static str {
        match self.source {
            SequenceTransportSource::None => "No sequence source",
            SequenceTransportSource::Sequence { .. } => "Ready",
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
    current_status: AudioPlaybackStatus,
) -> (String, AudioPlaybackStatus) {
    match audio {
        Some(audio) if audio.exists => (
            "nativeAudio".to_string(),
            native_audio_status(current_status),
        ),
        Some(_) => ("silent".to_string(), AudioPlaybackStatus::Missing),
        None => ("silent".to_string(), AudioPlaybackStatus::None),
    }
}

fn native_audio_status(current_status: AudioPlaybackStatus) -> AudioPlaybackStatus {
    match current_status {
        AudioPlaybackStatus::Ready
        | AudioPlaybackStatus::Playing
        | AudioPlaybackStatus::Ended
        | AudioPlaybackStatus::Error => current_status,
        AudioPlaybackStatus::None | AudioPlaybackStatus::Missing => AudioPlaybackStatus::Ready,
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

    use super::{
        AudioPlaybackStatus, NativeAudioClockProjection, SequenceKey, SequenceTransportSession,
        SequenceTransportSyncMode,
    };

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
        let mut session = SequenceTransportSession::default();
        session.sync_source(
            Some((key.clone(), document)),
            Some(&project),
            SequenceTransportSyncMode::RenderNow,
        );
        session.seek(2.0, Some(&project));
        let first_request = session
            .begin_deferred_render()
            .expect("initial source should schedule sequence render");

        let edited_document = edited_sequence_document(&first_request.document);
        let edited_key = key.clone();
        assert_eq!(key, edited_key);
        session.sync_source(
            Some((edited_key, edited_document)),
            Some(&project),
            SequenceTransportSyncMode::RenderNow,
        );
        let edited_request = session
            .begin_deferred_render()
            .expect("edited source should schedule sequence render");

        assert_ne!(first_request.dirty_revision, edited_request.dirty_revision);
        assert_ne!(first_request.generation, edited_request.generation);
        assert!(first_request.cancellation.is_cancelled());
        assert_eq!(edited_request.key, key);
    }

    #[test]
    fn native_audio_same_frame_ticks_do_not_reschedule_render() {
        let (project, mut document, key) = christmas_house_project_and_sequence();
        document.frame_rate = 144;
        let mut session = SequenceTransportSession::default();
        session.sync_source(
            Some((key, document)),
            Some(&project),
            SequenceTransportSyncMode::RenderNow,
        );
        session.apply_native_audio_clock(
            NativeAudioClockProjection {
                position_seconds: 0.001,
                status: AudioPlaybackStatus::Playing,
                ended: false,
            },
            Some(&project),
        );
        let request = session
            .begin_deferred_render()
            .expect("native audio play should schedule a sequence render");

        session.apply_native_audio_clock(
            NativeAudioClockProjection {
                position_seconds: 0.002,
                status: AudioPlaybackStatus::Playing,
                ended: false,
            },
            Some(&project),
        );

        assert!(!request.cancellation.is_cancelled());
        assert!(session.begin_deferred_render().is_none());
    }

    #[test]
    fn native_audio_frame_boundary_schedules_latest_frame_start() {
        let (project, mut document, key) = christmas_house_project_and_sequence();
        document.frame_rate = 144;
        let mut session = SequenceTransportSession::default();
        session.sync_source(
            Some((key, document)),
            Some(&project),
            SequenceTransportSyncMode::RenderNow,
        );
        session.apply_native_audio_clock(
            NativeAudioClockProjection {
                position_seconds: 0.001,
                status: AudioPlaybackStatus::Playing,
                ended: false,
            },
            Some(&project),
        );
        let first_request = session
            .begin_deferred_render()
            .expect("native audio play should schedule a sequence render");

        session.apply_native_audio_clock(
            NativeAudioClockProjection {
                position_seconds: (1.0 / 144.0) + 0.0001,
                status: AudioPlaybackStatus::Playing,
                ended: false,
            },
            Some(&project),
        );
        let second_request = session
            .begin_deferred_render()
            .expect("frame boundary should schedule the next sequence render");

        assert!(first_request.cancellation.is_cancelled());
        let super::PlaybackRenderMode::FullSequenceFrame {
            position_seconds,
            frame_index,
        } = second_request.kind;
        assert_eq!(frame_index, 1);
        assert_eq!(position_seconds, super::frame_start(1, 144));

        session.apply_native_audio_clock(
            NativeAudioClockProjection {
                position_seconds: (1.0 / 144.0) + 0.0002,
                status: AudioPlaybackStatus::Playing,
                ended: false,
            },
            Some(&project),
        );
        assert!(!second_request.cancellation.is_cancelled());
        assert!(session.begin_deferred_render().is_none());
    }
}
