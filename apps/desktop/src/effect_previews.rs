use dawn_app_core::output_runtime::SequenceEffectThumbnail;
use dawn_project::document::{SequenceDocument, SequenceEffectDocument};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::state::{lock_effect_preview_cache, AppState, CommandResult};

const PREVIEW_MAX_COLUMNS: usize = 360;
const PREVIEW_MAX_ROWS: usize = 50;

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

pub(crate) struct EffectPreviewRequest<'a> {
    pub(crate) state: &'a State<'a, AppState>,
    pub(crate) analysis: &'a dawn_project::analysis::ProjectAnalysis,
    pub(crate) document: &'a SequenceDocument,
    pub(crate) effect: &'a SequenceEffectDocument,
}

pub(crate) fn preview_for_effect(
    request: EffectPreviewRequest<'_>,
) -> CommandResult<Option<SequenceEffectPreviewDto>> {
    lock_effect_preview_cache(request.state)?
        .effect_thumbnail(
            request.analysis,
            request.document,
            request.effect,
            PREVIEW_MAX_COLUMNS,
            PREVIEW_MAX_ROWS,
        )?
        .map(sequence_effect_preview_dto)
        .transpose()
}

fn sequence_effect_preview_dto(
    thumbnail: SequenceEffectThumbnail,
) -> CommandResult<SequenceEffectPreviewDto> {
    Ok(SequenceEffectPreviewDto {
        effect_id: thumbnail.effect_id,
        duration_seconds: thumbnail.duration_seconds,
        source_pixel_count: thumbnail.source_pixel_count,
        sampled_pixel_indices: thumbnail.sampled_pixel_indices,
        columns: thumbnail.columns,
        rows: thumbnail.rows,
        colors: thumbnail.colors.into_iter().map(pack_rgb).collect(),
    })
}

fn pack_rgb(color: dawn_project::model::Color) -> u32 {
    ((color.red as u32) << 16) | ((color.green as u32) << 8) | color.blue as u32
}
