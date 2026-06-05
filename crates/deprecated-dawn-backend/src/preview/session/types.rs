use std::time::Instant;

use dawn_language::document::{SequenceAudioDocument, SequenceDocument};
use dawn_language::path::Utf8PathBuf;

use crate::output::sequence::OutputFrame;
use crate::RenderedFrame;

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
    pub preview_updating: bool,
    pub position_seconds: f64,
    pub home_seconds: f64,
    pub duration_seconds: f64,
    pub audio: Option<SequenceAudioDocument>,
    pub clock_source: String,
    pub audio_playback_status: AudioPlaybackStatus,
    pub effect_preview_active: bool,
    pub frame: RenderedFrame,
    pub status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewSyncMode {
    RenderNow,
    DeferRender,
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
pub struct PreviewRenderRequest {
    pub id: u64,
    pub dirty_revision: u64,
    pub generation: u64,
    pub key: SequenceKey,
    pub document: SequenceDocument,
    pub position_seconds: f64,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct PreviewRenderResult {
    pub request: PreviewRenderRequest,
    pub frame: OutputFrame,
    pub timing: PreviewRenderTiming,
}
