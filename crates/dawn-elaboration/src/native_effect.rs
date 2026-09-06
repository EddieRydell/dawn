pub use dawn_runtime::native_effect::*;

use dawn_language::dsl::{BoundParams, DslBindCache, Identifier, RuntimeError, Value};
use dawn_language::effect::{BuiltinEffect, builtin_effect_definition};
use indexmap::IndexMap;

pub fn bind(
    builtin: BuiltinEffect,
    overrides: &IndexMap<Identifier, Value>,
) -> Result<BoundNativeEffect, RuntimeError> {
    bind_cached(builtin, overrides, &mut DslBindCache::default())
}

pub fn bind_cached(
    builtin: BuiltinEffect,
    overrides: &IndexMap<Identifier, Value>,
    cache: &mut DslBindCache,
) -> Result<BoundNativeEffect, RuntimeError> {
    let params =
        BoundParams::bind_cached(&builtin_effect_definition(builtin).params, overrides, cache)?;
    bind_prepared(builtin, params)
}

use dawn_language::dsl::{GeneratorContext, TargetItemValue, TargetPixelValue};
use dawn_language::values::{
    Color, Curve, Gradient, Marks, SampleDuration, SampleTime, sample_duration_from_seconds_f32,
    sample_duration_seconds_f32, sample_time_with_seconds_offset,
};
use dawn_runtime::sampling::deterministic_random;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum BoundNativeEffect {
    Sample {
        sample: NativeSample,
        params: BoundParams,
    },
    MarkPulse(MarkPulse),
    MarkChase(MarkChase),
}

#[derive(Clone, Debug)]
pub struct NativeGeneratedEffect {
    pub start_time: SampleTime,
    pub duration: SampleDuration,
    pub target: Arc<TargetItemValue>,
    pub sample: NativeSample,
}

#[derive(Clone, Debug)]
pub struct MarkPulse {
    beats: Arc<Marks>,
    base: Color,
    accent: Arc<Gradient>,
    hue: Arc<Curve>,
    hue_mix: f32,
    offset_seconds: f32,
    decay: SampleDuration,
    section_width_pixels: i32,
    section_edge_fade_pixels: f32,
    sections_per_mark: i32,
    seed: f32,
}

#[derive(Clone, Debug)]
pub struct MarkChase {
    beats: Arc<Marks>,
    base: Color,
    gradient_mode: GradientMode,
    gradients: Vec<Arc<Gradient>>,
    hue: Arc<Curve>,
    hue_mix: f32,
    offset_seconds: f32,
    chase_duration: SampleDuration,
    pulse_overlap: f32,
    section_width_pixels: i32,
    chase_positions: Vec<Arc<Curve>>,
    pulse_shape: Arc<Curve>,
}

pub fn bind_prepared(
    builtin: BuiltinEffect,
    params: BoundParams,
) -> Result<BoundNativeEffect, RuntimeError> {
    Ok(match builtin {
        BuiltinEffect::Pulse | BuiltinEffect::Chase | BuiltinEffect::Spin => {
            let sample = prepare_sample(builtin, &params)?;
            BoundNativeEffect::Sample { sample, params }
        }
        BuiltinEffect::MarkPulse => BoundNativeEffect::MarkPulse(MarkPulse {
            beats: params.marks(0)?,
            base: params.color(1)?,
            accent: params.gradient(2)?,
            hue: params.curve(3)?,
            hue_mix: params.float(4)?,
            offset_seconds: params.float(5)?,
            decay: positive_duration(params.float(6)?, "decay_seconds")?,
            section_width_pixels: params.int(7)?,
            section_edge_fade_pixels: params.float(8)?,
            sections_per_mark: params.int(9)?,
            seed: params.float(10)?,
        }),
        BuiltinEffect::MarkChase => BoundNativeEffect::MarkChase(MarkChase {
            beats: params.marks(0)?,
            base: params.color(1)?,
            gradient_mode: parse_gradient_mode(params.enum_name(2)?)?,
            gradients: gradient_array(params.array(3)?, "gradients")?,
            hue: params.curve(4)?,
            hue_mix: params.float(5)?,
            offset_seconds: params.float(6)?,
            chase_duration: positive_duration(params.float(7)?, "chase_seconds")?,
            pulse_overlap: params.float(8)?,
            section_width_pixels: params.int(9)?,
            chase_positions: curve_array(params.array(10)?, "chase_positions")?,
            pulse_shape: params.curve(11)?,
        }),
    })
}

impl BoundNativeEffect {
    pub fn generate(
        &self,
        context: &GeneratorContext,
    ) -> Result<Vec<NativeGeneratedEffect>, RuntimeError> {
        match self {
            Self::MarkPulse(value) => value.generate(context),
            Self::MarkChase(value) => value.generate(context),
            Self::Sample { .. } => Err(error("sample effect cannot generate children")),
        }
    }
}

