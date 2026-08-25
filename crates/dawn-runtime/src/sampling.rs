use dawn_language::dsl::{CompiledEffect, DslBindCache, DslVmScratch, RunContext, RuntimeError};
use dawn_language::native_effect::{self, BoundNativeEffect};
use dawn_language::values::{Color, Marks};
use std::collections::HashMap;
use std::sync::Arc;

use super::color::compose_max;
use super::effect_preparation::apply_automation_params;
use super::target::PreparedTargetPixel;
use super::{
    PrepareTargetCache, PreparedEffect, PreparedEffectImplementation, RenderError, arc_key,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreparedSampleContext {
    pub(crate) pixel_index: usize,
    pub(crate) pixel_count: usize,
    pub(crate) pixel_fraction: f64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct PreparedSampleContextKey {
    pixel_index: usize,
    pixel_count: usize,
    pixel_fraction_bits: u64,
}

impl From<PreparedSampleContext> for PreparedSampleContextKey {
    fn from(context: PreparedSampleContext) -> Self {
        Self {
            pixel_index: context.pixel_index,
            pixel_count: context.pixel_count,
            pixel_fraction_bits: context.pixel_fraction.to_bits(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedSampleContextGroup {
    pub(crate) context: PreparedSampleContext,
    pub(crate) target_indexes: Vec<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedSampledEffectPixels {
    pub(crate) pixels: Vec<PreparedSampledEffectPixel>,
    pub(crate) groups: Option<Vec<PreparedSampledEffectPixelGroup>>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedSampledEffectPixel {
    pub(crate) pixel: PreparedTargetPixel,
    pub(crate) rows: Vec<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedSampledEffectPixelGroup {
    context: PreparedSampleContext,
    rows: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct TargetColorAddress {
    pub(crate) element_index: usize,
    pub(crate) element_cell_index: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedSampleGroupCacheEntry {
    pub(crate) source: Arc<Vec<PreparedTargetPixel>>,
    pub(crate) groups: Option<Arc<Vec<PreparedSampleContextGroup>>>,
}

pub(crate) fn render_sampled_effect_target_colors(
    effect: &PreparedEffect,
    effect_pixels: &PreparedSampledEffectPixels,
    rendered: &mut [Color],
    sample_seconds: f64,
    scratch: &mut DslVmScratch,
    bind_cache: &mut DslBindCache,
) -> Result<(), RenderError> {
    let local_seconds = sample_seconds - effect.start_seconds;
    let progress = (local_seconds / effect.duration_seconds).clamp(0.0, 1.0);
    let automated = effect_implementation_at(effect, sample_seconds, bind_cache)?;
    let implementation = automated.as_ref().unwrap_or(&effect.implementation);
    match implementation {
        PreparedEffectImplementation::Dsl {
            definition,
            bound_params,
        } => render_sampled_effect_pixels(effect_pixels, rendered, |sample_context| {
            definition.sample_bound(
                bound_params,
                &run_context(effect, sample_context, progress, local_seconds),
                scratch,
            )
        }),
        PreparedEffectImplementation::Native { sample, .. } => {
            render_sampled_effect_pixels(effect_pixels, rendered, |sample_context| {
                sample.sample(&run_context(
                    effect,
                    sample_context,
                    progress,
                    local_seconds,
                ))
            })
        }
    }
}

#[inline(always)]
fn render_sampled_effect_pixels(
    effect_pixels: &PreparedSampledEffectPixels,
    rendered: &mut [Color],
    mut sample: impl FnMut(PreparedSampleContext) -> Result<Color, RuntimeError>,
) -> Result<(), RenderError> {
    if let Some(groups) = &effect_pixels.groups {
        for group in groups {
            let color = sample(group.context)?;
            for row in &group.rows {
                if let Some(target) = rendered.get_mut(*row) {
                    compose_max(target, color);
                }
            }
        }
        return Ok(());
    }
    for sampled in &effect_pixels.pixels {
        let pixel = &sampled.pixel;
        let color = sample(PreparedSampleContext {
            pixel_index: pixel.pixel_index,
            pixel_count: pixel.pixel_count,
            pixel_fraction: pixel.pixel_fraction,
        })?;
        for row in &sampled.rows {
            if let Some(target) = rendered.get_mut(*row) {
                compose_max(target, color);
            }
        }
    }
    Ok(())
}

#[inline(always)]
fn run_context(
    effect: &PreparedEffect,
    sample_context: PreparedSampleContext,
    progress: f64,
    local_seconds: f64,
) -> RunContext {
    RunContext {
        progress,
        seconds: local_seconds,
        duration: effect.duration_seconds,
        pixel_index: sample_context.pixel_index as i64,
        pixel_count: sample_context.pixel_count as i64,
        pixel_fraction: sample_context.pixel_fraction,
        global_marks: Marks { marks: Vec::new() },
    }
}

#[inline(always)]
pub(crate) fn sample_effect_pixel(
    effect: &PreparedEffect,
    implementation: &PreparedEffectImplementation,
    pixel: &PreparedTargetPixel,
    progress: f64,
    local_seconds: f64,
    scratch: &mut DslVmScratch,
) -> Result<Color, RuntimeError> {
    sample_effect_group(
        effect,
        implementation,
        PreparedSampleContext {
            pixel_index: pixel.pixel_index,
            pixel_count: pixel.pixel_count,
            pixel_fraction: pixel.pixel_fraction,
        },
        progress,
        local_seconds,
        scratch,
    )
}

#[inline(always)]
pub(crate) fn sample_effect_group(
    effect: &PreparedEffect,
    implementation: &PreparedEffectImplementation,
    sample_context: PreparedSampleContext,
    progress: f64,
    local_seconds: f64,
    scratch: &mut DslVmScratch,
) -> Result<Color, RuntimeError> {
    let context = run_context(effect, sample_context, progress, local_seconds);
    match implementation {
        PreparedEffectImplementation::Dsl {
            definition,
            bound_params,
        } => definition.sample_bound(bound_params, &context, scratch),
        PreparedEffectImplementation::Native { sample, .. } => sample.sample(&context),
    }
}

pub(crate) fn effect_implementation_at(
    effect: &PreparedEffect,
    sample_seconds: f64,
    bind_cache: &mut DslBindCache,
) -> Result<Option<PreparedEffectImplementation>, RenderError> {
    if effect.automation.is_empty() {
        return Ok(None);
    }
    let params =
        apply_automation_params(effect.params.clone(), &effect.automation, sample_seconds)?;
    Ok(Some(match &effect.implementation {
        PreparedEffectImplementation::Dsl { definition, .. } => PreparedEffectImplementation::Dsl {
            bound_params: definition.bind_params_cached(&params, bind_cache)?,
            definition: Arc::clone(definition),
        },
        PreparedEffectImplementation::Native { builtin, .. } => match builtin {
            Some(builtin) => match native_effect::bind(*builtin, &params)? {
                BoundNativeEffect::Sample(sample) => PreparedEffectImplementation::Native {
                    builtin: Some(*builtin),
                    sample,
                },
                _ => {
                    return Err(RenderError::GeneratorPrepare {
                        message: "automated native sample bound as generator".to_string(),
                    });
                }
            },
            None => {
                return Err(RenderError::GeneratorPrepare {
                    message: "missing native effect identity".to_string(),
                });
            }
        },
    }))
}

fn prepare_sample_context_groups(
    target: &[PreparedTargetPixel],
) -> Option<Vec<PreparedSampleContextGroup>> {
    let mut group_indexes = HashMap::<PreparedSampleContextKey, usize>::new();
    let mut groups = Vec::<PreparedSampleContextGroup>::new();
    let mut has_repeated_context = false;

    for (target_index, pixel) in target.iter().enumerate() {
        let context = PreparedSampleContext {
            pixel_index: pixel.pixel_index,
            pixel_count: pixel.pixel_count,
            pixel_fraction: pixel.pixel_fraction,
        };
        let key = PreparedSampleContextKey::from(context);
        if let Some(group_index) = group_indexes.get(&key) {
            has_repeated_context = true;
            groups[*group_index].target_indexes.push(target_index);
        } else {
            group_indexes.insert(key, groups.len());
            groups.push(PreparedSampleContextGroup {
                context,
                target_indexes: vec![target_index],
            });
        }
    }

    has_repeated_context.then_some(groups)
}

pub(crate) fn prepare_sample_context_groups_cached(
    cache: &mut PrepareTargetCache,
    target: &Arc<Vec<PreparedTargetPixel>>,
) -> Option<Arc<Vec<PreparedSampleContextGroup>>> {
    let key = arc_key(target);
    if let Some(entry) = cache.sample_groups.get(&key)
        && Arc::ptr_eq(&entry.source, target)
    {
        return entry.groups.clone();
    }
    let groups = prepare_sample_context_groups(target).map(Arc::new);
    cache.sample_groups.insert(
        key,
        PreparedSampleGroupCacheEntry {
            source: Arc::clone(target),
            groups: groups.clone(),
        },
    );
    groups
}

pub(crate) fn prepare_sample_groups_for_effect(
    cache: &mut PrepareTargetCache,
    compiled: &Arc<CompiledEffect>,
    target: &Arc<Vec<PreparedTargetPixel>>,
) -> Option<Arc<Vec<PreparedSampleContextGroup>>> {
    let compiled_key = arc_key(compiled);
    let eligible = *cache
        .sample_group_eligibility
        .entry(compiled_key)
        .or_insert_with(|| compiled.sample_reads_only_written_slots());
    eligible
        .then(|| prepare_sample_context_groups_cached(cache, target))
        .flatten()
}

pub(crate) fn prepare_sample_groups_for_implementation(
    cache: &mut PrepareTargetCache,
    implementation: &PreparedEffectImplementation,
    target: &Arc<Vec<PreparedTargetPixel>>,
) -> Option<Arc<Vec<PreparedSampleContextGroup>>> {
    match implementation {
        PreparedEffectImplementation::Dsl { definition, .. } => {
            prepare_sample_groups_for_effect(cache, definition, target)
        }
        PreparedEffectImplementation::Native { .. } => {
            prepare_sample_context_groups_cached(cache, target)
        }
    }
}

pub(crate) fn prepare_sampled_effect_pixel_groups(
    pixels: &[PreparedSampledEffectPixel],
) -> Option<Vec<PreparedSampledEffectPixelGroup>> {
    let mut group_indexes = HashMap::<PreparedSampleContextKey, usize>::new();
    let mut groups = Vec::<PreparedSampledEffectPixelGroup>::new();
    let mut has_repeated_context = false;

    for sampled in pixels {
        let context = PreparedSampleContext {
            pixel_index: sampled.pixel.pixel_index,
            pixel_count: sampled.pixel.pixel_count,
            pixel_fraction: sampled.pixel.pixel_fraction,
        };
        let key = PreparedSampleContextKey::from(context);
        if let Some(group_index) = group_indexes.get(&key) {
            has_repeated_context = true;
            groups[*group_index]
                .rows
                .extend(sampled.rows.iter().copied());
        } else {
            group_indexes.insert(key, groups.len());
            groups.push(PreparedSampledEffectPixelGroup {
                context,
                rows: sampled.rows.clone(),
            });
        }
    }

    has_repeated_context.then_some(groups)
}

pub(crate) fn evenly_sample_indices(source_count: usize, sample_count: usize) -> Vec<usize> {
    if source_count == 0 || sample_count == 0 {
        return Vec::new();
    }
    let sample_count = source_count.min(sample_count);
    if sample_count == 1 {
        return vec![0];
    }
    let last_source_index = source_count - 1;
    let last_sample_index = sample_count - 1;
    (0..sample_count)
        .map(|sample_index| {
            (sample_index * last_source_index + last_sample_index / 2) / last_sample_index
        })
        .collect()
}
