use dawn_app_runtime::output_runtime::SequenceEffectThumbnail;
use serde::{Deserialize, Serialize};
use specta::Type;

const PREVIEW_MAX_COLUMNS: usize = 360;
const PREVIEW_MAX_ROWS: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectPreviewRequestEffectDto {
    pub effect_id: u32,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectPreviewResultsDto {
    pub results: Vec<SequenceEffectPreviewResultDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SequenceEffectPreviewResultDto {
    Ready(SequenceEffectPreviewReadyResultDto),
    Unavailable(SequenceEffectPreviewUnavailableResultDto),
    Error(SequenceEffectPreviewErrorResultDto),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectPreviewReadyResultDto {
    pub request_id: u32,
    pub signature: String,
    pub preview: SequenceEffectPreviewDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectPreviewUnavailableResultDto {
    pub request_id: u32,
    pub effect_id: u32,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectPreviewErrorResultDto {
    pub request_id: u32,
    pub effect_id: u32,
    pub signature: String,
    pub message: String,
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

pub(crate) fn sequence_effect_preview_dto(
    thumbnail: SequenceEffectThumbnail,
) -> SequenceEffectPreviewDto {
    SequenceEffectPreviewDto {
        effect_id: thumbnail.effect_id,
        duration_seconds: thumbnail.duration_seconds,
        source_pixel_count: thumbnail.source_pixel_count,
        sampled_pixel_indices: thumbnail.sampled_pixel_indices,
        columns: thumbnail.columns,
        rows: thumbnail.rows,
        colors: thumbnail.colors.into_iter().map(pack_rgb).collect(),
    }
}

pub(crate) fn preview_max_columns() -> usize {
    PREVIEW_MAX_COLUMNS
}

pub(crate) fn preview_max_rows() -> usize {
    PREVIEW_MAX_ROWS
}

fn pack_rgb(color: dawn_language::model::Color) -> u32 {
    ((color.red as u32) << 16) | ((color.green as u32) << 8) | color.blue as u32
}
