use crate::dsl::{
    GeneratorContext, Identifier, RunContext, RuntimeError, TargetItemValue, TargetPixelValue,
    Value,
};
use crate::effect::BuiltinEffect;
use crate::sampling::{
    curve_crossing, deterministic_random, hsv, mix_colors, sample_curve, sample_gradient,
    scale_color,
};
use crate::values::{Color, Curve, Gradient, Marks};
use indexmap::IndexMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GradientMode {
    ThroughEffect,
    AcrossItems,
    PerPulse,
}

#[derive(Clone, Debug)]
pub enum BoundNativeEffect {
    Sample(NativeSample),
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
        revolutions: i64,
    },
    MarkPulseChild(MarkPulseChild),
    MarkChaseChild(MarkChaseChild),
}

#[derive(Clone, Debug)]
pub struct NativeGeneratedEffect {
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub target: Arc<TargetItemValue>,
    pub sample: NativeSample,
}

#[derive(Clone, Debug)]
pub struct Chase {
    gradient: Arc<Gradient>,
    gradient_mode: GradientMode,
    pulse_overlap: f64,
    section_width_pixels: i64,
    chase_position: Arc<Curve>,
    reverse: bool,
    extend_to_start: bool,
    extend_to_end: bool,
    pulse_shape: Arc<Curve>,
}

#[derive(Clone, Debug)]
pub struct MarkPulse {
    beats: Arc<Marks>,
    base: Color,
    accent: Arc<Gradient>,
    hue: Arc<Curve>,
    hue_mix: f64,
    offset_seconds: f64,
    decay_seconds: f64,
    section_width_pixels: i64,
    section_edge_fade_pixels: f64,
    sections_per_mark: i64,
    seed: f64,
}

#[derive(Clone, Debug)]
pub struct MarkPulseChild {
    base: Color,
    accent: Arc<Gradient>,
    hue: Arc<Curve>,
    hue_mix: f64,
    section_width_pixels: i64,
    section_edge_fade_pixels: f64,
    parent_duration: f64,
    child_start: f64,
}

#[derive(Clone, Debug)]
pub struct MarkChase {
    beats: Arc<Marks>,
    base: Color,
    gradient_mode: GradientMode,
    gradients: Vec<Arc<Gradient>>,
    hue: Arc<Curve>,
    hue_mix: f64,
    offset_seconds: f64,
    chase_seconds: f64,
    pulse_overlap: f64,
    section_width_pixels: i64,
    chase_positions: Vec<Arc<Curve>>,
    pulse_shape: Arc<Curve>,
}

#[derive(Clone, Debug)]
pub struct MarkChaseChild {
    base: Color,
    gradient_mode: GradientMode,
    gradient: Arc<Gradient>,
    hue: Arc<Curve>,
    hue_mix: f64,
    pulse_overlap: f64,
    section_width_pixels: i64,
    chase_position: Arc<Curve>,
    pulse_shape: Arc<Curve>,
    parent_duration: f64,
    child_start: f64,
}

pub fn bind(
    builtin: BuiltinEffect,
    overrides: &IndexMap<Identifier, Value>,
) -> Result<BoundNativeEffect, RuntimeError> {
    let mut params = builtin
        .definition()
        .params
        .iter()
        .filter_map(|param| {
            param
                .default
                .clone()
                .map(|value| (param.name.clone(), value))
        })
        .collect::<IndexMap<_, _>>();
    params.extend(
        overrides
            .iter()
            .map(|(name, value)| (name.clone(), value.clone())),
    );
    for name in overrides.keys() {
        if !builtin
            .definition()
            .params
            .iter()
            .any(|param| &param.name == name)
        {
            return Err(error(format!(
                "unknown native parameter `{}`",
                name.as_str()
            )));
        }
    }
    Ok(match builtin {
        BuiltinEffect::Pulse => BoundNativeEffect::Sample(NativeSample::Pulse {
            gradient: gradient(&params, "gradient")?,
            pulse_shape: curve(&params, "pulse_shape")?,
        }),
        BuiltinEffect::Chase => BoundNativeEffect::Sample(NativeSample::Chase(chase(&params)?)),
        BuiltinEffect::Spin => BoundNativeEffect::Sample(NativeSample::Spin {
            chase: chase(&params)?,
            revolutions: int(&params, "revolutions")?,
        }),
        BuiltinEffect::MarkPulse => BoundNativeEffect::MarkPulse(MarkPulse {
            beats: marks(&params, "beats")?,
            base: color(&params, "base")?,
            accent: gradient(&params, "accent")?,
            hue: curve(&params, "hue")?,
            hue_mix: float(&params, "hue_mix")?,
            offset_seconds: float(&params, "offset_seconds")?,
            decay_seconds: float(&params, "decay_seconds")?,
            section_width_pixels: int(&params, "section_width_pixels")?,
            section_edge_fade_pixels: float(&params, "section_edge_fade_pixels")?,
            sections_per_mark: int(&params, "sections_per_mark")?,
            seed: float(&params, "seed")?,
        }),
        BuiltinEffect::MarkChase => BoundNativeEffect::MarkChase(MarkChase {
            beats: marks(&params, "beats")?,
            base: color(&params, "base")?,
            gradient_mode: gradient_mode(&params)?,
            gradients: gradient_array(&params, "gradients")?,
            hue: curve(&params, "hue")?,
            hue_mix: float(&params, "hue_mix")?,
            offset_seconds: float(&params, "offset_seconds")?,
            chase_seconds: float(&params, "chase_seconds")?,
            pulse_overlap: float(&params, "pulse_overlap")?,
            section_width_pixels: int(&params, "section_width_pixels")?,
            chase_positions: curve_array(&params, "chase_positions")?,
            pulse_shape: curve(&params, "pulse_shape")?,
        }),
    })
}

