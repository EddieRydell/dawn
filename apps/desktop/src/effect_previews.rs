use std::time::Instant;

use dawn_app_core::output_runtime::{
    pixel_context_for_effect, prepare_params_from_document, SequenceFrameEvaluator,
};
use dawn_project::document::{
    SequenceDocument, SequenceEffectDocument, SequenceEffectParamDocument,
    SequenceMarkCollectionDocument,
};
use dawn_project::effect_script::{EffectScriptKind, FixtureContext};
use dawn_project::frame::{frame_count, frame_start};
use dawn_project::model::{EffectParam, Resolved, TimeSpan};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::state::{
    lock_effect_preview_cache, lock_effect_preview_preparation_cache, AppState, CommandResult,
    SequencePreparationCacheKey,
};

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
    script_dependencies_json: String,
    params_json: String,
    mark_collections_json: String,
    target_pixels_json: String,
    sampled_pixel_indices: Vec<usize>,
    max_columns: usize,
    max_rows: usize,
}

pub(crate) struct EffectPreviewRequest<'a> {
    pub(crate) state: &'a State<'a, AppState>,
    pub(crate) analysis: &'a dawn_project::analysis::ProjectAnalysis,
    pub(crate) sequence_path: &'a str,
    pub(crate) object_key: &'a str,
    pub(crate) frame_rate: u32,
    pub(crate) mark_collections: &'a [SequenceMarkCollectionDocument],
    pub(crate) document: &'a SequenceDocument,
    pub(crate) effect: &'a SequenceEffectDocument,
}

pub(crate) fn preview_for_effect(
    request: EffectPreviewRequest<'_>,
) -> CommandResult<Option<SequenceEffectPreviewDto>> {
    let Some(render) = &request.effect.render else {
        return Ok(None);
    };
    if request.frame_rate == 0
        || request.effect.duration_seconds == 0.0
        || render.target_pixels.is_empty()
    {
        return Ok(None);
    }

    let duration = TimeSpan::try_from_seconds_f64_rounded(request.effect.duration_seconds)
        .map_err(str::to_string)?;
    if duration == TimeSpan::ZERO {
        return Ok(None);
    }

    let source_pixel_count = render.target_pixels.len();
    let sampled_pixel_indices = evenly_sample_indices(source_pixel_count, PREVIEW_MAX_ROWS);
    let Some(script) = request.analysis.compiled_script_for_key(&render.script_key) else {
        return Ok(None);
    };
    let cache_key = EffectPreviewCacheKey {
        sequence_path: request.sequence_path.to_string(),
        object_key: request.object_key.to_string(),
        effect_id: request.effect.id,
        duration_nanoseconds: duration.as_nanoseconds(),
        frame_rate: request.frame_rate,
        scope: request.effect.scope,
        script_key: render.script_key.clone(),
        script_source: render.script_source.clone(),
        script_dependencies_json: script_dependencies_json(
            request.analysis,
            &render.script_key,
            script,
        )?,
        params_json: serde_json::to_string(&render.params).map_err(|error| error.to_string())?,
        mark_collections_json: relevant_mark_collections_json(
            &render.params,
            request.mark_collections,
            request.effect.start_seconds,
        )?,
        target_pixels_json: serde_json::to_string(&render.target_pixels)
            .map_err(|error| error.to_string())?,
        sampled_pixel_indices: sampled_pixel_indices.clone(),
        max_columns: PREVIEW_MAX_COLUMNS,
        max_rows: PREVIEW_MAX_ROWS,
    };
    if let Some(preview) = lock_effect_preview_cache(request.state)?
        .get(&cache_key)
        .cloned()
    {
        if script.kind == EffectScriptKind::Generator {
            eprintln!(
                "[effect-preview] raster-cache hit sequence={} object={} effect={} script={} columns={} rows={}",
                request.sequence_path,
                request.object_key,
                request.effect.id,
                render.script_key,
                preview.columns,
                preview.rows,
            );
        }
        return Ok(Some(preview));
    }

    let total_frames = total_preview_frames(duration, request.frame_rate);
    let sampled_frame_indices = evenly_sample_indices(total_frames, PREVIEW_MAX_COLUMNS);
    if script.kind == EffectScriptKind::Generator {
        return preview_for_generator_effect(GeneratorEffectPreviewRequest {
            state: request.state,
            analysis: request.analysis,
            sequence_path: request.sequence_path,
            object_key: request.object_key,
            document: request.document,
            effect: request.effect,
            render,
            duration,
            source_pixel_count,
            sampled_pixel_indices,
            sampled_frame_indices,
            cache_key,
        });
    }
    let prepared_params = match prepare_params_from_document(
        script,
        &render.params,
        request.mark_collections,
        request.effect.start_seconds,
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
            let local_seconds = local_seconds_for_frame(*frame_index, request.frame_rate, duration);
            let progress = (local_seconds / request.effect.duration_seconds).clamp(0.0, 1.0);
            let pixel_context = pixel_context_for_effect(
                request.effect.scope,
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
        effect_id: request.effect.id,
        duration_seconds: request.effect.duration_seconds,
        source_pixel_count: source_pixel_count.min(u32::MAX as usize) as u32,
        sampled_pixel_indices: sampled_pixel_indices
            .iter()
            .map(|index| (*index).min(u32::MAX as usize) as u32)
            .collect(),
        columns: sampled_frame_indices.len().min(u32::MAX as usize) as u32,
        rows: sampled_pixel_indices.len().min(u32::MAX as usize) as u32,
        colors,
    };
    lock_effect_preview_cache(request.state)?.insert(cache_key, preview.clone());
    Ok(Some(preview))
}

struct GeneratorEffectPreviewRequest<'a> {
    state: &'a State<'a, AppState>,
    analysis: &'a dawn_project::analysis::ProjectAnalysis,
    sequence_path: &'a str,
    object_key: &'a str,
    document: &'a SequenceDocument,
    effect: &'a SequenceEffectDocument,
    render: &'a dawn_project::document::SequenceEffectRenderDocument,
    duration: TimeSpan,
    source_pixel_count: usize,
    sampled_pixel_indices: Vec<usize>,
    sampled_frame_indices: Vec<usize>,
    cache_key: EffectPreviewCacheKey,
}

