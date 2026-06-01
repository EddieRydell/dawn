use std::collections::{HashMap, HashSet};
use std::time::Instant;

use dawn_project::analysis::ProjectAnalysis;
use dawn_project::document::{SequenceAudioDocument, SequenceDocument};
use dawn_project::path::Utf8PathBuf;

use crate::output_runtime::{
    empty_frame, OutputFrame, OutputFrameStatus, SequenceFrameEvaluationTiming,
    SequenceFrameEvaluator,
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
        started_position_seconds: f64,
    },
}

#[derive(Debug, Clone)]
pub struct PreviewSnapshot {
    pub source_label: String,
    pub source_key: Option<SequenceKey>,
    pub is_playing: bool,
    pub position_seconds: f64,
    pub home_seconds: f64,
    pub duration_seconds: f64,
    pub audio: Option<SequenceAudioDocument>,
    pub clock_source: String,
    pub audio_playback_status: AudioPlaybackStatus,
    pub effect_preview_active: bool,
    pub frame: OutputFrame,
    pub status: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PreviewRenderTiming {
    pub total_ms: f64,
    pub renderer_build_ms: f64,
    pub frame_evaluate_ms: f64,
    pub fixture_clone_ms: f64,
    pub effect_loop_ms: f64,
    pub output_frame_ms: f64,
    pub active_effects: u32,
    pub sampled_pixels: u32,
}

#[derive(Debug, Clone)]
struct EffectPreviewState {
    ids: HashSet<u32>,
    started_at: Instant,
}

#[derive(Debug, Clone)]
pub struct PreviewSession {
    source: PreviewSource,
    transport: PreviewTransport,
    sequence_states: HashMap<SequenceKey, SequencePlaybackState>,
    effect_preview: Option<EffectPreviewState>,
    render_cache: Option<SequenceFrameEvaluator>,
    last_render_timing: PreviewRenderTiming,
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
            effect_preview: None,
            render_cache: None,
            last_render_timing: PreviewRenderTiming::default(),
            generation: 0,
            snapshot: PreviewSnapshot {
                source_label: "No preview source".to_string(),
                source_key: None,
                is_playing: false,
                position_seconds: 0.0,
                home_seconds: 0.0,
                duration_seconds: 0.0,
                audio: None,
                clock_source: "silent".to_string(),
                audio_playback_status: AudioPlaybackStatus::None,
                effect_preview_active: false,
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

    pub fn last_render_timing(&self) -> PreviewRenderTiming {
        self.last_render_timing
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
        self.render_cache = None;

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
        self.effect_preview = None;
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            self.transport = PreviewTransport::Stopped;
            self.render(analysis, "No sequence preview source");
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
        self.render(analysis, "Playing");
    }

    pub fn play_from_native_audio_clock(
        &mut self,
        position_seconds: f64,
        analysis: Option<&ProjectAnalysis>,
    ) {
        self.effect_preview = None;
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            self.transport = PreviewTransport::Stopped;
            self.render(analysis, "No sequence preview source");
            return;
        };
        let state = self.sequence_states.entry(key).or_default();
        state.position_seconds = clamp_position_seconds(position_seconds, duration_seconds);
        self.transport = PreviewTransport::NativeAudioPlaying;
        self.render(analysis, "Playing");
    }

    pub fn pause(&mut self, analysis: Option<&ProjectAnalysis>) {
        self.pause_current(analysis);
        self.render(analysis, "Paused");
    }

    pub fn pause_at(&mut self, position_seconds: f64, analysis: Option<&ProjectAnalysis>) {
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            self.render(analysis, "No active sequence");
            return;
        };
        let state = self.sequence_states.entry(key).or_default();
        state.position_seconds = clamp_position_seconds(position_seconds, duration_seconds);
        self.transport = PreviewTransport::Paused;
        self.render(analysis, "Paused");
    }

    pub fn stop(&mut self, analysis: Option<&ProjectAnalysis>) {
        self.capture_position();
        self.transport = PreviewTransport::Stopped;
        if let Some((key, duration_seconds)) = self.sequence_source_meta() {
            let state = self.sequence_states.entry(key).or_default();
            state.position_seconds = clamp_position_seconds(state.home_seconds, duration_seconds);
        }
        self.render(analysis, "Stopped");
    }

    pub fn stop_native_audio(&mut self, analysis: Option<&ProjectAnalysis>) {
        self.transport = PreviewTransport::Stopped;
        if let Some((key, duration_seconds)) = self.sequence_source_meta() {
            let state = self.sequence_states.entry(key).or_default();
            state.position_seconds = clamp_position_seconds(state.home_seconds, duration_seconds);
        }
        self.render(analysis, "Stopped");
    }