fn chase(params: &IndexMap<Identifier, Value>) -> Result<Chase, RuntimeError> {
    Ok(Chase {
        gradient: gradient(params, "gradient")?,
        gradient_mode: gradient_mode(params)?,
        pulse_overlap: float(params, "pulse_overlap")?,
        section_width_pixels: int(params, "section_width_pixels")?,
        chase_position: curve(params, "chase_position")?,
        reverse: boolean(params, "reverse")?,
        extend_to_start: boolean(params, "extend_to_start")?,
        extend_to_end: boolean(params, "extend_to_end")?,
        pulse_shape: curve(params, "pulse_shape")?,
    })
}

impl NativeSample {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Pulse { .. } => "Pulse",
            Self::Chase(_) => "Chase",
            Self::Spin { .. } => "Spin",
            Self::MarkPulseChild(_) => "Mark Pulse",
            Self::MarkChaseChild(_) => "Mark Chase",
        }
    }

    pub fn sample(&self, context: &RunContext) -> Result<Color, RuntimeError> {
        match self {
            Self::Pulse {
                gradient,
                pulse_shape,
            } => gradient_scaled(
                gradient,
                context.progress,
                sample_curve(pulse_shape, context.progress),
            ),
            Self::Chase(chase) => chase.sample(context),
            Self::Spin { chase, revolutions } => chase.sample_spin(context, *revolutions),
            Self::MarkPulseChild(child) => child.sample(context),
            Self::MarkChaseChild(child) => child.sample(context),
        }
    }
}