fn preview_for_generator_effect(
    request: GeneratorEffectPreviewRequest<'_>,
) -> CommandResult<Option<SequenceEffectPreviewDto>> {
    let total_started = Instant::now();
    let filter = [request.effect.id].into_iter().collect();
    eprintln!(
        "[effect-preview] generator-cache miss sequence={} object={} effect={} script={} duration={:.6}s source_pixels={} sampled_rows={} sampled_columns={}",
        request.sequence_path,
        request.object_key,
        request.effect.id,
        request.render.script_key,
        request.effect.duration_seconds,
        request.source_pixel_count,
        request.sampled_pixel_indices.len(),
        request.sampled_frame_indices.len(),
    );

    let lock_started = Instant::now();
    let mut preparation_caches = lock_effect_preview_preparation_cache(request.state)?;
    let lock_ms = elapsed_ms(lock_started);
    let preparation_cache = preparation_caches
        .entry(SequencePreparationCacheKey {
            sequence_path: request.sequence_path.to_string(),
            object_key: request.object_key.to_string(),
        })
        .or_default();
    let prepare_started = Instant::now();
    let (mut evaluator, preparation_timing) =
        match SequenceFrameEvaluator::new_filtered_timed_with_preparation_cache(
            request.analysis,
            request.document,
            Some(&filter),
            preparation_cache,
        ) {
            Ok(result) => result,
            Err(error) => {
                eprintln!(
                    "[effect-preview] generator-prepare error effect={} lock_ms={:.3} prepare_ms={:.3} error={}",
                    request.effect.id,
                    lock_ms,
                    elapsed_ms(prepare_started),
                    error,
                );
                return Ok(None);
            }
        };
    let prepare_wall_ms = elapsed_ms(prepare_started);
    drop(preparation_caches);
    eprintln!(
        "[effect-preview] generator-prepare done effect={} lock_ms={:.3} prepare_wall_ms={:.3} timing_total_ms={:.3} layout_ms={:.3} authored_sample_ms={:.3} generator_ms={:.3} timeline_ms={:.3} prepared_effects={} generator_parents={} generated_children={}",
        request.effect.id,
        lock_ms,
        prepare_wall_ms,
        preparation_timing.total_ms,
        preparation_timing.layout_template_ms,
        preparation_timing.authored_sample_ms,
        preparation_timing.generator_expansion_ms,
        preparation_timing.timeline_index_ms,
        preparation_timing.prepared_effect_count,
        preparation_timing.generator_parent_count,
        preparation_timing.generated_child_count,
    );
    for parent in &preparation_timing.generator_parents {
        eprintln!(
            "[effect-preview] generator-parent effect={} script={} prepared_cache_hit={} topology_cache_hit={} target_pixels={} emitted_children={} prepared_children={} prepare_ms={:.3}",
            parent.parent_effect_id,
            parent.script_key,
            parent.prepared_cache_hit,
            parent.topology_cache_hit,
            parent.target_pixels,
            parent.emitted_children,
            parent.prepared_children,
            parent.total_prepare_ms,
        );
    }
    let local_seconds_by_column = request
        .sampled_frame_indices
        .iter()
        .map(|frame_index| {
            local_seconds_for_frame(*frame_index, request.document.frame_rate, request.duration)
        })
        .collect::<Vec<_>>();
    let sampled_pixels_by_row = request
        .sampled_pixel_indices
        .iter()
        .map(|target_pixel_index| {
            request
                .render
                .target_pixels
                .get(*target_pixel_index)
                .cloned()
        })
        .collect::<Option<Vec<_>>>();
    let Some(sampled_pixels_by_row) = sampled_pixels_by_row else {
        return Ok(None);
    };
    let sample_started = Instant::now();
    let colors = match evaluator.evaluate_generator_effect_thumbnail(
        request.effect.id,
        &local_seconds_by_column,
        &sampled_pixels_by_row,
    ) {
        Ok(colors) => colors.into_iter().map(pack_rgb).collect(),
        Err(error) => {
            eprintln!(
                "[effect-preview] generator-sample error effect={} sample_ms={:.3} error={}",
                request.effect.id,
                elapsed_ms(sample_started),
                error,
            );
            return Ok(None);
        }
    };
    let sample_ms = elapsed_ms(sample_started);

    let preview = SequenceEffectPreviewDto {
        effect_id: request.effect.id,
        duration_seconds: request.effect.duration_seconds,
        source_pixel_count: request.source_pixel_count.min(u32::MAX as usize) as u32,
        sampled_pixel_indices: request
            .sampled_pixel_indices
            .iter()
            .map(|index| (*index).min(u32::MAX as usize) as u32)
            .collect(),
        columns: request.sampled_frame_indices.len().min(u32::MAX as usize) as u32,
        rows: request.sampled_pixel_indices.len().min(u32::MAX as usize) as u32,
        colors,
    };
    let insert_started = Instant::now();
    lock_effect_preview_cache(request.state)?.insert(request.cache_key, preview.clone());
    let insert_ms = elapsed_ms(insert_started);
    eprintln!(
        "[effect-preview] generator-preview done effect={} sample_ms={:.3} insert_ms={:.3} total_ms={:.3} colors={}",
        request.effect.id,
        sample_ms,
        insert_ms,
        elapsed_ms(total_started),
        preview.colors.len(),
    );
    Ok(Some(preview))
}

fn script_dependencies_json(
    analysis: &dawn_project::analysis::ProjectAnalysis,
    script_key: &str,
    script: &dawn_project::effect_script::CompiledEffect,
) -> Result<String, String> {
    let mut dependencies = Vec::new();
    let (script_path, _) = dawn_project::analysis::split_effect_script_key(script_key);
    for import in &script.imports {
        let path = dawn_project::path::resolve_import_path(
            &script_path,
            &dawn_project::path::Utf8PathBuf::from(import.path.clone()),
        );
        dependencies.push((
            import.alias.clone(),
            path.to_string(),
            analysis
                .files
                .get(&path)
                .and_then(|file| file.text.clone())
                .unwrap_or_default(),
        ));
    }
    serde_json::to_string(&dependencies).map_err(|error| error.to_string())
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

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}
