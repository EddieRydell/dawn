use dawn_language::dsl::{BytecodeProgram, RunContext, RuntimeError, VmWorkspace};
use dawn_language::native_effect::{self, NativeSample};
use dawn_language::values::{Color, SampleTime};
use std::collections::HashMap;
use std::sync::Arc;

use crate::sequence::color::compose_max;
use crate::sequence::targets::PreparedTargetPixel;
use crate::{PreparedEffect, PreparedEffectImplementation, RenderError};

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreparedSampleContext {
    pub(crate) pixel_index: usize,
    pub(crate) pixel_count: usize,
    pub(crate) pixel_fraction: f32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct PreparedSampleContextKey {
    pixel_index: usize,
    pixel_count: usize,
    pixel_fraction_bits: u32,
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

pub(crate) fn render_sampled_effect_target_colors(
    programs: &[Arc<BytecodeProgram>],
    effect: &PreparedEffect,
    effect_pixels: &PreparedSampledEffectPixels,
    rendered: &mut [Color],
    sample_time: dawn_language::values::SampleTime,
    workspace: &mut VmWorkspace,
) -> Result<(), RenderError> {
    let local_time = effect.local_time(sample_time);
    let progress = effect.progress(sample_time);
    let automated = effect_params_at(effect, sample_time)?;
    let native_sample = match (&effect.implementation, automated.as_ref()) {
        (
            PreparedEffectImplementation::Native {
                params: Some((builtin, _)),
                ..
            },
            Some(params),
        ) => Some(native_effect::prepare_sample(*builtin, params)?),
        _ => None,
    };
    render_sampled_effect_pixels(effect_pixels, rendered, |sample_context| {
        sample_effect_group(
            programs,
            effect,
            automated.as_ref(),
            native_sample.as_ref(),
            sample_context,
            progress,
            local_time,
            workspace,
        )
    })
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
            pixel_index: pixel.pixel_index(),
            pixel_count: pixel.pixel_count(),
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
    progress: f32,
    local_time: dawn_language::values::SampleDuration,
) -> RunContext {
    RunContext {
        progress,
        time: local_time,
        duration: effect.duration,
        pixel_index: sample_context.pixel_index as i32,
        pixel_count: sample_context.pixel_count as i32,
        pixel_fraction: sample_context.pixel_fraction,
    }
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn sample_effect_group(
    programs: &[Arc<BytecodeProgram>],
    effect: &PreparedEffect,
    params: Option<&dawn_language::dsl::BoundParams>,
    native_sample: Option<&NativeSample>,
    sample_context: PreparedSampleContext,
    progress: f32,
    local_time: dawn_language::values::SampleDuration,
    workspace: &mut VmWorkspace,
) -> Result<Color, RuntimeError> {
    let context = run_context(effect, sample_context, progress, local_time);
    match &effect.implementation {
        PreparedEffectImplementation::Dsl {
            program,
            bound_params,
        } => programs[*program as usize].sample_effect(
            params.unwrap_or(bound_params),
            &context,
            workspace,
        ),
        PreparedEffectImplementation::Native { sample, .. } => {
            let sample_time = effect
                .start_time
                .checked_add_duration(local_time)
                .unwrap_or(effect.start_time);
            native_sample
                .unwrap_or(sample)
                .sample(&context, sample_time)
        }
    }
}

fn effect_params_at(
    effect: &PreparedEffect,
    sample_time: SampleTime,
) -> Result<Option<dawn_language::dsl::BoundParams>, RenderError> {
    let Some(automation) = &effect.automation else {
        return Ok(None);
    };
    let mut params = implementation_params(&effect.implementation)?.clone();
    apply_bound_automation(&mut params, &automation.bindings, sample_time)?;
    Ok(Some(params))
}

pub(crate) fn apply_bound_automation(
    params: &mut dawn_language::dsl::BoundParams,
    automation: &[crate::PreparedAutomation],
    sample_time: SampleTime,
) -> Result<(), RenderError> {
    for binding in automation {
        params.apply_automation(
            usize::from(binding.param_index),
            &binding.curve,
            &binding.mapping,
            binding.position(sample_time),
        )?;
    }
    Ok(())
}

fn implementation_params(
    implementation: &PreparedEffectImplementation,
) -> Result<&dawn_language::dsl::BoundParams, RenderError> {
    let params = match implementation {
        PreparedEffectImplementation::Dsl { bound_params, .. } => bound_params,
        PreparedEffectImplementation::Native {
            params: Some((_, params)),
            ..
        } => params,
        PreparedEffectImplementation::Native { params: None, .. } => {
            return Err(RenderError::BadGraph {
                message: "automation requires a parameterized sample effect".to_string(),
            });
        }
    };
    Ok(params)
}

pub(crate) fn prepare_sampled_effect_pixel_groups(
    pixels: &[PreparedSampledEffectPixel],
) -> Option<Vec<PreparedSampledEffectPixelGroup>> {
    let mut group_indexes = HashMap::<PreparedSampleContextKey, usize>::new();
    let mut groups = Vec::<PreparedSampledEffectPixelGroup>::new();
    let mut has_repeated_context = false;

    for sampled in pixels {
        let context = PreparedSampleContext {
            pixel_index: sampled.pixel.pixel_index(),
            pixel_count: sampled.pixel.pixel_count(),
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
