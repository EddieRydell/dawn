use dawn_app_core::dto::{GeometryRenderBoundsDto, GeometryRenderPointDto};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectPreviewBatchDto {
    pub previews: Vec<SequenceEffectPreviewDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectPreviewDto {
    pub effect_id: u32,
    pub duration_seconds: f64,
    pub source_pixel_count: u32,
    pub sampled_pixel_indices: Vec<u32>,
    pub columns: u32,
    pub rows: u32,
    pub colors: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewStateEventDto {
    pub source_label: String,
    pub is_playing: bool,
    pub position_seconds: f64,
    pub home_seconds: f64,
    pub duration_seconds: f64,
    pub audio: Option<dawn_app_core::dto::SequenceAudioDto>,
    pub clock_source: String,
    pub audio_playback_status: String,
    pub status: String,
    pub timing: PreviewTimingDto,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewTimingDto {
    pub backend_seconds: f64,
    pub target_fps: u32,
    pub active_fps: u32,
    pub target_frame_ms: f64,
    pub sleep_planned_ms: f64,
    pub loop_interval_ms: f64,
    pub audio_position_seconds: Option<f64>,
    pub snapshot_position_seconds: f64,
    pub frame_position_seconds: f64,
    pub snapshot_minus_audio_ms: Option<f64>,
    pub frame_minus_audio_ms: Option<f64>,
    pub loop_elapsed_ms: f64,
    pub audio_poll_ms: f64,
    pub audio_apply_ms: f64,
    pub model_update_ms: f64,
    pub render_ms: f64,
    pub publish_ms: f64,
    pub event_interval_ms: f64,
    pub has_sink: bool,
    pub published_frame: bool,
    pub rendered_frame: bool,
}

impl PreviewTimingDto {
    pub(crate) fn empty(backend_seconds: f64) -> Self {
        Self {
            backend_seconds,
            target_fps: 0,
            active_fps: 0,
            target_frame_ms: 0.0,
            sleep_planned_ms: 0.0,
            loop_interval_ms: 0.0,
            audio_position_seconds: None,
            snapshot_position_seconds: 0.0,
            frame_position_seconds: 0.0,
            snapshot_minus_audio_ms: None,
            frame_minus_audio_ms: None,
            loop_elapsed_ms: 0.0,
            audio_poll_ms: 0.0,
            audio_apply_ms: 0.0,
            model_update_ms: 0.0,
            render_ms: 0.0,
            publish_ms: 0.0,
            event_interval_ms: 0.0,
            has_sink: false,
            published_frame: false,
            rendered_frame: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSceneDto {
    pub generation: u32,
    pub source_label: String,
    pub bounds: GeometryRenderBoundsDto,
    pub pixel_count: u32,
    pub fixtures: Vec<PreviewSceneFixtureDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSceneFixtureDto {
    pub id: u32,
    pub name: String,
    pub bulb_radius_meters: f64,
    pub first_pixel_index: u32,
    pub pixels: Vec<GeometryRenderPointDto>,
}