impl MarkPulse {
    fn generate(
        &self,
        context: &GeneratorContext,
    ) -> Result<Vec<NativeGeneratedEffect>, RuntimeError> {
        let width = self.section_width_pixels.max(1);
        let mut sections: Vec<Vec<TargetPixelValue>> = Vec::new();
        for group in &context.target.groups {
            for pixel in group.pixels.iter().copied() {
                if sections
                    .last()
                    .and_then(|s| s.first())
                    .is_some_and(|first| {
                        first.element_index == pixel.element_index
                            && first.element_cell_index / width == pixel.element_cell_index / width
                    })
                {
                    if let Some(section) = sections.last_mut() {
                        section.push(pixel);
                    }
                } else {
                    sections.push(vec![pixel]);
                }
            }
        }
        if sections.is_empty() {
            return Ok(Vec::new());
        }
        let mut generated = Vec::new();
        for mark in &self.beats.marks {
            let hit = sample_duration_seconds_f32(*mark);
            for index in 0..self.sections_per_mark {
                let choice =
                    (deterministic_random([self.seed + hit * 1000.0, index as f32].into_iter())
                        * sections.len() as f32)
                        .floor() as usize;
                let start_time =
                    sample_time_with_seconds_offset(context.start_time, hit + self.offset_seconds)
                        .map_err(|_| error("generated effect start is out of range"))?;
                generated.push(NativeGeneratedEffect {
                    start_time,
                    duration: self.decay,
                    target: Arc::new(TargetItemValue {
                        pixels: Arc::from(sections[choice].clone()),
                    }),
                    sample: NativeSample::MarkPulseChild(MarkPulseChild {
                        base: self.base,
                        accent: Arc::clone(&self.accent),
                        hue: Arc::clone(&self.hue),
                        hue_mix: self.hue_mix,
                        section_width_pixels: self.section_width_pixels,
                        section_edge_fade_pixels: self.section_edge_fade_pixels,
                        parent_start: context.start_time,
                        parent_duration: context.duration,
                    }),
                });
            }
        }
        Ok(generated)
    }
}

impl MarkChase {
    fn generate(
        &self,
        context: &GeneratorContext,
    ) -> Result<Vec<NativeGeneratedEffect>, RuntimeError> {
        if self.gradients.is_empty() || self.chase_positions.is_empty() {
            return Err(error(
                "mark chase requires non-empty gradients and chase_positions",
            ));
        }
        let target = if context.target.groups.len() == 1 {
            Arc::clone(&context.target.groups[0])
        } else {
            Arc::new(TargetItemValue {
                pixels: Arc::from(
                    context
                        .target
                        .groups
                        .iter()
                        .flat_map(|group| group.pixels.iter().copied())
                        .collect::<Vec<_>>(),
                ),
            })
        };
        self.beats
            .marks
            .iter()
            .enumerate()
            .map(|(index, mark)| {
                let hit = sample_duration_seconds_f32(*mark);
                let start_time =
                    sample_time_with_seconds_offset(context.start_time, hit + self.offset_seconds)
                        .map_err(|_| error("generated effect start is out of range"))?;
                Ok(NativeGeneratedEffect {
                    start_time,
                    duration: self.chase_duration,
                    target: Arc::clone(&target),
                    sample: NativeSample::MarkChaseChild(MarkChaseChild {
                        base: self.base,
                        gradient_mode: self.gradient_mode,
                        gradient: Arc::clone(&self.gradients[index % self.gradients.len()]),
                        hue: Arc::clone(&self.hue),
                        hue_mix: self.hue_mix,
                        pulse_overlap: self.pulse_overlap,
                        section_width_pixels: self.section_width_pixels,
                        chase_position: Arc::clone(
                            &self.chase_positions[index % self.chase_positions.len()],
                        ),
                        pulse_shape: Arc::clone(&self.pulse_shape),
                        parent_start: context.start_time,
                        parent_duration: context.duration,
                    }),
                })
            })
            .collect::<Result<Vec<_>, _>>()
    }
}

fn positive_duration(seconds: f32, name: &str) -> Result<SampleDuration, RuntimeError> {
    let duration = sample_duration_from_seconds_f32(seconds)
        .map_err(|_| error(format!("native parameter `{name}` is out of range")))?;
    if duration.ticks() == 0 {
        return Err(error(format!("native parameter `{name}` must be positive")));
    }
    Ok(duration)
}

fn gradient_array(values: &[Value], name: &str) -> Result<Vec<Arc<Gradient>>, RuntimeError> {
    values
        .iter()
        .map(|value| match value {
            Value::Gradient(value) => Ok(Arc::clone(value)),
            _ => Err(error(format!("native parameter `{name}` has wrong type"))),
        })
        .collect()
}
fn curve_array(values: &[Value], name: &str) -> Result<Vec<Arc<Curve>>, RuntimeError> {
    values
        .iter()
        .map(|value| match value {
            Value::Curve(value) => Ok(Arc::clone(value)),
            _ => Err(error(format!("native parameter `{name}` has wrong type"))),
        })
        .collect()
}

fn error(message: impl Into<String>) -> RuntimeError {
    RuntimeError {
        message: message.into(),
    }
}