    pub fn seek(&mut self, position_seconds: f64, analysis: Option<&ProjectAnalysis>) {
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            self.render(analysis, "No active sequence");
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
        self.render(analysis, "Ready");
    }

    pub fn seek_native_audio(
        &mut self,
        position_seconds: f64,
        playing: bool,
        analysis: Option<&ProjectAnalysis>,
    ) {
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            self.render(analysis, "No active sequence");
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
        self.render(analysis, "Ready");
    }

    pub fn set_sequence_playhead(&mut self, time_seconds: f64, analysis: Option<&ProjectAnalysis>) {
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            self.render(analysis, "No active sequence");
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
        self.render(analysis, "Sequence playhead moved");
    }

    pub fn go_to_sequence_beginning(&mut self, analysis: Option<&ProjectAnalysis>) {
        let Some((key, _)) = self.sequence_source_meta() else {
            self.render(analysis, "No active sequence");
            return;
        };
        let state = self.sequence_states.entry(key).or_default();
        state.position_seconds = 0.0;
        state.home_seconds = 0.0;
        self.transport = PreviewTransport::Stopped;
        self.render(analysis, "Sequence returned to beginning");
    }

    pub fn go_to_sequence_beginning_native_audio(&mut self, analysis: Option<&ProjectAnalysis>) {
        let Some((key, _)) = self.sequence_source_meta() else {
            self.render(analysis, "No active sequence");
            return;
        };
        let state = self.sequence_states.entry(key).or_default();
        state.position_seconds = 0.0;
        state.home_seconds = 0.0;
        self.transport = PreviewTransport::Paused;
        self.render(analysis, "Sequence returned to beginning");
    }

