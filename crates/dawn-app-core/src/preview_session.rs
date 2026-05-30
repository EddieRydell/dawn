use std::collections::HashMap;
use std::time::Instant;

use dawn_project::analysis::ProjectAnalysis;
use dawn_project::document::{SequenceAudioDocument, SequenceDocument};
use dawn_project::path::Utf8PathBuf;

use crate::output_runtime::{empty_frame, evaluate_sequence_frame, OutputFrame, OutputFrameStatus};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SequenceKey {
    pub path: Utf8PathBuf,
    pub object_key: String,
}

#[derive(Debug, Clone, Default)]
pub struct SequencePlaybackState {
    pub position_ms: u64,
    pub home_ms: u64,
}

#[derive(Debug, Clone)]
pub enum PreviewSource {
    None,
    Sequence {
        key: SequenceKey,
        document: Box<SequenceDocument>,
    },
}

#[derive(Debug, Clone)]
pub enum PreviewTransport {
    Stopped,
    Paused,
    NativeAudioPlaying,
    Playing {
        started_at: Instant,
        started_position_ms: u64,
    },
}

#[derive(Debug, Clone)]
pub struct PreviewSnapshot {
    pub source_label: String,
    pub source_key: Option<SequenceKey>,
    pub is_playing: bool,
    pub position_ms: u64,
    pub home_ms: u64,
    pub duration_ms: u64,
    pub audio: Option<SequenceAudioDocument>,
    pub clock_source: String,
    pub audio_playback_status: String,
    pub frame: OutputFrame,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct PreviewSession {
    source: PreviewSource,
    transport: PreviewTransport,
    sequence_states: HashMap<SequenceKey, SequencePlaybackState>,
    generation: u64,
    snapshot: PreviewSnapshot,
}

impl Default for PreviewSession {
    fn default() -> Self {
        let frame = empty_frame(0, "No sequence preview source");
        Self {
            source: PreviewSource::None,
            transport: PreviewTransport::Stopped,
            sequence_states: HashMap::new(),
            generation: 0,
            snapshot: PreviewSnapshot {
                source_label: "No preview source".to_string(),
                source_key: None,
                is_playing: false,
                position_ms: 0,
                home_ms: 0,
                duration_ms: 0,
                audio: None,
                clock_source: "silent".to_string(),
                audio_playback_status: "noAudio".to_string(),
                frame,
                status: "No sequence preview source".to_string(),
            },
        }
    }
}

impl PreviewSession {
    pub fn snapshot(&self) -> PreviewSnapshot {
        self.snapshot.clone()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn sync_source(
        &mut self,
        source: Option<(SequenceKey, SequenceDocument)>,
        analysis: Option<&ProjectAnalysis>,
    ) {
        let next_key = source.as_ref().map(|(key, _)| key);
        let source_changed = self.current_key().as_ref() != next_key;
        if source_changed && self.is_playing() {
            self.pause_current(analysis);
        }

        match source {
            Some((key, document)) => {
                self.sequence_states.entry(key.clone()).or_default();
                self.source = PreviewSource::Sequence {
                    key,
                    document: Box::new(document),
                };
            }
            None => {
                self.source = PreviewSource::None;
                self.transport = PreviewTransport::Stopped;
            }
        }
        self.render(analysis, self.status_for_source());
    }

    pub fn play(&mut self, analysis: Option<&ProjectAnalysis>) {
        let Some((key, duration_ms)) = self.sequence_source_meta() else {
            self.transport = PreviewTransport::Stopped;
            self.render(analysis, "No sequence preview source");
            return;
        };

        let state = self.sequence_states.entry(key).or_default();
        if state.position_ms >= duration_ms {
            state.position_ms = state.home_ms.min(duration_ms);
        }
        self.transport = PreviewTransport::Playing {
            started_at: Instant::now(),
            started_position_ms: state.position_ms,
        };
        self.render(analysis, "Playing");
    }

    pub fn play_from_native_audio_clock(
        &mut self,
        position_ms: u64,
        analysis: Option<&ProjectAnalysis>,
    ) {
        let Some((key, duration_ms)) = self.sequence_source_meta() else {
            self.transport = PreviewTransport::Stopped;
            self.render(analysis, "No sequence preview source");
            return;
        };
        let state = self.sequence_states.entry(key).or_default();
        state.position_ms = position_ms.min(duration_ms);
        self.transport = PreviewTransport::NativeAudioPlaying;
        self.render(analysis, "Playing");
    }

    pub fn pause(&mut self, analysis: Option<&ProjectAnalysis>) {
        self.pause_current(analysis);
        self.render(analysis, "Paused");
    }

    pub fn pause_at(&mut self, position_ms: u64, analysis: Option<&ProjectAnalysis>) {
        let Some((key, duration_ms)) = self.sequence_source_meta() else {
            self.render(analysis, "No active sequence");
            return;
        };
        let state = self.sequence_states.entry(key).or_default();
        state.position_ms = position_ms.min(duration_ms);
        self.transport = PreviewTransport::Paused;
        self.render(analysis, "Paused");
    }

    pub fn stop(&mut self, analysis: Option<&ProjectAnalysis>) {
        self.capture_position();
        self.transport = PreviewTransport::Stopped;
        if let Some((key, duration_ms)) = self.sequence_source_meta() {
            let state = self.sequence_states.entry(key).or_default();
            state.position_ms = state.home_ms.min(duration_ms);
        }
        self.render(analysis, "Stopped");
    }

    pub fn stop_native_audio(&mut self, analysis: Option<&ProjectAnalysis>) {
        self.transport = PreviewTransport::Stopped;
        if let Some((key, duration_ms)) = self.sequence_source_meta() {
            let state = self.sequence_states.entry(key).or_default();
            state.position_ms = state.home_ms.min(duration_ms);
        }
        self.render(analysis, "Stopped");
    }

    pub fn seek(&mut self, position_ms: u64, analysis: Option<&ProjectAnalysis>) {
        let Some((key, duration_ms)) = self.sequence_source_meta() else {
            self.render(analysis, "No active sequence");
            return;
        };
        let position_ms = position_ms.min(duration_ms);
        let state = self.sequence_states.entry(key).or_default();
        state.position_ms = position_ms;
        state.home_ms = position_ms;
        if self.is_playing() {
            self.transport = PreviewTransport::Playing {
                started_at: Instant::now(),
                started_position_ms: position_ms,
            };
        }
        self.render(analysis, "Ready");
    }

    pub fn seek_native_audio(
        &mut self,
        position_ms: u64,
        playing: bool,
        analysis: Option<&ProjectAnalysis>,
    ) {
        let Some((key, duration_ms)) = self.sequence_source_meta() else {
            self.render(analysis, "No active sequence");
            return;
        };
        let position_ms = position_ms.min(duration_ms);
        let state = self.sequence_states.entry(key).or_default();
        state.position_ms = position_ms;
        state.home_ms = position_ms;
        self.transport = if playing {
            PreviewTransport::NativeAudioPlaying
        } else {
            PreviewTransport::Paused
        };
        self.render(analysis, "Ready");
    }

    pub fn set_sequence_playhead(&mut self, time_ms: u64, analysis: Option<&ProjectAnalysis>) {
        let Some((key, duration_ms)) = self.sequence_source_meta() else {
            self.render(analysis, "No active sequence");
            return;
        };
        let position_ms = time_ms.min(duration_ms);
        let state = self.sequence_states.entry(key).or_default();
        state.position_ms = position_ms;
        state.home_ms = position_ms;
        if self.is_playing() {
            self.transport = PreviewTransport::Playing {
                started_at: Instant::now(),
                started_position_ms: position_ms,
            };
        }
        self.render(analysis, "Sequence playhead moved");
    }

    pub fn go_to_sequence_beginning(&mut self, analysis: Option<&ProjectAnalysis>) {
        let Some((key, _)) = self.sequence_source_meta() else {
            self.render(analysis, "No active sequence");
            return;
        };
        let state = self.sequence_states.entry(key).or_default();
        state.position_ms = 0;
        state.home_ms = 0;
        self.transport = PreviewTransport::Stopped;
        self.render(analysis, "Sequence returned to beginning");
    }

    pub fn go_to_sequence_beginning_native_audio(&mut self, analysis: Option<&ProjectAnalysis>) {
        let Some((key, _)) = self.sequence_source_meta() else {
            self.render(analysis, "No active sequence");
            return;
        };
        let state = self.sequence_states.entry(key).or_default();
        state.position_ms = 0;
        state.home_ms = 0;
        self.transport = PreviewTransport::Paused;
        self.render(analysis, "Sequence returned to beginning");
    }

    pub fn tick(&mut self, analysis: Option<&ProjectAnalysis>) {
        if self.tick_clock() {
            self.render(analysis, self.snapshot.status.clone());
        }
    }

    pub fn tick_clock(&mut self) -> bool {
        if !self.is_playing() || matches!(self.transport, PreviewTransport::NativeAudioPlaying) {
            return false;
        }
        if let Some((key, duration_ms)) = self.sequence_source_meta() {
            let position_ms = self.playing_position_ms().min(duration_ms);
            let state = self.sequence_states.entry(key).or_default();
            state.position_ms = position_ms;
            if position_ms >= duration_ms {
                self.transport = PreviewTransport::Stopped;
                self.refresh_snapshot_metadata("Sequence playback complete");
            } else {
                self.refresh_snapshot_metadata("Playing");
            }
            return true;
        }
        false
    }

    pub fn render_current_frame(&mut self, analysis: Option<&ProjectAnalysis>) {
        let status = self.snapshot.status.clone();
        self.render(analysis, status);
    }

    pub fn render_at_native_audio_clock(
        &mut self,
        position_ms: u64,
        ended: bool,
        analysis: Option<&ProjectAnalysis>,
    ) {
        let Some((key, duration_ms)) = self.sequence_source_meta() else {
            self.render(analysis, "No active sequence");
            return;
        };
        let position_ms = position_ms.min(duration_ms);
        self.sequence_states.entry(key).or_default().position_ms = position_ms;
        if ended || position_ms >= duration_ms {
            self.transport = PreviewTransport::Stopped;
            self.render(analysis, "Sequence playback complete");
        } else {
            self.transport = PreviewTransport::NativeAudioPlaying;
            self.render(analysis, "Playing");
        }
    }

    pub fn set_timing_status(
        &mut self,
        clock_source: impl Into<String>,
        audio_playback_status: impl Into<String>,
    ) {
        let clock_source = clock_source.into();
        let audio_playback_status = audio_playback_status.into();
        self.snapshot.clock_source = clock_source;
        self.snapshot.audio_playback_status = audio_playback_status;
    }

    pub fn is_playing(&self) -> bool {
        matches!(
            self.transport,
            PreviewTransport::Playing { .. } | PreviewTransport::NativeAudioPlaying
        )
    }

    pub fn target_fps(&self) -> u32 {
        match &self.source {
            PreviewSource::Sequence { document, .. } => document.frame_rate.clamp(1, 60),
            PreviewSource::None => 30,
        }
    }

    fn pause_current(&mut self, analysis: Option<&ProjectAnalysis>) {
        self.capture_position();
        if self.is_playing() {
            self.transport = PreviewTransport::Paused;
            self.render(analysis, "Paused");
        }
    }

    fn capture_position(&mut self) {
        let Some((key, duration_ms)) = self.sequence_source_meta() else {
            return;
        };
        let position_ms = self.playing_position_ms().min(duration_ms);
        self.sequence_states.entry(key).or_default().position_ms = position_ms;
    }

    fn render(&mut self, analysis: Option<&ProjectAnalysis>, status: impl Into<String>) {
        self.generation = self.generation.saturating_add(1);
        let status = status.into();
        let (source_label, source_key, position_ms, home_ms, duration_ms, audio, frame) = match self
            .source
            .clone()
        {
            PreviewSource::None => (
                "No preview source".to_string(),
                None,
                0,
                0,
                0,
                None,
                empty_frame(self.generation, status.clone()),
            ),
            PreviewSource::Sequence { key, document } => {
                let duration_ms = document.duration_ms;
                let position_ms = self.current_position_ms(&key, duration_ms);
                let home_ms = self
                    .sequence_states
                    .get(&key)
                    .map(|state| state.home_ms.min(duration_ms))
                    .unwrap_or_default();
                let frame = match analysis {
                    Some(analysis) => {
                        evaluate_sequence_frame(analysis, &document, position_ms, self.generation)
                    }
                    None => empty_frame(self.generation, "No project analysis"),
                };
                (
                    format!("Sequence {}", document.object_key),
                    Some(key),
                    position_ms,
                    home_ms,
                    duration_ms,
                    document.audio,
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
            is_playing: self.is_playing(),
            position_ms,
            home_ms,
            duration_ms,
            audio,
            clock_source,
            audio_playback_status,
            frame,
            status: frame_status,
        };
    }

    fn refresh_snapshot_metadata(&mut self, status: impl Into<String>) {
        let status = status.into();
        match &self.source {
            PreviewSource::None => {
                self.snapshot.source_label = "No preview source".to_string();
                self.snapshot.source_key = None;
                self.snapshot.is_playing = false;
                self.snapshot.position_ms = 0;
                self.snapshot.home_ms = 0;
                self.snapshot.duration_ms = 0;
                self.snapshot.audio = None;
                self.snapshot.clock_source = "silent".to_string();
                self.snapshot.audio_playback_status = "noAudio".to_string();
                self.snapshot.status = status;
            }
            PreviewSource::Sequence { key, document } => {
                let duration_ms = document.duration_ms;
                self.snapshot.source_label = format!("Sequence {}", document.object_key);
                self.snapshot.source_key = Some(key.clone());
                self.snapshot.is_playing = self.is_playing();
                self.snapshot.position_ms = self.current_position_ms(key, duration_ms);
                self.snapshot.home_ms = self
                    .sequence_states
                    .get(key)
                    .map(|state| state.home_ms.min(duration_ms))
                    .unwrap_or_default();
                self.snapshot.duration_ms = duration_ms;
                self.snapshot.audio = document.audio.clone();
                let (clock_source, audio_playback_status) =
                    timing_status_for(self.snapshot.audio.as_ref(), self.is_playing());
                self.snapshot.clock_source = clock_source;
                self.snapshot.audio_playback_status = audio_playback_status;
                self.snapshot.status = status;
            }
        }
    }

    fn current_position_ms(&self, key: &SequenceKey, duration_ms: u64) -> u64 {
        if self.is_playing() && self.current_key().as_ref() == Some(key) {
            self.playing_position_ms().min(duration_ms)
        } else {
            self.sequence_states
                .get(key)
                .map(|state| state.position_ms.min(duration_ms))
                .unwrap_or_default()
        }
    }

    fn playing_position_ms(&self) -> u64 {
        match self.transport {
            PreviewTransport::NativeAudioPlaying => self
                .current_key()
                .and_then(|key| {
                    self.sequence_states
                        .get(&key)
                        .map(|state| state.position_ms)
                })
                .unwrap_or_default(),
            PreviewTransport::Playing {
                started_at,
                started_position_ms,
            } => started_position_ms.saturating_add(started_at.elapsed().as_millis() as u64),
            PreviewTransport::Stopped | PreviewTransport::Paused => self
                .current_key()
                .and_then(|key| {
                    self.sequence_states
                        .get(&key)
                        .map(|state| state.position_ms)
                })
                .unwrap_or_default(),
        }
    }

    fn sequence_source_meta(&self) -> Option<(SequenceKey, u64)> {
        match &self.source {
            PreviewSource::Sequence { key, document } => Some((key.clone(), document.duration_ms)),
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

fn status_from_frame(status: &OutputFrameStatus) -> Option<String> {
    match status {
        OutputFrameStatus::Live => None,
        OutputFrameStatus::Idle(message) | OutputFrameStatus::Error(message) => {
            Some(message.clone())
        }
    }
}

fn timing_status_for(audio: Option<&SequenceAudioDocument>, is_playing: bool) -> (String, String) {
    match audio {
        Some(audio) if audio.exists => (
            "nativeAudio".to_string(),
            if is_playing { "playing" } else { "ready" }.to_string(),
        ),
        Some(_) => ("silent".to_string(), "missingAudio".to_string()),
        None => ("silent".to_string(), "noAudio".to_string()),
    }
}
