use dawn_app_core::output_runtime::{pixel_context_for_effect, prepare_params_from_document};
use dawn_project::document::{
    SequenceEffectDocument, SequenceEffectParamDocument, SequenceMarkCollectionDocument,
};
use dawn_project::effect_script::FixtureContext;
use dawn_project::frame::{frame_count, frame_start};
use dawn_project::model::{EffectParam, Resolved, TimeSpan};
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct EffectPreviewCacheKey {
    sequence_path: String,
    object_key: String,
    effect_id: u32,
    duration_nanoseconds: u64,
    frame_rate: u32,
    scope: dawn_project::model::SequenceEffectScope,
    script_key: String,
    script_source: String,
    params_json: String,
    mark_collections_json: String,
    target_pixels_json: String,
    sampled_pixel_indices: Vec<usize>,
    max_columns: usize,
    max_rows: usize,
}

pub(crate) fn preview_for_effect(
    state: &State<'_, AppState>,
    analysis: &dawn_project::analysis::ProjectAnalysis,
    sequence_path: &str,
    object_key: &str,
    frame_rate: u32,
    mark_collections: &[SequenceMarkCollectionDocument],
    effect: &SequenceEffectDocument,
) -> CommandResult<Option<SequenceEffectPreviewDto>> {
    let Some(render) = &effect.render else {
        return Ok(None);
    };
    if frame_rate == 0 || effect.duration_seconds == 0.0 || render.target_pixels.is_empty() {
        return Ok(None);
    }

    let duration =
        TimeSpan::try_from_seconds_f64_rounded(effect.duration_seconds).map_err(str::to_string)?;
    if duration == TimeSpan::ZERO {
        return Ok(None);
    }

    let source_pixel_count = render.target_pixels.len();
    let sampled_pixel_indices = evenly_sample_indices(source_pixel_count, PREVIEW_MAX_ROWS);
    let cache_key = EffectPreviewCacheKey {
        sequence_path: sequence_path.to_string(),
        object_key: object_key.to_string(),
        effect_id: effect.id,
        duration_nanoseconds: duration.as_nanoseconds(),
        frame_rate,
        scope: effect.scope,
        script_key: render.script_key.clone(),
        script_source: render.script_source.clone(),
        params_json: serde_json::to_string(&render.params).map_err(|error| error.to_string())?,
        mark_collections_json: relevant_mark_collections_json(
            &render.params,
            mark_collections,
            effect.start_seconds,
        )?,
        target_pixels_json: serde_json::to_string(&render.target_pixels)
            .map_err(|error| error.to_string())?,
        sampled_pixel_indices: sampled_pixel_indices.clone(),
        max_columns: PREVIEW_MAX_COLUMNS,
        max_rows: PREVIEW_MAX_ROWS,
    };
    if let Some(preview) = lock_effect_preview_cache(state)?.get(&cache_key).cloned() {
        return Ok(Some(preview));
    }

    let total_frames = total_preview_frames(duration, frame_rate);
    let sampled_frame_indices = evenly_sample_indices(total_frames, PREVIEW_MAX_COLUMNS);
    let Some(script) = analysis.compiled_script_for_key(&render.script_key) else {
        return Ok(None);
    };
    let prepared_params = match prepare_params_from_document(
        script,
        &render.params,
        mark_collections,
        effect.start_seconds,
    ) {
        Ok(params) => params,
        Err(_) => return Ok(None),
    };
    let mut colors = Vec::with_capacity(sampled_frame_indices.len() * sampled_pixel_indices.len());

    for target_pixel_index in &sampled_pixel_indices {
        let Some(pixel) = render.target_pixels.get(*target_pixel_index) else {
            return Ok(None);
        };
        for frame_index in &sampled_frame_indices {
            let local_seconds = local_seconds_for_frame(*frame_index, frame_rate, duration);
            let progress = (local_seconds / effect.duration_seconds).clamp(0.0, 1.0);
            let pixel_context = pixel_context_for_effect(
                effect.scope,
                *target_pixel_index,
                source_pixel_count,
                pixel.pixel_index,
                pixel.pixel_count,
            );
            let color = match script.sample_prepared(
                progress,
                local_seconds,
                FixtureContext {
                    index: pixel.fixture_index,
                },
                pixel_context,
                &prepared_params,
            ) {
                Ok(color) => color,
                Err(_) => return Ok(None),
            };
            colors.push(pack_rgb(color));
        }
    }

    let preview = SequenceEffectPreviewDto {
        effect_id: effect.id,
        duration_seconds: effect.duration_seconds,
        source_pixel_count: source_pixel_count.min(u32::MAX as usize) as u32,
        sampled_pixel_indices: sampled_pixel_indices
            .iter()
            .map(|index| (*index).min(u32::MAX as usize) as u32)
            .collect(),
        columns: sampled_frame_indices.len().min(u32::MAX as usize) as u32,
        rows: sampled_pixel_indices.len().min(u32::MAX as usize) as u32,
        colors,
    };
    lock_effect_preview_cache(state)?.insert(cache_key, preview.clone());
    Ok(Some(preview))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MarkCollectionCacheSignature<'a> {
    key: &'a str,
    marks_seconds: &'a [f64],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MarkCacheSignature<'a> {
    effect_start_seconds: f64,
    collections: Vec<MarkCollectionCacheSignature<'a>>,
}

fn relevant_mark_collections_json(
    params: &[SequenceEffectParamDocument],
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
) -> Result<String, String> {
    let keys = params
        .iter()
        .filter_map(|param| match &param.value {
            EffectParam::<Resolved>::Marks { key } => Some(key.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    if keys.is_empty() {
        return Ok("[]".to_string());
    }

    let collections = mark_collections
        .iter()
        .filter(|collection| keys.contains(&collection.key.as_str()))
        .map(|collection| MarkCollectionCacheSignature {
            key: &collection.key,
            marks_seconds: &collection.marks_seconds,
        })
        .collect();
    serde_json::to_string(&MarkCacheSignature {
        effect_start_seconds,
        collections,
    })
    .map_err(|error| error.to_string())
}

fn total_preview_frames(duration: TimeSpan, frame_rate: u32) -> usize {
    frame_count(duration, frame_rate).max(1)
}

fn local_seconds_for_frame(frame_index: usize, frame_rate: u32, duration: TimeSpan) -> f64 {
    let local_nanoseconds = frame_start(frame_index as u64, frame_rate)
        .as_nanoseconds()
        .min(duration.as_nanoseconds().saturating_sub(1));
    local_nanoseconds as f64 / 1_000_000_000.0
}

fn evenly_sample_indices(source_count: usize, max_count: usize) -> Vec<usize> {
    if source_count == 0 || max_count == 0 {
        return Vec::new();
    }
    let count = source_count.min(max_count);
    if count == 1 {
        return vec![0];
    }
    (0..count)
        .map(|index| {
            ((index as f64) * ((source_count - 1) as f64) / ((count - 1) as f64)).round() as usize
        })
        .collect()
}

fn pack_rgb(color: dawn_project::model::Color) -> u32 {
    ((color.red as u32) << 16) | ((color.green as u32) << 8) | color.blue as u32
}
