use std::collections::HashMap;
use std::time::Instant;

use dawn_project::analysis::ProjectAnalysis;
use dawn_project::document::SequenceDocument;
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
        document: SequenceDocument,
    },
}

#[derive(Debug, Clone)]
pub enum PreviewTransport {
    Stopped,
    Paused,
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
                self.source = PreviewSource::Sequence { key, document };
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

    pub fn pause(&mut self, analysis: Option<&ProjectAnalysis>) {
        self.pause_current(analysis);
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

    pub fn tick(&mut self, analysis: Option<&ProjectAnalysis>) {
        if !self.is_playing() {
            return;
        }
        if let Some((key, duration_ms)) = self.sequence_source_meta() {
            let position_ms = self.playing_position_ms().min(duration_ms);
            let state = self.sequence_states.entry(key).or_default();
            state.position_ms = position_ms;
            if position_ms >= duration_ms {
                self.transport = PreviewTransport::Stopped;
                self.render(analysis, "Sequence playback complete");
            } else {
                self.render(analysis, "Playing");
            }
        }
    }

    pub fn is_playing(&self) -> bool {
        matches!(self.transport, PreviewTransport::Playing { .. })
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
        let (source_label, source_key, position_ms, home_ms, duration_ms, frame) = match self
            .source
            .clone()
        {
            PreviewSource::None => (
                "No preview source".to_string(),
                None,
                0,
                0,
                0,
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
                    frame,
                )
            }
        };
        let frame_status = status_from_frame(&frame.status).unwrap_or(status);
        self.snapshot = PreviewSnapshot {
            source_label,
            source_key,
            is_playing: self.is_playing(),
            position_ms,
            home_ms,
            duration_ms,
            frame,
            status: frame_status,
        };
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
