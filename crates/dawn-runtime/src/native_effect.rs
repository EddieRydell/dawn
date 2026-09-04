use crate::BuiltinEffect;
use crate::dsl::{
    BoundParams, GeneratorContext, RunContext, RuntimeError, TargetItemValue, TargetPixelValue,
    Value,
};
use crate::sampling::{
    deterministic_random, hsv, mix_colors, sample_curve, sample_gradient, scale_color,
};
use crate::values::{
    Color, Curve, Gradient, Marks, SampleDuration, SampleTime, sample_duration_from_seconds_f32,
    sample_duration_seconds_f32, sample_time_with_seconds_offset,
};
use alloc::format;
#[cfg(not(feature = "atomic"))]
use alloc::rc::Rc as Arc;
use alloc::string::String;
#[cfg(feature = "atomic")]
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GradientMode {
    ThroughEffect,
    AcrossItems,
    PerPulse,
}

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
pub enum NativeSample {
    Pulse {
        gradient: Arc<Gradient>,
        pulse_shape: Arc<Curve>,
    },
    Chase(Chase),
    Spin {
        chase: Chase,
        revolutions: i32,
    },
    MarkPulseChild(MarkPulseChild),
    MarkChaseChild(MarkChaseChild),
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
pub struct MarkPulseChild {
    base: Color,
    accent: Arc<Gradient>,
    hue: Arc<Curve>,
    hue_mix: f32,
    section_width_pixels: i32,
    section_edge_fade_pixels: f32,
    parent_start: SampleTime,
    parent_duration: SampleDuration,
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

#[derive(Clone, Debug)]
pub struct MarkChaseChild {
    base: Color,
    gradient_mode: GradientMode,
    gradient: Arc<Gradient>,
    hue: Arc<Curve>,
    hue_mix: f32,
    pulse_overlap: f32,
    section_width_pixels: i32,
    chase_position: Arc<Curve>,
    pulse_shape: Arc<Curve>,
    parent_start: SampleTime,
    parent_duration: SampleDuration,
}

#[derive(Clone, Debug)]
pub struct Chase {
    gradient: Arc<Gradient>,
    gradient_mode: GradientMode,
    pulse_overlap: f32,
    section_width_pixels: i32,
    chase_position: Arc<Curve>,
    reverse: bool,
    extend_to_start: bool,
    extend_to_end: bool,
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

pub fn prepare_sample(
    builtin: BuiltinEffect,
    params: &BoundParams,
) -> Result<NativeSample, RuntimeError> {
    Ok(match builtin {
        BuiltinEffect::Pulse => NativeSample::Pulse {
            gradient: params.gradient(0)?,
            pulse_shape: params.curve(1)?,
        },
        BuiltinEffect::Chase => NativeSample::Chase(prepare_chase(params, 0)?),
        BuiltinEffect::Spin => NativeSample::Spin {
            chase: prepare_chase(params, 1)?,
            revolutions: params.int(5)?,
        },
        BuiltinEffect::MarkPulse | BuiltinEffect::MarkChase => {
            return Err(error("generator effect cannot be sampled"));
        }
    })
}

fn prepare_chase(params: &BoundParams, shifted: usize) -> Result<Chase, RuntimeError> {
    Ok(Chase {
        gradient: params.gradient(0)?,
        gradient_mode: parse_gradient_mode(params.enum_name(1)?)?,
        pulse_overlap: params.float(2)?,
        section_width_pixels: params.int(3)?,
        chase_position: params.curve(4)?,
        reverse: params.boolean(5 + shifted)?,
        extend_to_start: params.boolean(6 + shifted)?,
        extend_to_end: params.boolean(7 + shifted)?,
        pulse_shape: params.curve(8 + shifted)?,
    })
}

impl NativeSample {
    pub fn sample(
        &self,
        context: &RunContext,
        sample_time: SampleTime,
    ) -> Result<Color, RuntimeError> {
        match self {
            Self::Pulse {
                gradient,
                pulse_shape,
            } => gradient_scaled(
                gradient,
                context.progress,
                sample_curve(pulse_shape, context.progress),
            ),
            Self::Chase(chase) => sample_chase(chase, context, None),
            Self::Spin { chase, revolutions } => sample_chase(chase, context, Some(*revolutions)),
            Self::MarkPulseChild(child) => child.sample(context, sample_time),
            Self::MarkChaseChild(child) => child.sample(context, sample_time),
        }
    }
}

fn sample_chase(
    chase: &Chase,
    context: &RunContext,
    revolutions: Option<i32>,
) -> Result<Color, RuntimeError> {
    let width = (chase.section_width_pixels as f32).max(1.0);
    let count = libm::floorf((context.pixel_count as f32 + width - 1.0) / width).max(1.0);
    let position = libm::floorf(context.pixel_index as f32 / width) / (count - 1.0).max(1.0);
    if let Some(revolutions) = revolutions {
        let revolutions = revolutions.max(1);
        let virtual_count = count * revolutions as f32;
        let duration = (chase.pulse_overlap.max(1.0) / virtual_count).max(1e-9);
        let start = if chase.extend_to_start {
            -duration
        } else {
            0.0
        };
        let end = if chase.extend_to_end {
            1.0
        } else {
            (1.0 - duration).max(0.0)
        };
        let mut level = 0.0;
        let mut gradient_position = 0.0;
        for revolution in 0..revolutions {
            let mut virtual_position = (revolution as f32 + position) / revolutions as f32;
            if chase.reverse {
                virtual_position = 1.0 - virtual_position;
            }
            let hit = start
                + (end - start)
                    * crate::sampling::curve_crossing(
                        &chase.chase_position,
                        virtual_position,
                        virtual_position,
                    )
                    .clamp(0.0, 1.0);
            let pulse_progress = (context.progress - hit) / duration;
            if (0.0..=1.0).contains(&pulse_progress) {
                let candidate = sample_curve(&chase.pulse_shape, pulse_progress).clamp(0.0, 1.0);
                if candidate > level {
                    level = candidate;
                    gradient_position = match chase.gradient_mode {
                        GradientMode::ThroughEffect => context.progress,
                        GradientMode::AcrossItems => position,
                        GradientMode::PerPulse => pulse_progress,
                    };
                }
            }
        }
        return gradient_scaled(&chase.gradient, gradient_position.clamp(0.0, 1.0), level);
    }

    let chase_value = if chase.reverse {
        1.0 - position
    } else {
        position
    };
    let duration = (chase.pulse_overlap.max(1.0) / count).max(1e-9);
    let start = if chase.extend_to_start {
        -duration
    } else {
        0.0
    };
    let end = if chase.extend_to_end {
        1.0
    } else {
        (1.0 - duration).max(0.0)
    };
    let hit = start
        + (end - start)
            * crate::sampling::curve_crossing(&chase.chase_position, chase_value, chase_value)
                .clamp(0.0, 1.0);
    let pulse_progress = (context.progress - hit) / duration;
    let level = if (0.0..=1.0).contains(&pulse_progress) {
        sample_curve(&chase.pulse_shape, pulse_progress).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let gradient_position = match chase.gradient_mode {
        GradientMode::ThroughEffect => context.progress,
        GradientMode::AcrossItems => position,
        GradientMode::PerPulse => pulse_progress,
    };
    gradient_scaled(&chase.gradient, gradient_position.clamp(0.0, 1.0), level)
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
                let choice = libm::floorf(
                    deterministic_random([self.seed + hit * 1000.0, index as f32].into_iter())
                        * sections.len() as f32,
                ) as usize;
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

impl MarkPulseChild {
    fn sample(&self, c: &RunContext, sample_time: SampleTime) -> Result<Color, RuntimeError> {
        let width = (self.section_width_pixels as f32).max(1.0);
        let pixel = c.pixel_index as f32;
        let choice = libm::floorf(pixel / width);
        let fade = self.section_edge_fade_pixels.max(0.0);
        let active = if fade > 0.0 {
            let start = choice * width;
            let end = (start + width - 1.0).min(c.pixel_count as f32 - 1.0);
            ((pixel - start).min(end - pixel) / fade).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let progress = c.progress.clamp(0.0, 1.0);
        let parent = parent_progress(sample_time, self.parent_start, self.parent_duration);
        let accent = sample_gradient(&self.accent, progress)
            .ok_or_else(|| error("cannot sample empty gradient"))?;
        let pulse = mix_colors(
            accent,
            hsv(sample_curve(&self.hue, parent) / 360.0, 1.0, 1.0),
            self.hue_mix.clamp(0.0, 1.0),
        );
        Ok(mix_colors(self.base, pulse, active * (1.0 - progress)))
    }
}
impl MarkChaseChild {
    fn sample(&self, c: &RunContext, sample_time: SampleTime) -> Result<Color, RuntimeError> {
        let width = (self.section_width_pixels as f32).max(1.0);
        let pixel = c.pixel_index as f32;
        let section = (pixel - libm::floorf(pixel / width) * width) / width;
        let travel_start = -self.pulse_overlap / (c.pixel_count as f32).max(1.0);
        let travel_end = 1.0 + self.pulse_overlap / (c.pixel_count as f32).max(1.0);
        let chase = travel_start
            + (travel_end - travel_start)
                * sample_curve(&self.chase_position, c.progress).clamp(0.0, 1.0);
        let pulse_progress = (c.pixel_fraction - chase).abs()
            / (self.pulse_overlap / (c.pixel_count as f32).max(1.0)).max(1e-9);
        let level = sample_curve(&self.pulse_shape, pulse_progress.clamp(0.0, 1.0));
        let gp = match self.gradient_mode {
            GradientMode::ThroughEffect => c.progress,
            GradientMode::AcrossItems => c.pixel_fraction,
            GradientMode::PerPulse => section,
        };
        let parent = parent_progress(sample_time, self.parent_start, self.parent_duration);
        let value = gradient_scaled(&self.gradient, gp.clamp(0.0, 1.0), level)?;
        let hue = hsv(sample_curve(&self.hue, parent) / 360.0, 1.0, 1.0);
        Ok(mix_colors(
            self.base,
            mix_colors(value, hue, self.hue_mix.clamp(0.0, 1.0)),
            level,
        ))
    }
}

fn gradient_scaled(g: &Gradient, p: f32, level: f32) -> Result<Color, RuntimeError> {
    sample_gradient(g, p)
        .map(|c| scale_color(c, level))
        .ok_or_else(|| error("cannot sample empty gradient"))
}
fn positive_duration(seconds: f32, name: &str) -> Result<SampleDuration, RuntimeError> {
    let duration = sample_duration_from_seconds_f32(seconds)
        .map_err(|_| error(format!("native parameter `{name}` is out of range")))?;
    if duration.ticks() == 0 {
        return Err(error(format!("native parameter `{name}` must be positive")));
    }
    Ok(duration)
}
fn parent_progress(
    sample_time: SampleTime,
    parent_start: SampleTime,
    parent_duration: SampleDuration,
) -> f32 {
    let elapsed = sample_time
        .checked_duration_since(parent_start)
        .map_or(0, |duration| duration.ticks());
    (elapsed as f32 / parent_duration.ticks().max(1) as f32).clamp(0.0, 1.0)
}
fn parse_gradient_mode(value: &str) -> Result<GradientMode, RuntimeError> {
    match value {
        "through_effect" => Ok(GradientMode::ThroughEffect),
        "across_items" => Ok(GradientMode::AcrossItems),
        "per_pulse" => Ok(GradientMode::PerPulse),
        _ => Err(error("unsupported gradient mode")),
    }
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
