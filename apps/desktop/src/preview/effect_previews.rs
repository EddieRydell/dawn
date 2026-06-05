use deprecated_dawn_backend::{
    SequenceEffectPreview, SequenceEffectPreviewRequestEffect, SequenceEffectPreviewResult,
};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectPreviewRequestEffectDto {
    pub effect_id: u32,
    pub signature: String,
}

impl From<SequenceEffectPreviewRequestEffectDto> for SequenceEffectPreviewRequestEffect {
    fn from(value: SequenceEffectPreviewRequestEffectDto) -> Self {
        Self {
            effect_id: value.effect_id,
            signature: value.signature,
        }
    }
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

pub(crate) fn sequence_effect_preview_result_dto(
    result: SequenceEffectPreviewResult,
) -> SequenceEffectPreviewResultDto {
    match result {
        SequenceEffectPreviewResult::Ready(result) => {
            SequenceEffectPreviewResultDto::Ready(SequenceEffectPreviewReadyResultDto {
                request_id: result.request_id,
                signature: result.signature,
                preview: sequence_effect_preview_dto(result.preview),
            })
        }
        SequenceEffectPreviewResult::Unavailable(result) => {
            SequenceEffectPreviewResultDto::Unavailable(SequenceEffectPreviewUnavailableResultDto {
                request_id: result.request_id,
                effect_id: result.effect_id,
                signature: result.signature,
            })
        }
        SequenceEffectPreviewResult::Error(result) => {
            SequenceEffectPreviewResultDto::Error(SequenceEffectPreviewErrorResultDto {
                request_id: result.request_id,
                effect_id: result.effect_id,
                signature: result.signature,
                message: result.message,
            })
        }
    }
}

fn sequence_effect_preview_dto(thumbnail: SequenceEffectPreview) -> SequenceEffectPreviewDto {
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

fn pack_rgb(color: dawn_language::model::Color) -> u32 {
    ((color.red as u32) << 16) | ((color.green as u32) << 8) | color.blue as u32
}