impl Chase {
    fn sample(&self, context: &RunContext) -> Result<Color, RuntimeError> {
        let width = (self.section_width_pixels as f64).max(1.0);
        let count = ((context.pixel_count as f64 + width - 1.0) / width)
            .floor()
            .max(1.0);
        let position = (context.pixel_index as f64 / width).floor() / (count - 1.0).max(1.0);
        let chase_value = if self.reverse {
            1.0 - position
        } else {
            position
        };
        let duration = (self.pulse_overlap.max(1.0) / count).max(1e-9);
        let start = if self.extend_to_start { -duration } else { 0.0 };
        let end = if self.extend_to_end {
            1.0
        } else {
            (1.0 - duration).max(0.0)
        };
        let hit = start
            + (end - start)
                * curve_crossing(&self.chase_position, chase_value, chase_value).clamp(0.0, 1.0);
        let pulse_progress = (context.progress - hit) / duration;
        let level = if (0.0..=1.0).contains(&pulse_progress) {
            sample_curve(&self.pulse_shape, pulse_progress).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let gradient_position = match self.gradient_mode {
            GradientMode::ThroughEffect => context.progress,
            GradientMode::AcrossItems => position,
            GradientMode::PerPulse => pulse_progress,
        };
        gradient_scaled(&self.gradient, gradient_position.clamp(0.0, 1.0), level)
    }
    fn sample_spin(&self, context: &RunContext, revolutions: i64) -> Result<Color, RuntimeError> {
        let width = (self.section_width_pixels as f64).max(1.0);
        let count = ((context.pixel_count as f64 + width - 1.0) / width)
            .floor()
            .max(1.0);
        let position = (context.pixel_index as f64 / width).floor() / (count - 1.0).max(1.0);
        let revolutions = revolutions.max(1);
        let virtual_count = count * revolutions as f64;
        let duration = (self.pulse_overlap.max(1.0) / virtual_count).max(1e-9);
        let start = if self.extend_to_start { -duration } else { 0.0 };
        let end = if self.extend_to_end {
            1.0
        } else {
            (1.0 - duration).max(0.0)
        };
        let mut level = 0.0;
        let mut gradient_position = 0.0;
        for revolution in 0..revolutions {
            let mut virtual_position = (revolution as f64 + position) / revolutions as f64;
            if self.reverse {
                virtual_position = 1.0 - virtual_position;
            }
            let hit = start
                + (end - start)
                    * curve_crossing(&self.chase_position, virtual_position, virtual_position)
                        .clamp(0.0, 1.0);
            let pulse_progress = (context.progress - hit) / duration;
            if (0.0..=1.0).contains(&pulse_progress) {
                let candidate = sample_curve(&self.pulse_shape, pulse_progress).clamp(0.0, 1.0);
                if candidate > level {
                    level = candidate;
                    gradient_position = match self.gradient_mode {
                        GradientMode::ThroughEffect => context.progress,
                        GradientMode::AcrossItems => position,
                        GradientMode::PerPulse => pulse_progress,
                    };
                }
            }
        }
        gradient_scaled(&self.gradient, gradient_position.clamp(0.0, 1.0), level)
    }
}

impl BoundNativeEffect {
    pub fn generate(
        &self,
        context: &GeneratorContext,
    ) -> Result<Vec<NativeGeneratedEffect>, RuntimeError> {
        match self {
            Self::MarkPulse(value) => value.generate(context),
            Self::MarkChase(value) => value.generate(context),
            Self::Sample(_) => Err(error("sample effect cannot generate children")),
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
            let hit = mark.as_seconds_f64();
            for index in 0..self.sections_per_mark {
                let choice =
                    (deterministic_random([self.seed + hit * 1000.0, index as f64].into_iter())
                        * sections.len() as f64)
                        .floor() as usize;
                generated.push(NativeGeneratedEffect {
                    start_seconds: hit + self.offset_seconds,
                    duration_seconds: self.decay_seconds.max(1e-9),
                    target: Arc::new(TargetItemValue {
                        pixels: Arc::new(sections[choice].clone()),
                    }),
                    sample: NativeSample::MarkPulseChild(MarkPulseChild {
                        base: self.base,
                        accent: Arc::clone(&self.accent),
                        hue: Arc::clone(&self.hue),
                        hue_mix: self.hue_mix,
                        section_width_pixels: self.section_width_pixels,
                        section_edge_fade_pixels: self.section_edge_fade_pixels,
                        parent_duration: context.duration,
                        child_start: hit + self.offset_seconds,
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
                pixels: Arc::new(
                    context
                        .target
                        .groups
                        .iter()
                        .flat_map(|group| group.pixels.iter().copied())
                        .collect(),
                ),
            })
        };
        Ok(self
            .beats
            .marks
            .iter()
            .enumerate()
            .map(|(index, mark)| {
                let hit = mark.as_seconds_f64();
                NativeGeneratedEffect {
                    start_seconds: hit + self.offset_seconds,
                    duration_seconds: self.chase_seconds.max(1e-9),
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
                        parent_duration: context.duration,
                        child_start: hit + self.offset_seconds,
                    }),
                }
            })
            .collect())
    }
}

impl MarkPulseChild {
    fn sample(&self, c: &RunContext) -> Result<Color, RuntimeError> {
        let width = (self.section_width_pixels as f64).max(1.0);
        let pixel = c.pixel_index as f64;
        let choice = (pixel / width).floor();
        let fade = self.section_edge_fade_pixels.max(0.0);
        let active = if fade > 0.0 {
            let start = choice * width;
            let end = (start + width - 1.0).min(c.pixel_count as f64 - 1.0);
            ((pixel - start).min(end - pixel) / fade).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let progress = c.progress.clamp(0.0, 1.0);
        let parent =
            ((self.child_start + c.seconds) / self.parent_duration.max(1e-9)).clamp(0.0, 1.0);
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
    fn sample(&self, c: &RunContext) -> Result<Color, RuntimeError> {
        let width = (self.section_width_pixels as f64).max(1.0);
        let pixel = c.pixel_index as f64;
        let section = (pixel - (pixel / width).floor() * width) / width;
        let travel_start = -self.pulse_overlap / (c.pixel_count as f64).max(1.0);
        let travel_end = 1.0 + self.pulse_overlap / (c.pixel_count as f64).max(1.0);
        let chase = travel_start
            + (travel_end - travel_start)
                * sample_curve(&self.chase_position, c.progress).clamp(0.0, 1.0);
        let pulse_progress = (c.pixel_fraction - chase).abs()
            / (self.pulse_overlap / (c.pixel_count as f64).max(1.0)).max(1e-9);
        let level = sample_curve(&self.pulse_shape, pulse_progress.clamp(0.0, 1.0));
        let gp = match self.gradient_mode {
            GradientMode::ThroughEffect => c.progress,
            GradientMode::AcrossItems => c.pixel_fraction,
            GradientMode::PerPulse => section,
        };
        let parent =
            ((self.child_start + c.seconds) / self.parent_duration.max(1e-9)).clamp(0.0, 1.0);
        let value = gradient_scaled(&self.gradient, gp.clamp(0.0, 1.0), level)?;
        let hue = hsv(sample_curve(&self.hue, parent) / 360.0, 1.0, 1.0);
        Ok(mix_colors(
            self.base,
            mix_colors(value, hue, self.hue_mix.clamp(0.0, 1.0)),
            level,
        ))
    }
}

fn gradient_scaled(g: &Gradient, p: f64, level: f64) -> Result<Color, RuntimeError> {
    sample_gradient(g, p)
        .map(|c| scale_color(c, level))
        .ok_or_else(|| error("cannot sample empty gradient"))
}
fn get<'a>(p: &'a IndexMap<Identifier, Value>, n: &str) -> Result<&'a Value, RuntimeError> {
    p.iter()
        .find_map(|(k, v)| (k.as_str() == n).then_some(v))
        .ok_or_else(|| error(format!("missing native parameter `{n}`")))
}
fn float(p: &IndexMap<Identifier, Value>, n: &str) -> Result<f64, RuntimeError> {
    match get(p, n)? {
        Value::Float(v) => Ok(*v),
        Value::Int(v) => Ok(*v as f64),
        _ => Err(error(format!("native parameter `{n}` has wrong type"))),
    }
}
fn int(p: &IndexMap<Identifier, Value>, n: &str) -> Result<i64, RuntimeError> {
    match get(p, n)? {
        Value::Int(v) => Ok(*v),
        _ => Err(error(format!("native parameter `{n}` has wrong type"))),
    }
}
fn boolean(p: &IndexMap<Identifier, Value>, n: &str) -> Result<bool, RuntimeError> {
    match get(p, n)? {
        Value::Bool(v) => Ok(*v),
        _ => Err(error(format!("native parameter `{n}` has wrong type"))),
    }
}
fn color(p: &IndexMap<Identifier, Value>, n: &str) -> Result<Color, RuntimeError> {
    match get(p, n)? {
        Value::Color(v) => Ok(*v),
        _ => Err(error(format!("native parameter `{n}` has wrong type"))),
    }
}
fn curve(p: &IndexMap<Identifier, Value>, n: &str) -> Result<Arc<Curve>, RuntimeError> {
    match get(p, n)? {
        Value::Curve(v) => Ok(Arc::clone(v)),
        _ => Err(error(format!("native parameter `{n}` has wrong type"))),
    }
}
fn gradient(p: &IndexMap<Identifier, Value>, n: &str) -> Result<Arc<Gradient>, RuntimeError> {
    match get(p, n)? {
        Value::Gradient(v) => Ok(Arc::clone(v)),
        _ => Err(error(format!("native parameter `{n}` has wrong type"))),
    }
}
fn marks(p: &IndexMap<Identifier, Value>, n: &str) -> Result<Arc<Marks>, RuntimeError> {
    match get(p, n)? {
        Value::Marks(v) => Ok(Arc::clone(v)),
        _ => Err(error(format!("native parameter `{n}` has wrong type"))),
    }
}
fn gradient_mode(p: &IndexMap<Identifier, Value>) -> Result<GradientMode, RuntimeError> {
    match get(p, "gradient_mode")? {
        Value::Enum(v) => match v.as_str() {
            "through_effect" => Ok(GradientMode::ThroughEffect),
            "across_items" => Ok(GradientMode::AcrossItems),
            "per_pulse" => Ok(GradientMode::PerPulse),
            _ => Err(error("unsupported gradient mode")),
        },
        _ => Err(error("native parameter `gradient_mode` has wrong type")),
    }
}
fn gradient_array(
    p: &IndexMap<Identifier, Value>,
    n: &str,
) -> Result<Vec<Arc<Gradient>>, RuntimeError> {
    match get(p, n)? {
        Value::Array(v) => v
            .iter()
            .map(|v| match v {
                Value::Gradient(v) => Ok(Arc::clone(v)),
                _ => Err(error(format!("native parameter `{n}` has wrong type"))),
            })
            .collect(),
        _ => Err(error(format!("native parameter `{n}` has wrong type"))),
    }
}
fn curve_array(p: &IndexMap<Identifier, Value>, n: &str) -> Result<Vec<Arc<Curve>>, RuntimeError> {
    match get(p, n)? {
        Value::Array(v) => v
            .iter()
            .map(|v| match v {
                Value::Curve(v) => Ok(Arc::clone(v)),
                _ => Err(error(format!("native parameter `{n}` has wrong type"))),
            })
            .collect(),
        _ => Err(error(format!("native parameter `{n}` has wrong type"))),
    }
}
fn error(message: impl Into<String>) -> RuntimeError {
    RuntimeError {
        message: message.into(),
    }
}