    pub fn tick(&mut self, analysis: Option<&ProjectAnalysis>) {
        if self.tick_clock() {
            self.render(analysis, self.snapshot.status.clone());
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

    pub fn render_current_frame(&mut self, analysis: Option<&ProjectAnalysis>) {
        let status = self.snapshot.status.clone();
        self.render(analysis, status);
    }

    pub fn set_effect_preview_ids(&mut self, ids: Vec<u32>, analysis: Option<&ProjectAnalysis>) {
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
        self.render(analysis, status);
    }

    pub fn clear_effect_preview(&mut self, analysis: Option<&ProjectAnalysis>) {
        self.effect_preview = None;
        let status = self.snapshot.status.clone();
        self.render(analysis, status);
    }

    pub fn render_at_native_audio_clock(
        &mut self,
        position_seconds: f64,
        ended: bool,
        analysis: Option<&ProjectAnalysis>,
    ) {
        let Some((key, duration_seconds)) = self.sequence_source_meta() else {
            self.render(analysis, "No active sequence");
            return;
        };
        let position_seconds = clamp_position_seconds(position_seconds, duration_seconds);
        self.sequence_states
            .entry(key)
            .or_default()
            .position_seconds = position_seconds;
        if ended || position_seconds >= duration_seconds {
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
        audio_playback_status: AudioPlaybackStatus,
    ) {
        let clock_source = clock_source.into();
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
            PreviewSource::Sequence { document, .. } => document.frame_rate.max(1),
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

    fn render(&mut self, analysis: Option<&ProjectAnalysis>, status: impl Into<String>) {
        let render_started = Instant::now();
        self.last_render_timing = PreviewRenderTiming::default();
        self.generation = self.generation.saturating_add(1);
        let status = status.into();
        let (
            source_label,
            source_key,
            position_seconds,
            home_seconds,
            duration_seconds,
            audio,
            frame,
        ) = match self.source.clone() {
            PreviewSource::None => (
                "No preview source".to_string(),
                None,
                0.0,
                0.0,
                0.0,
                None,
                empty_frame(self.generation, status.clone()),
            ),
            PreviewSource::Sequence { key, document } => {
                let duration_seconds = document.duration_seconds;
                let position_seconds = self.current_position_seconds(&key, duration_seconds);
                let home_seconds = self
                    .sequence_states
                    .get(&key)
                    .map(|state| clamp_position_seconds(state.home_seconds, duration_seconds))
                    .unwrap_or_default();
                let effect_preview = self.effect_preview.clone();
                let frame = match analysis {
                    Some(analysis) => match effect_preview.as_ref() {
                        Some(effect_preview) => self.render_effect_preview_frame(
                            analysis,
                            &document,
                            effect_preview.started_at.elapsed().as_secs_f64(),
                            &effect_preview.ids,
                        ),
                        None => self.render_sequence_frame(analysis, &document, position_seconds),
                    },
                    None => empty_frame(self.generation, "No project analysis"),
                };
                (
                    format!("Sequence {}", document.object_key),
                    Some(key),
                    position_seconds,
                    home_seconds,
                    duration_seconds,
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
            position_seconds,
            home_seconds,
            duration_seconds,
            audio,
            clock_source,
            audio_playback_status,
            effect_preview_active: self.effect_preview.is_some(),
            frame,
            status: frame_status,
        };
        self.last_render_timing.total_ms = elapsed_ms(render_started);
    }

    fn refresh_snapshot_metadata(&mut self, status: impl Into<String>) {
        let status = status.into();
        match &self.source {
            PreviewSource::None => {
                self.snapshot.source_label = "No preview source".to_string();
                self.snapshot.source_key = None;
                self.snapshot.is_playing = false;
                self.snapshot.position_seconds = 0.0;
                self.snapshot.home_seconds = 0.0;
                self.snapshot.duration_seconds = 0.0;
                self.snapshot.audio = None;
                self.snapshot.clock_source = "silent".to_string();
                self.snapshot.audio_playback_status = AudioPlaybackStatus::None;
                self.snapshot.effect_preview_active = false;
                self.snapshot.status = status;
            }
            PreviewSource::Sequence { key, document } => {
                let duration_seconds = document.duration_seconds;
                self.snapshot.source_label = format!("Sequence {}", document.object_key);
                self.snapshot.source_key = Some(key.clone());
                self.snapshot.is_playing = self.is_playing();
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
                self.snapshot.effect_preview_active = self.effect_preview.is_some();
                self.snapshot.status = status;
            }
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

    fn render_sequence_frame(
        &mut self,
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        position_seconds: f64,
    ) -> OutputFrame {
        let generation = self.generation;
        let mut render_timing = PreviewRenderTiming::default();
        match self.cached_renderer(analysis, document) {
            Ok((renderer, renderer_build_ms)) => {
                let (frame, evaluation_timing) =
                    renderer.evaluate_timed(position_seconds, generation);
                render_timing =
                    PreviewRenderTiming::from_evaluation(renderer_build_ms, evaluation_timing);
                self.last_render_timing = render_timing;
                frame
            }
            Err(message) => {
                self.last_render_timing = render_timing;
                empty_frame(generation, message)
            }
        }
    }

    fn render_effect_preview_frame(
        &mut self,
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        preview_seconds: f64,
        effect_filter: &HashSet<u32>,
    ) -> OutputFrame {
        let generation = self.generation;
        let mut render_timing = PreviewRenderTiming::default();
        match self.cached_renderer(analysis, document) {
            Ok((renderer, renderer_build_ms)) => {
                let (frame, evaluation_timing) = renderer.evaluate_effect_preview_filtered_timed(
                    preview_seconds,
                    generation,
                    Some(effect_filter),
                );
                render_timing =
                    PreviewRenderTiming::from_evaluation(renderer_build_ms, evaluation_timing);
                self.last_render_timing = render_timing;
                frame
            }
            Err(message) => {
                self.last_render_timing = render_timing;
                empty_frame(generation, message)
            }
        }
    }

    fn cached_renderer(
        &mut self,
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
    ) -> Result<(&mut SequenceFrameEvaluator, f64), String> {
        let mut renderer_build_ms = 0.0;
        if self.render_cache.is_none() {
            let build_started = Instant::now();
            self.render_cache = Some(SequenceFrameEvaluator::new(analysis, document)?);
            renderer_build_ms = elapsed_ms(build_started);
        }
        self.render_cache
            .as_mut()
            .map(|renderer| (renderer, renderer_build_ms))
            .ok_or_else(|| "Sequence preview renderer was not prepared".to_string())
    }
}

impl PreviewRenderTiming {
    fn from_evaluation(renderer_build_ms: f64, evaluation: SequenceFrameEvaluationTiming) -> Self {
        Self {
            total_ms: renderer_build_ms + evaluation.total_ms,
            renderer_build_ms,
            frame_evaluate_ms: evaluation.total_ms,
            fixture_clone_ms: evaluation.fixture_clone_ms,
            effect_loop_ms: evaluation.effect_loop_ms,
            output_frame_ms: evaluation.output_frame_ms,
            active_effects: evaluation.active_effects,
            sampled_pixels: evaluation.sampled_pixels,
        }
    }
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use dawn_project::analysis::{analyze_project_with_overlays, ProjectAnalysis, ProjectOverlay};
    use dawn_project::document::{get_sequence_document, SequenceDocument};
    use dawn_project::fs::WorkspaceFs;
    use dawn_project::model::{Color, FixtureId};
    use dawn_project::path::{canonicalize_path, utf8_path, Utf8PathBuf};

    use super::{PreviewSession, SequenceKey};

    fn club_rig_project_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/club-rig/project.dawn")
    }

    fn club_rig_context() -> (WorkspaceFs, Utf8PathBuf, Utf8PathBuf) {
        let project_path = club_rig_project_path();
        let root = project_path
            .parent()
            .expect("club rig project should have a parent");
        let fs = WorkspaceFs::open(root).expect("club rig root should open");
        let project_path = utf8_path(
            project_path
                .strip_prefix(root)
                .expect("project path should be under root"),
        )
        .expect("project path should be valid UTF-8");
        let sequence_path = utf8_path(Path::new("sequences/opening.sequence.dawn"))
            .expect("sequence path should be valid UTF-8");
        (fs, project_path, sequence_path)
    }

    fn club_rig_analysis_and_sequence(
        overlays: Vec<ProjectOverlay>,
    ) -> (ProjectAnalysis, SequenceDocument, SequenceKey) {
        let (fs, project_path, sequence_path) = club_rig_context();
        let analysis = analyze_project_with_overlays(
            &fs,
            project_path.clone(),
            Some("club_rig"),
            overlays.clone(),
        );
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let document = get_sequence_document(
            &fs,
            sequence_path.clone(),
            "opening",
            project_path,
            overlays,
        )
        .expect("club rig sequence should load");
        let key = SequenceKey {
            path: document.path.clone().into(),
            object_key: document.object_key.clone(),
        };
        (analysis, document, key)
    }

    fn edited_sequence_overlay() -> ProjectOverlay {
        let (fs, _, sequence_path) = club_rig_context();
        let sequence_path = canonicalize_path(&fs.resolve(&sequence_path));
        let content = fs
            .read_to_string(&sequence_path)
            .expect("club rig sequence should read")
            .replace("value: '#66e3ff'", "value: '#0000ff'");
        ProjectOverlay {
            path: sequence_path,
            content,
        }
    }

    fn frame_colors(session: &PreviewSession) -> Vec<Color> {
        session
            .snapshot
            .frame
            .fixtures
            .iter()
            .flat_map(|fixture| fixture.pixels.iter().map(|pixel| pixel.color))
            .collect()
    }

    fn fixture_is_lit(session: &PreviewSession, id: FixtureId) -> bool {
        session
            .snapshot
            .frame
            .fixtures
            .iter()
            .find(|fixture| fixture.id == id)
            .map(|fixture| {
                fixture
                    .pixels
                    .iter()
                    .any(|pixel| pixel.color != Color::new(0, 0, 0))
            })
            .unwrap_or(false)
    }

    #[test]
    fn render_cache_invalidates_when_sequence_source_refreshes() {
        let (analysis, document, key) = club_rig_analysis_and_sequence(Vec::new());
        let mut session = PreviewSession::default();
        session.sync_source(Some((key.clone(), document)), Some(&analysis));
        session.seek(2.0, Some(&analysis));
        let before = frame_colors(&session);

        let overlay = edited_sequence_overlay();
        let (edited_analysis, edited_document, edited_key) =
            club_rig_analysis_and_sequence(vec![overlay]);
        assert_eq!(key, edited_key);
        session.sync_source(Some((edited_key, edited_document)), Some(&edited_analysis));
        let after = frame_colors(&session);

        assert_ne!(before, after);
    }

    #[test]
    fn effect_preview_id_changes_reuse_the_sequence_render_cache() {
        let (analysis, document, key) = club_rig_analysis_and_sequence(Vec::new());
        let mut session = PreviewSession::default();
        session.sync_source(Some((key, document)), Some(&analysis));
        let original_cache = session
            .render_cache
            .as_ref()
            .map(|renderer| renderer as *const _ as usize)
            .expect("source sync should prepare renderer");

        session.set_effect_preview_ids(vec![1], Some(&analysis));
        let first_selection_cache = session
            .render_cache
            .as_ref()
            .map(|renderer| renderer as *const _ as usize)
            .expect("effect preview should keep renderer");
        assert_eq!(original_cache, first_selection_cache);
        assert!(fixture_is_lit(&session, FixtureId(11)));
        assert!(!fixture_is_lit(&session, FixtureId(1)));

        session.set_effect_preview_ids(vec![23], Some(&analysis));
        let second_selection_cache = session
            .render_cache
            .as_ref()
            .map(|renderer| renderer as *const _ as usize)
            .expect("effect preview id changes should keep renderer");
        assert_eq!(original_cache, second_selection_cache);
        assert!(fixture_is_lit(&session, FixtureId(1)));
        assert!(!fixture_is_lit(&session, FixtureId(11)));
    }
}
