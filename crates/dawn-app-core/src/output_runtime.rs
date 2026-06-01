use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use dawn_project::analysis::{effect_script_key, split_effect_script_key, ProjectAnalysis};
use dawn_project::document::{
    SequenceDocument, SequenceEffectParamDocument, SequenceEffectPixelDocument,
    SequenceMarkCollectionDocument,
};
use dawn_project::effect_script::{
    run_generator, BytecodeStats, CompiledEffect, EffectSampleScratch, EffectScriptKind,
    FixtureContext, GeneratedChildEffectRef, GeneratorTarget, GeneratorTargetPixel, PixelContext,
    PreparedEffectParams, RuntimeError, RuntimeValue,
};
use dawn_project::frame::{ceil_frame, floor_frame, frame_count};
use dawn_project::model::{
    Color, Distance, DistanceSpan, EffectParam, FixtureId, Resolved, SequenceEffectScope, Time,
    TimeSpan,
};
use dawn_project::path::{resolve_import_path, Utf8PathBuf};
use dawn_project::render::{layout_render_plan, GeometryRenderBounds, GeometryRenderPoint};

#[derive(Debug, Clone)]
pub struct OutputFrame {
    pub source: OutputSourceMetadata,
    pub time_seconds: f64,
    pub generation: u64,
    pub status: OutputFrameStatus,
    pub bounds: GeometryRenderBounds,
    pub fixtures: Vec<OutputFixtureFrame>,
}

#[derive(Debug, Clone)]
pub struct OutputSourceMetadata {
    pub label: String,
    pub kind: OutputSourceKind,
    pub duration_seconds: f64,
    pub fps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputSourceKind {
    Sequence,
    Empty,
}

#[derive(Debug, Clone)]
pub enum OutputFrameStatus {
    Live,
    Idle(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct OutputFixtureFrame {
    pub id: FixtureId,
    pub name: String,
    pub bulb_radius: DistanceSpan,
    pub pixels: Vec<OutputPixelFrame>,
}

#[derive(Debug, Clone)]
pub struct OutputPixelFrame {
    pub position: GeometryRenderPoint,
    pub color: Color,
}

pub trait OutputSink {
    fn write_frame(&self, frame: OutputFrame);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SequenceFrameEvaluationTiming {
    pub total_ms: f64,
    pub fixture_clone_ms: f64,
    pub effect_loop_ms: f64,
    pub output_frame_ms: f64,
    pub active_effects: u32,
    pub active_authored_effects: u32,
    pub active_prepared_effects: u32,
    pub visited_prepared_effects: u32,
    pub sampled_pixels: u32,
}

#[derive(Debug, Clone, Default)]
pub struct SequenceFrameEvaluatorPreparationTiming {
    pub total_ms: f64,
    pub layout_template_ms: f64,
    pub authored_sample_ms: f64,
    pub generator_expansion_ms: f64,
    pub timeline_index_ms: f64,
    pub prepared_effect_count: usize,
    pub generator_parent_count: usize,
    pub generated_child_count: usize,
    pub generator_parents: Vec<GeneratorParentPreparationTiming>,
}

#[derive(Debug, Clone)]
pub struct GeneratorParentPreparationTiming {
    pub parent_effect_id: u32,
    pub script_key: String,
    pub target_pixels: usize,
    pub emitted_children: usize,
    pub prepared_children: usize,
    pub total_prepare_ms: f64,
}

#[derive(Debug, Clone)]
pub struct SequenceFrameEvaluator {
    source: OutputSourceMetadata,
    bounds: GeometryRenderBounds,
    fixture_templates: Vec<OutputFixtureFrame>,
    effects: Vec<PreparedSequenceEffect>,
    effect_indices_by_frame: Vec<Vec<usize>>,
    authored_intervals_by_id: HashMap<u32, EffectInterval>,
}

impl SequenceFrameEvaluator {
    pub fn new(analysis: &ProjectAnalysis, document: &SequenceDocument) -> Result<Self, String> {
        Self::new_filtered(analysis, document, None)
    }

    pub fn new_timed(
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
    ) -> Result<(Self, SequenceFrameEvaluatorPreparationTiming), String> {
        Self::new_filtered_timed(analysis, document, None)
    }

    pub fn new_filtered(
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        effect_filter: Option<&HashSet<u32>>,
    ) -> Result<Self, String> {
        Self::new_filtered_timed(analysis, document, effect_filter)
            .map(|(evaluator, _timing)| evaluator)
    }

    pub fn new_filtered_timed(
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        effect_filter: Option<&HashSet<u32>>,
    ) -> Result<(Self, SequenceFrameEvaluatorPreparationTiming), String> {
        let total_started = Instant::now();
        let Some(project) = analysis.resolved.as_ref() else {
            return Err("Project must resolve before preview is available".to_string());
        };
        let layout_started = Instant::now();
        let render_plan = layout_render_plan(&project.display.layout.fixtures);
        let fixture_templates = render_plan
            .fixtures
            .iter()
            .zip(project.display.layout.fixtures.iter())
            .map(|(plan, fixture)| OutputFixtureFrame {
                id: fixture.id,
                name: fixture.name.clone(),
                bulb_radius: plan.bulb_radius,
                pixels: plan
                    .emitters
                    .iter()
                    .map(|position| OutputPixelFrame {
                        position: *position,
                        color: Color::new(0, 0, 0),
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        let layout_template_ms = elapsed_ms(layout_started);

        let mut effects = Vec::new();
        let mut authored_sample_ms = 0.0;
        let mut generator_expansion_ms = 0.0;
        let mut generator_parent_count = 0;
        let mut generated_child_count = 0;
        let mut generator_parents = Vec::new();
        for effect in document.effects.iter().filter(|effect| {
            effect_filter
                .map(|ids| ids.contains(&effect.id))
                .unwrap_or(true)
        }) {
            let Some(render) = effect.render.as_ref() else {
                continue;
            };
            match analysis.compiled_script_for_key(&render.script_key) {
                Some(script) if script.kind == EffectScriptKind::Generator => {
                    generator_parent_count += 1;
                    let generator_started = Instant::now();
                    match prepare_generated_effects(
                        analysis,
                        document,
                        effect.id,
                        effect.start_seconds,
                        effect.duration_seconds,
                        effect.scope,
                        script,
                        render,
                        &fixture_templates,
                    ) {
                        Ok(children) => {
                            let total_prepare_ms = elapsed_ms(generator_started);
                            generator_expansion_ms += total_prepare_ms;
                            generated_child_count += children.len();
                            generator_parents.push(GeneratorParentPreparationTiming {
                                parent_effect_id: effect.id,
                                script_key: render.script_key.clone(),
                                target_pixels: render.target_pixels.len(),
                                emitted_children: children.len(),
                                prepared_children: children.len(),
                                total_prepare_ms,
                            });
                            effects.extend(children);
                        }
                        Err(error) => {
                            let total_prepare_ms = elapsed_ms(generator_started);
                            generator_expansion_ms += total_prepare_ms;
                            generator_parents.push(GeneratorParentPreparationTiming {
                                parent_effect_id: effect.id,
                                script_key: render.script_key.clone(),
                                target_pixels: render.target_pixels.len(),
                                emitted_children: 0,
                                prepared_children: 0,
                                total_prepare_ms,
                            });
                            effects.push(PreparedSequenceEffect {
                                id: effect.id,
                                start_seconds: effect.start_seconds,
                                duration_seconds: effect.duration_seconds,
                                authored: true,
                                render: PreparedEffectRender::BadParams(error),
                            });
                        }
                    }
                }
                Some(script) => {
                    let sample_started = Instant::now();
                    let prepared_render = prepare_sample_render(
                        script,
                        &render.params,
                        &document.mark_collections,
                        effect.start_seconds,
                        effect.scope,
                        &render.target_pixels,
                        &fixture_templates,
                    );
                    authored_sample_ms += elapsed_ms(sample_started);
                    effects.push(PreparedSequenceEffect {
                        id: effect.id,
                        start_seconds: effect.start_seconds,
                        duration_seconds: effect.duration_seconds,
                        authored: true,
                        render: prepared_render,
                    });
                }
                None => effects.push(PreparedSequenceEffect {
                    id: effect.id,
                    start_seconds: effect.start_seconds,
                    duration_seconds: effect.duration_seconds,
                    authored: true,
                    render: PreparedEffectRender::MissingScript(render.script_key.clone()),
                }),
            }
        }

        let source = OutputSourceMetadata {
            label: format!("Sequence {}", document.object_key),
            kind: OutputSourceKind::Sequence,
            duration_seconds: document.duration_seconds,
            fps: document.frame_rate,
        };
        let timeline_started = Instant::now();
        let effect_indices_by_frame =
            build_effect_indices_by_frame(&effects, source.duration_seconds, source.fps);
        let timeline_index_ms = elapsed_ms(timeline_started);
        let authored_intervals_by_id = document
            .effects
            .iter()
            .map(|effect| {
                (
                    effect.id,
                    EffectInterval {
                        start_seconds: effect.start_seconds,
                        duration_seconds: effect.duration_seconds,
                    },
                )
            })
            .collect();

        let timing = SequenceFrameEvaluatorPreparationTiming {
            total_ms: elapsed_ms(total_started),
            layout_template_ms,
            authored_sample_ms,
            generator_expansion_ms,
            timeline_index_ms,
            prepared_effect_count: effects.len(),
            generator_parent_count,
            generated_child_count,
            generator_parents,
        };

        Ok((
            Self {
                source,
                bounds: render_plan.bounds,
                fixture_templates,
                effects,
                effect_indices_by_frame,
                authored_intervals_by_id,
            },
            timing,
        ))
    }

    pub fn evaluate(&mut self, time_seconds: f64, generation: u64) -> OutputFrame {
        self.evaluate_timed(time_seconds, generation).0
    }

    pub fn evaluate_timed(
        &mut self,
        time_seconds: f64,
        generation: u64,
    ) -> (OutputFrame, SequenceFrameEvaluationTiming) {
        let total_started = Instant::now();
        let clone_started = Instant::now();
        let mut fixtures = self.fixture_templates.clone();
        let fixture_clone_ms = elapsed_ms(clone_started);
        let mut status = OutputFrameStatus::Live;
        let mut counters = SequenceEffectEvaluationCounters::default();

        let effect_loop_started = Instant::now();
        if let Some(effect_indices) = self.effect_indices_for_time(time_seconds) {
            for effect_index in effect_indices.clone() {
                evaluate_prepared_effect_at_time(
                    &mut self.effects[effect_index],
                    time_seconds,
                    &mut fixtures,
                    &mut status,
                    &mut counters,
                );
            }
        }
        let effect_loop_ms = elapsed_ms(effect_loop_started);

        let output_started = Instant::now();
        let frame = self.output_frame(time_seconds, generation, status, fixtures);
        let output_frame_ms = elapsed_ms(output_started);
        let total_ms = elapsed_ms(total_started);
        (
            frame,
            SequenceFrameEvaluationTiming {
                total_ms,
                fixture_clone_ms,
                effect_loop_ms,
                output_frame_ms,
                active_effects: counters.active_prepared_effects,
                active_authored_effects: counters.active_authored_effects,
                active_prepared_effects: counters.active_prepared_effects,
                visited_prepared_effects: counters.visited_prepared_effects,
                sampled_pixels: counters.sampled_pixels,
            },
        )
    }

    pub fn evaluate_effect_preview(
        &mut self,
        preview_seconds: f64,
        generation: u64,
    ) -> OutputFrame {
        self.evaluate_effect_preview_filtered(preview_seconds, generation, None)
    }

    pub fn evaluate_effect_preview_filtered(
        &mut self,
        preview_seconds: f64,
        generation: u64,
        effect_filter: Option<&HashSet<u32>>,
    ) -> OutputFrame {
        self.evaluate_effect_preview_filtered_timed(preview_seconds, generation, effect_filter)
            .0
    }

    pub fn evaluate_effect_preview_filtered_timed(
        &mut self,
        preview_seconds: f64,
        generation: u64,
        effect_filter: Option<&HashSet<u32>>,
    ) -> (OutputFrame, SequenceFrameEvaluationTiming) {
        let total_started = Instant::now();
        let clone_started = Instant::now();
        let mut fixtures = self.fixture_templates.clone();
        let fixture_clone_ms = elapsed_ms(clone_started);
        let mut status = OutputFrameStatus::Live;
        let mut counters = SequenceEffectEvaluationCounters::default();

        let effect_loop_started = Instant::now();
        let preview_frame_times = self.preview_frame_times(preview_seconds, effect_filter);
        let mut visited_effect_indices = HashSet::new();
        for (preview_id, preview_frame_time) in preview_frame_times {
            if let Some(effect_indices) = self.effect_indices_for_time(preview_frame_time) {
                for effect_index in effect_indices.clone() {
                    if !visited_effect_indices.insert(effect_index) {
                        continue;
                    }
                    let effect = &mut self.effects[effect_index];
                    if effect.id != preview_id {
                        continue;
                    }
                    if effect_filter
                        .map(|ids| !ids.contains(&effect.id))
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    evaluate_prepared_effect_at_time(
                        effect,
                        preview_frame_time,
                        &mut fixtures,
                        &mut status,
                        &mut counters,
                    );
                }
            }
        }
        let effect_loop_ms = elapsed_ms(effect_loop_started);

        let output_started = Instant::now();
        let frame = self.output_frame(preview_seconds, generation, status, fixtures);
        let output_frame_ms = elapsed_ms(output_started);
        let total_ms = elapsed_ms(total_started);
        (
            frame,
            SequenceFrameEvaluationTiming {
                total_ms,
                fixture_clone_ms,
                effect_loop_ms,
                output_frame_ms,
                active_effects: counters.active_prepared_effects,
                active_authored_effects: counters.active_authored_effects,
                active_prepared_effects: counters.active_prepared_effects,
                visited_prepared_effects: counters.visited_prepared_effects,
                sampled_pixels: counters.sampled_pixels,
            },
        )
    }

    pub fn prepared_effect_count(&self) -> usize {
        self.effects.len()
    }

    pub fn evaluate_generator_effect_thumbnail(
        &mut self,
        effect_id: u32,
        local_seconds_by_column: &[f64],
        sampled_pixels_by_row: &[SequenceEffectPixelDocument],
    ) -> Result<Vec<Color>, String> {
        let interval = self
            .authored_intervals_by_id
            .get(&effect_id)
            .ok_or_else(|| format!("sequence effect `{effect_id}` was not found"))?;
        if interval.duration_seconds <= 0.0 {
            return Err(format!(
                "sequence effect `{effect_id}` must have a positive duration"
            ));
        }

        let mut row_indices_by_pixel = HashMap::new();
        for (row_index, pixel) in sampled_pixels_by_row.iter().enumerate() {
            if self
                .fixture_templates
                .get(pixel.fixture_index)
                .and_then(|fixture| fixture.pixels.get(pixel.pixel_index))
                .is_none()
            {
                return Err(format!(
                    "sequence effect `{effect_id}` references an unavailable preview pixel"
                ));
            }
            row_indices_by_pixel.insert((pixel.fixture_index, pixel.pixel_index), row_index);
        }

        let columns = local_seconds_by_column.len();
        let rows = sampled_pixels_by_row.len();
        let mut colors = vec![Color::new(0, 0, 0); columns * rows];

        for (column_index, local_seconds) in local_seconds_by_column.iter().copied().enumerate() {
            if !local_seconds.is_finite() || local_seconds < 0.0 {
                return Err(format!(
                    "sequence effect `{effect_id}` has an invalid preview sample time"
                ));
            }
            let sequence_seconds = interval.start_seconds + local_seconds;
            let Some(effect_indices) = self.effect_indices_for_time(sequence_seconds).cloned()
            else {
                continue;
            };

            for effect_index in effect_indices {
                let effect = &mut self.effects[effect_index];
                if effect.id != effect_id {
                    continue;
                }
                sample_prepared_effect_thumbnail_column(
                    effect,
                    sequence_seconds,
                    column_index,
                    columns,
                    &row_indices_by_pixel,
                    &mut colors,
                )?;
            }
        }

        Ok(colors)
    }

    fn effect_indices_for_time(&self, time_seconds: f64) -> Option<&Vec<usize>> {
        if !time_seconds.is_finite() || time_seconds < 0.0 {
            return None;
        }
        let frame_index = floor_frame(time_from_seconds_clamped(time_seconds), self.source.fps);
        self.effect_indices_by_frame
            .get(usize::try_from(frame_index).ok()?)
    }

    fn preview_frame_times(
        &self,
        preview_seconds: f64,
        effect_filter: Option<&HashSet<u32>>,
    ) -> Vec<(u32, f64)> {
        let ids = match effect_filter {
            Some(ids) => ids.iter().copied().collect::<Vec<_>>(),
            None => self
                .authored_intervals_by_id
                .keys()
                .copied()
                .collect::<Vec<_>>(),
        };
        ids.into_iter()
            .filter_map(|id| {
                let interval = self.authored_intervals_by_id.get(&id)?;
                (interval.duration_seconds > 0.0).then(|| {
                    (
                        id,
                        interval.start_seconds
                            + preview_seconds.rem_euclid(interval.duration_seconds),
                    )
                })
            })
            .collect()
    }

    fn output_frame(
        &self,
        time_seconds: f64,
        generation: u64,
        status: OutputFrameStatus,
        fixtures: Vec<OutputFixtureFrame>,
    ) -> OutputFrame {
        OutputFrame {
            source: self.source.clone(),
            time_seconds,
            generation,
            status,
            bounds: self.bounds,
            fixtures,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct EffectInterval {
    start_seconds: f64,
    duration_seconds: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct SequenceEffectEvaluationCounters {
    active_authored_effects: u32,
    active_prepared_effects: u32,
    visited_prepared_effects: u32,
    sampled_pixels: u32,
}

fn evaluate_prepared_effect_at_time(
    effect: &mut PreparedSequenceEffect,
    time_seconds: f64,
    fixtures: &mut [OutputFixtureFrame],
    status: &mut OutputFrameStatus,
    counters: &mut SequenceEffectEvaluationCounters,
) {
    counters.visited_prepared_effects = counters.visited_prepared_effects.saturating_add(1);
    let local_seconds =
        if time_seconds < effect.start_seconds || time_seconds >= effect.end_seconds() {
            return;
        } else {
            time_seconds - effect.start_seconds
        };
    sample_prepared_effect(effect, local_seconds, fixtures, status, counters);
}

fn sample_prepared_effect(
    effect: &mut PreparedSequenceEffect,
    local_seconds: f64,
    fixtures: &mut [OutputFixtureFrame],
    status: &mut OutputFrameStatus,
    counters: &mut SequenceEffectEvaluationCounters,
) {
    let progress = if effect.duration_seconds == 0.0 {
        0.0
    } else {
        (local_seconds / effect.duration_seconds).clamp(0.0, 1.0)
    };

    let PreparedEffectRender::Ready {
        script,
        target_pixels,
        prepared_params,
        scratch,
        ..
    } = &mut effect.render
    else {
        *status = effect.render.error_status();
        return;
    };

    if effect.authored {
        counters.active_authored_effects = counters.active_authored_effects.saturating_add(1);
    }
    counters.active_prepared_effects = counters.active_prepared_effects.saturating_add(1);
    for pixel in target_pixels {
        let output_pixel = &mut fixtures[pixel.fixture_index].pixels[pixel.pixel_index];
        match script.sample_prepared_with_scratch(
            progress,
            local_seconds,
            pixel.fixture_context,
            pixel.pixel_context,
            prepared_params,
            scratch,
        ) {
            Ok(color) => add_clamped(&mut output_pixel.color, color),
            Err(error) => *status = OutputFrameStatus::Error(error.to_string()),
        }
        counters.sampled_pixels = counters.sampled_pixels.saturating_add(1);
    }
}

fn sample_prepared_effect_thumbnail_column(
    effect: &mut PreparedSequenceEffect,
    sequence_seconds: f64,
    column_index: usize,
    columns: usize,
    row_indices_by_pixel: &HashMap<(usize, usize), usize>,
    colors: &mut [Color],
) -> Result<(), String> {
    let local_seconds =
        if sequence_seconds < effect.start_seconds || sequence_seconds >= effect.end_seconds() {
            return Ok(());
        } else {
            sequence_seconds - effect.start_seconds
        };
    let progress = if effect.duration_seconds == 0.0 {
        0.0
    } else {
        (local_seconds / effect.duration_seconds).clamp(0.0, 1.0)
    };

    let PreparedEffectRender::Ready {
        script,
        target_pixels,
        prepared_params,
        scratch,
        ..
    } = &mut effect.render
    else {
        return Err(effect.render.error_message());
    };

    for pixel in target_pixels {
        let Some(row_index) = row_indices_by_pixel
            .get(&(pixel.fixture_index, pixel.pixel_index))
            .copied()
        else {
            continue;
        };
        let target_index = row_index
            .checked_mul(columns)
            .and_then(|row_start| row_start.checked_add(column_index))
            .ok_or_else(|| "effect thumbnail raster dimensions overflowed".to_string())?;
        let color = script
            .sample_prepared_with_scratch(
                progress,
                local_seconds,
                pixel.fixture_context,
                pixel.pixel_context,
                prepared_params,
                scratch,
            )
            .map_err(|error| error.to_string())?;
        add_clamped(&mut colors[target_index], color);
    }

    Ok(())
}

fn prepare_sample_render(
    script: &CompiledEffect,
    params: &[SequenceEffectParamDocument],
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
    scope: SequenceEffectScope,
    target_pixels: &[dawn_project::document::SequenceEffectPixelDocument],
    fixture_templates: &[OutputFixtureFrame],
) -> PreparedEffectRender {
    match prepare_params_from_document(script, params, mark_collections, effect_start_seconds) {
        Ok(prepared_params) => PreparedEffectRender::Ready {
            script: script.clone(),
            target_pixels: prepare_effect_pixels(scope, target_pixels, fixture_templates),
            prepared_params,
            scratch: EffectSampleScratch::new(script.bytecode_stats()),
            _bytecode_stats: script.bytecode_stats(),
        },
        Err(error) => PreparedEffectRender::BadParams(error),
    }
}

fn build_effect_indices_by_frame(
    effects: &[PreparedSequenceEffect],
    duration_seconds: f64,
    frame_rate: u32,
) -> Vec<Vec<usize>> {
    let sequence_duration =
        TimeSpan::try_from_seconds_f64_rounded(duration_seconds.max(0.0)).unwrap_or(TimeSpan::ZERO);
    let frame_count = frame_count(sequence_duration, frame_rate);
    let mut indices_by_frame = vec![Vec::new(); frame_count];
    if frame_count == 0 {
        return indices_by_frame;
    }

    for (effect_index, effect) in effects.iter().enumerate() {
        let Some((start_frame, end_frame)) =
            effect_frame_range(effect, duration_seconds, frame_rate, frame_count)
        else {
            continue;
        };
        for frame_indices in &mut indices_by_frame[start_frame..end_frame] {
            frame_indices.push(effect_index);
        }
    }
    indices_by_frame
}

fn effect_frame_range(
    effect: &PreparedSequenceEffect,
    duration_seconds: f64,
    frame_rate: u32,
    frame_count: usize,
) -> Option<(usize, usize)> {
    let sequence_end = duration_seconds.max(0.0);
    let effect_start = effect.start_seconds;
    let effect_end = effect.end_seconds();
    if !effect_start.is_finite()
        || !effect.duration_seconds.is_finite()
        || effect.duration_seconds <= 0.0
        || effect_end <= 0.0
        || effect_start >= sequence_end
    {
        return None;
    }

    let clamped_start = effect_start.max(0.0).min(sequence_end);
    let clamped_end = effect_end.max(0.0).min(sequence_end);
    if clamped_start >= clamped_end {
        return None;
    }

    let start_frame = floor_frame(time_from_seconds_clamped(clamped_start), frame_rate);
    let end_frame = ceil_frame(time_from_seconds_clamped(clamped_end), frame_rate);
    let start_frame = usize::try_from(start_frame).ok()?.min(frame_count);
    let end_frame = usize::try_from(end_frame).ok()?.min(frame_count);
    (start_frame < end_frame).then_some((start_frame, end_frame))
}

fn time_from_seconds_clamped(seconds: f64) -> Time {
    Time::try_from_seconds_f64_rounded(seconds.max(0.0)).unwrap_or(Time::ZERO)
}

fn prepare_generated_effects(
    analysis: &ProjectAnalysis,
    document: &SequenceDocument,
    parent_id: u32,
    parent_start_seconds: f64,
    parent_duration_seconds: f64,
    parent_scope: SequenceEffectScope,
    generator: &CompiledEffect,
    render: &dawn_project::document::SequenceEffectRenderDocument,
    fixture_templates: &[OutputFixtureFrame],
) -> Result<Vec<PreparedSequenceEffect>, RuntimeError> {
    let prepared_params = prepare_params_from_document(
        generator,
        &render.params,
        &document.mark_collections,
        parent_start_seconds,
    )?;
    let param_names = generator
        .params
        .iter()
        .map(|param| param.name.clone())
        .collect::<Vec<_>>();
    let target = GeneratorTarget {
        pixels: render
            .target_pixels
            .iter()
            .map(|pixel| GeneratorTargetPixel {
                fixture_index: pixel.fixture_index,
                pixel_index: pixel.pixel_index,
                pixel_count: pixel.pixel_count,
            })
            .collect(),
    };
    let targets = generator_targets_for_scope(parent_scope, target);
    let statements = generator
        .generator_statements()
        .ok_or_else(|| RuntimeError {
            message: format!("effect `{}` is not a generator effect", generator.name),
        })?;
    let mut children = Vec::new();
    for target in targets {
        children.extend(run_generator(
            statements,
            &prepared_params,
            &param_names,
            target,
            parent_duration_seconds,
        )?);
    }
    let (parent_path, _) = split_effect_script_key(&render.script_key);
    children
        .into_iter()
        .map(|child| {
            let (child_path, child_name, child_label) = match &child.effect {
                GeneratedChildEffectRef::Local { name } => {
                    (parent_path.clone(), name.clone(), name.clone())
                }
                GeneratedChildEffectRef::Imported { alias, name } => {
                    let import = generator
                        .imports
                        .iter()
                        .find(|import| import.alias == *alias)
                        .ok_or_else(|| RuntimeError {
                            message: format!("generator import alias `{alias}` was not found"),
                        })?;
                    (
                        resolve_import_path(&parent_path, &Utf8PathBuf::from(import.path.clone())),
                        name.clone(),
                        format!("{alias}.{name}"),
                    )
                }
            };
            let child_key = effect_script_key(&child_path, &child_name);
            let child_script = analysis
                .compiled_script_for_key(&child_key)
                .ok_or_else(|| RuntimeError {
                    message: format!("compiled child script `{child_key}` was not found"),
                })?;
            if child_script.kind != EffectScriptKind::Sample || child_script.name != child_name {
                return Err(RuntimeError {
                    message: format!("emitted child `{child_label}` is not a sample effect"),
                });
            }
            let prepared_params = child_script.prepare_params(&child.params)?;
            Ok(PreparedSequenceEffect {
                id: parent_id,
                start_seconds: parent_start_seconds + child.start_seconds,
                duration_seconds: child.duration_seconds,
                authored: false,
                render: PreparedEffectRender::Ready {
                    script: child_script.clone(),
                    target_pixels: prepare_effect_pixels(
                        SequenceEffectScope::WholeTarget,
                        &child
                            .target
                            .pixels
                            .iter()
                            .map(
                                |pixel| dawn_project::document::SequenceEffectPixelDocument {
                                    fixture_index: pixel.fixture_index,
                                    pixel_index: pixel.pixel_index,
                                    pixel_count: pixel.pixel_count,
                                },
                            )
                            .collect::<Vec<_>>(),
                        fixture_templates,
                    ),
                    prepared_params,
                    scratch: EffectSampleScratch::new(child_script.bytecode_stats()),
                    _bytecode_stats: child_script.bytecode_stats(),
                },
            })
        })
        .collect()
}

fn generator_targets_for_scope(
    scope: SequenceEffectScope,
    target: GeneratorTarget,
) -> Vec<GeneratorTarget> {
    match scope {
        SequenceEffectScope::WholeTarget => vec![target],
        SequenceEffectScope::PerFixture => {
            let mut targets = Vec::new();
            for pixel in target.pixels {
                match targets.last_mut() {
                    Some(last) if same_generator_target_fixture(last, pixel.fixture_index) => {
                        last.pixels.push(pixel);
                    }
                    _ => targets.push(GeneratorTarget {
                        pixels: vec![pixel],
                    }),
                }
            }
            targets
        }
    }
}

fn same_generator_target_fixture(target: &GeneratorTarget, fixture_index: usize) -> bool {
    target
        .pixels
        .first()
        .is_some_and(|pixel| pixel.fixture_index == fixture_index)
}

#[derive(Debug, Clone)]
struct PreparedSequenceEffect {
    id: u32,
    start_seconds: f64,
    duration_seconds: f64,
    authored: bool,
    render: PreparedEffectRender,
}

impl PreparedSequenceEffect {
    fn end_seconds(&self) -> f64 {
        self.start_seconds + self.duration_seconds
    }
}

#[derive(Debug, Clone)]
enum PreparedEffectRender {
    Ready {
        script: CompiledEffect,
        target_pixels: Vec<PreparedEffectPixel>,
        prepared_params: PreparedEffectParams,
        scratch: EffectSampleScratch,
        _bytecode_stats: BytecodeStats,
    },
    MissingScript(String),
    BadParams(RuntimeError),
}

#[derive(Debug, Clone)]
struct PreparedEffectPixel {
    fixture_index: usize,
    pixel_index: usize,
    fixture_context: FixtureContext,
    pixel_context: PixelContext,
}

impl PreparedEffectRender {
    fn error_status(&self) -> OutputFrameStatus {
        match self {
            Self::Ready { .. } => OutputFrameStatus::Live,
            Self::MissingScript(script_key) => {
                OutputFrameStatus::Error(format!("compiled script `{script_key}` was not found"))
            }
            Self::BadParams(error) => OutputFrameStatus::Error(error.to_string()),
        }
    }

    fn error_message(&self) -> String {
        match self {
            Self::Ready { .. } => "effect render is ready".to_string(),
            Self::MissingScript(script_key) => {
                format!("compiled script `{script_key}` was not found")
            }
            Self::BadParams(error) => error.to_string(),
        }
    }
}

fn prepare_effect_pixels(
    scope: SequenceEffectScope,
    target_pixels: &[dawn_project::document::SequenceEffectPixelDocument],
    fixture_templates: &[OutputFixtureFrame],
) -> Vec<PreparedEffectPixel> {
    let target_pixel_count = target_pixels.len();
    target_pixels
        .iter()
        .enumerate()
        .filter_map(|(target_pixel_index, pixel)| {
            fixture_templates
                .get(pixel.fixture_index)?
                .pixels
                .get(pixel.pixel_index)?;
            Some(PreparedEffectPixel {
                fixture_index: pixel.fixture_index,
                pixel_index: pixel.pixel_index,
                fixture_context: FixtureContext {
                    index: pixel.fixture_index,
                },
                pixel_context: pixel_context_for_effect(
                    scope,
                    target_pixel_index,
                    target_pixel_count,
                    pixel.pixel_index,
                    pixel.pixel_count,
                ),
            })
        })
        .collect()
}

pub fn evaluate_sequence_frame(
    analysis: &ProjectAnalysis,
    document: &SequenceDocument,
    time_seconds: f64,
    generation: u64,
) -> OutputFrame {
    match SequenceFrameEvaluator::new(analysis, document) {
        Ok(mut evaluator) => evaluator.evaluate(time_seconds, generation),
        Err(message) => empty_frame(generation, message),
    }
}

pub fn evaluate_sequence_frame_filtered(
    analysis: &ProjectAnalysis,
    document: &SequenceDocument,
    time_seconds: f64,
    generation: u64,
    effect_filter: Option<&HashSet<u32>>,
) -> OutputFrame {
    match SequenceFrameEvaluator::new_filtered(analysis, document, effect_filter) {
        Ok(mut evaluator) => evaluator.evaluate(time_seconds, generation),
        Err(message) => empty_frame(generation, message),
    }
}

pub fn evaluate_sequence_effect_preview_frame(
    analysis: &ProjectAnalysis,
    document: &SequenceDocument,
    preview_seconds: f64,
    generation: u64,
    effect_filter: &HashSet<u32>,
) -> OutputFrame {
    match SequenceFrameEvaluator::new_filtered(analysis, document, Some(effect_filter)) {
        Ok(mut evaluator) => evaluator.evaluate_effect_preview(preview_seconds, generation),
        Err(message) => empty_frame(generation, message),
    }
}

pub fn pixel_context_for_effect(
    scope: SequenceEffectScope,
    target_pixel_index: usize,
    target_pixel_count: usize,
    fixture_pixel_index: usize,
    fixture_pixel_count: usize,
) -> PixelContext {
    match scope {
        SequenceEffectScope::PerFixture => PixelContext {
            index: fixture_pixel_index,
            count: fixture_pixel_count,
        },
        SequenceEffectScope::WholeTarget => PixelContext {
            index: target_pixel_index,
            count: target_pixel_count,
        },
    }
}

fn add_clamped(target: &mut Color, color: Color) {
    target.red = target.red.saturating_add(color.red);
    target.green = target.green.saturating_add(color.green);
    target.blue = target.blue.saturating_add(color.blue);
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

pub fn runtime_params_from_document(
    params: &[SequenceEffectParamDocument],
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
) -> BTreeMap<String, RuntimeValue> {
    params
        .iter()
        .filter_map(|param| {
            runtime_value_from_param(&param.value, mark_collections, effect_start_seconds)
                .map(|value| (param.name.clone(), value))
        })
        .collect()
}

pub fn prepare_params_from_document(
    script: &CompiledEffect,
    params: &[SequenceEffectParamDocument],
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
) -> Result<PreparedEffectParams, RuntimeError> {
    script.prepare_params_with(|name| {
        params
            .iter()
            .find(|param| param.name == name)
            .and_then(|param| {
                runtime_value_from_param(&param.value, mark_collections, effect_start_seconds)
            })
    })
}

pub fn runtime_value_from_param(
    param: &EffectParam<Resolved>,
    mark_collections: &[SequenceMarkCollectionDocument],
    effect_start_seconds: f64,
) -> Option<RuntimeValue> {
    match param {
        EffectParam::Integer { value } => Some(RuntimeValue::Int(*value as i64)),
        EffectParam::Float { value } => Some(RuntimeValue::Float(*value)),
        EffectParam::Boolean { value } => Some(RuntimeValue::Bool(*value)),
        EffectParam::Enum { value } => Some(RuntimeValue::Enum(value.clone())),
        EffectParam::Flags { value } => Some(RuntimeValue::Flags(value.clone())),
        EffectParam::Color { value } => Some(RuntimeValue::Color(*value)),
        EffectParam::Curve { curve } => Some(RuntimeValue::Curve(curve.clone())),
        EffectParam::Marks { key } => {
            let mut marks = mark_collections
                .iter()
                .find(|collection| collection.key == *key)?
                .marks_seconds
                .iter()
                .map(|mark_seconds| *mark_seconds - effect_start_seconds)
                .collect::<Vec<_>>();
            marks.sort_by(f64::total_cmp);
            Some(RuntimeValue::Marks(marks))
        }
    }
}

pub fn empty_frame(generation: u64, message: impl Into<String>) -> OutputFrame {
    OutputFrame {
        source: OutputSourceMetadata {
            label: "No preview source".to_string(),
            kind: OutputSourceKind::Empty,
            duration_seconds: 0.0,
            fps: 0,
        },
        time_seconds: 0.0,
        generation,
        status: OutputFrameStatus::Idle(message.into()),
        bounds: GeometryRenderBounds {
            min_x: Distance::from_micrometers(-5_000_000),
            min_y: Distance::from_micrometers(-4_000_000),
            max_x: Distance::from_micrometers(5_000_000),
            max_y: Distance::from_micrometers(4_000_000),
        },
        fixtures: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use dawn_project::analysis::{analyze_project, ProjectAnalysis};
    use dawn_project::document::{get_sequence_document, SequenceDocument};
    use dawn_project::fs::WorkspaceFs;
    use dawn_project::model::{Color, Distance, SequenceEffectScope};
    use dawn_project::path::{utf8_path, Utf8PathBuf};
    use dawn_project::render::GeometryRenderBounds;

    use dawn_project::effect_script::{GeneratorTarget, GeneratorTargetPixel};

    use super::{
        build_effect_indices_by_frame, generator_targets_for_scope, pixel_context_for_effect,
        OutputFixtureFrame, OutputFrame, OutputFrameStatus, OutputSourceKind, OutputSourceMetadata,
        PreparedEffectRender, PreparedSequenceEffect, SequenceFrameEvaluator,
    };

    fn club_rig_project_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/club-rig/project.dawn")
    }

    fn club_rig_context() -> (WorkspaceFs, Utf8PathBuf, Utf8PathBuf) {
        let project_path = club_rig_project_path();
        let root = project_path
            .parent()
            .expect("club rig project should have a parent");
        let fs = WorkspaceFs::open(root).expect("club rig root should open");
        let project_path = utf8_path(
            project_path
                .strip_prefix(root)
                .expect("project path should be under root"),
        )
        .expect("project path should be valid UTF-8");
        let sequence_path = utf8_path(Path::new("sequences/opening.sequence.dawn"))
            .expect("sequence path should be valid UTF-8");
        (fs, project_path, sequence_path)
    }

    fn club_rig_analysis_and_sequence() -> (ProjectAnalysis, SequenceDocument) {
        let (fs, project_path, sequence_path) = club_rig_context();
        let analysis = analyze_project(&fs, project_path.clone(), "club_rig");
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let document =
            get_sequence_document(&fs, sequence_path, "opening", project_path, Vec::new())
                .expect("club rig sequence should load");
        (analysis, document)
    }

    fn thirty_output_controller_analysis_and_sequence() -> (ProjectAnalysis, SequenceDocument) {
        let project_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/thirty-output-controller/project.dawn");
        let root = project_path
            .parent()
            .expect("thirty output controller project should have a parent");
        let fs = WorkspaceFs::open(root).expect("thirty output controller root should open");
        let project_path = utf8_path(
            project_path
                .strip_prefix(root)
                .expect("project path should be under root"),
        )
        .expect("project path should be valid UTF-8");
        let sequence_path = utf8_path(Path::new("sequences/empty.sequence.dawn"))
            .expect("sequence path should be valid UTF-8");
        let analysis = analyze_project(&fs, project_path.clone(), "thirty_output_controller");
        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let document = get_sequence_document(&fs, sequence_path, "empty", project_path, Vec::new())
            .expect("thirty output controller sequence should load");
        (analysis, document)
    }

    fn frame_colors(frame: &OutputFrame) -> Vec<Color> {
        frame
            .fixtures
            .iter()
            .flat_map(|fixture| fixture.pixels.iter().map(|pixel| pixel.color))
            .collect()
    }

    fn lit_pixel_count(frame: &OutputFrame) -> usize {
        frame_colors(frame)
            .into_iter()
            .filter(|color| *color != Color::new(0, 0, 0))
            .count()
    }

    fn bad_effect(
        id: u32,
        start_seconds: f64,
        duration_seconds: f64,
        authored: bool,
    ) -> PreparedSequenceEffect {
        PreparedSequenceEffect {
            id,
            start_seconds,
            duration_seconds,
            authored,
            render: PreparedEffectRender::MissingScript("missing.effect.dawn".to_string()),
        }
    }

    fn evaluator_for_effects(effects: Vec<PreparedSequenceEffect>) -> SequenceFrameEvaluator {
        let source = OutputSourceMetadata {
            label: "Test".to_string(),
            kind: OutputSourceKind::Sequence,
            duration_seconds: 3.0,
            fps: 10,
        };
        let effect_indices_by_frame =
            build_effect_indices_by_frame(&effects, source.duration_seconds, source.fps);
        SequenceFrameEvaluator {
            source,
            bounds: GeometryRenderBounds {
                min_x: Distance::from_micrometers(0),
                min_y: Distance::from_micrometers(0),
                max_x: Distance::from_micrometers(0),
                max_y: Distance::from_micrometers(0),
            },
            fixture_templates: Vec::<OutputFixtureFrame>::new(),
            effects,
            effect_indices_by_frame,
            authored_intervals_by_id: [(
                1,
                super::EffectInterval {
                    start_seconds: 0.0,
                    duration_seconds: 3.0,
                },
            )]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn per_fixture_scope_repeats_pixel_context_for_group_members() {
        let contexts = [
            pixel_context_for_effect(SequenceEffectScope::PerFixture, 0, 5, 0, 2),
            pixel_context_for_effect(SequenceEffectScope::PerFixture, 1, 5, 1, 2),
            pixel_context_for_effect(SequenceEffectScope::PerFixture, 2, 5, 0, 3),
            pixel_context_for_effect(SequenceEffectScope::PerFixture, 3, 5, 1, 3),
            pixel_context_for_effect(SequenceEffectScope::PerFixture, 4, 5, 2, 3),
        ];

        assert_eq!(
            contexts.map(|context| (context.index, context.count)),
            [(0, 2), (1, 2), (0, 3), (1, 3), (2, 3)]
        );
    }

    #[test]
    fn whole_target_scope_uses_continuous_group_pixel_context() {
        let contexts = [
            pixel_context_for_effect(SequenceEffectScope::WholeTarget, 0, 5, 0, 2),
            pixel_context_for_effect(SequenceEffectScope::WholeTarget, 1, 5, 1, 2),
            pixel_context_for_effect(SequenceEffectScope::WholeTarget, 2, 5, 0, 3),
            pixel_context_for_effect(SequenceEffectScope::WholeTarget, 3, 5, 1, 3),
            pixel_context_for_effect(SequenceEffectScope::WholeTarget, 4, 5, 2, 3),
        ];

        assert_eq!(
            contexts.map(|context| (context.index, context.count)),
            [(0, 5), (1, 5), (2, 5), (3, 5), (4, 5)]
        );
    }

    #[test]
    fn fixture_target_context_matches_for_both_scopes() {
        let per_fixture = pixel_context_for_effect(SequenceEffectScope::PerFixture, 1, 3, 1, 3);
        let whole_target = pixel_context_for_effect(SequenceEffectScope::WholeTarget, 1, 3, 1, 3);

        assert_eq!(
            (per_fixture.index, per_fixture.count),
            (whole_target.index, whole_target.count)
        );
    }

    #[test]
    fn generator_per_fixture_scope_splits_target_before_generation() {
        let target = GeneratorTarget {
            pixels: vec![
                GeneratorTargetPixel {
                    fixture_index: 0,
                    pixel_index: 0,
                    pixel_count: 2,
                },
                GeneratorTargetPixel {
                    fixture_index: 0,
                    pixel_index: 1,
                    pixel_count: 2,
                },
                GeneratorTargetPixel {
                    fixture_index: 1,
                    pixel_index: 0,
                    pixel_count: 3,
                },
                GeneratorTargetPixel {
                    fixture_index: 1,
                    pixel_index: 1,
                    pixel_count: 3,
                },
                GeneratorTargetPixel {
                    fixture_index: 1,
                    pixel_index: 2,
                    pixel_count: 3,
                },
            ],
        };

        let per_fixture =
            generator_targets_for_scope(SequenceEffectScope::PerFixture, target.clone());
        let whole_target = generator_targets_for_scope(SequenceEffectScope::WholeTarget, target);

        assert_eq!(per_fixture.len(), 2);
        assert_eq!(per_fixture[0].pixels.len(), 2);
        assert_eq!(per_fixture[1].pixels.len(), 3);
        assert_eq!(whole_target.len(), 1);
        assert_eq!(whole_target[0].pixels.len(), 5);
    }

    #[test]
    fn prepared_timeline_index_visits_only_current_frame_bucket() {
        let mut evaluator = evaluator_for_effects(vec![
            bad_effect(1, 0.5, 0.2, true),
            bad_effect(2, 2.0, 0.2, true),
        ]);

        let (frame, timing) = evaluator.evaluate_timed(0.55, 0);

        assert!(matches!(frame.status, OutputFrameStatus::Error(_)));
        assert_eq!(timing.visited_prepared_effects, 1);
    }

    #[test]
    fn prepared_timeline_index_preserves_effect_boundaries() {
        let mut evaluator = evaluator_for_effects(vec![bad_effect(1, 0.5, 0.2, true)]);

        let (at_start, at_start_timing) = evaluator.evaluate_timed(0.5, 0);
        let (at_end, at_end_timing) = evaluator.evaluate_timed(0.7, 0);

        assert!(matches!(at_start.status, OutputFrameStatus::Error(_)));
        assert_eq!(at_start_timing.visited_prepared_effects, 1);
        assert!(matches!(at_end.status, OutputFrameStatus::Live));
        assert_eq!(at_end_timing.visited_prepared_effects, 0);
    }

    #[test]
    fn generated_children_are_indexed_by_their_own_interval() {
        let mut evaluator = evaluator_for_effects(vec![bad_effect(1, 1.2, 0.4, false)]);

        let (frame, timing) = evaluator.evaluate_timed(1.3, 0);

        assert!(matches!(frame.status, OutputFrameStatus::Error(_)));
        assert_eq!(timing.visited_prepared_effects, 1);
    }

    #[test]
    fn bad_prepared_renders_surface_errors_only_during_indexed_interval() {
        let mut evaluator = evaluator_for_effects(vec![bad_effect(1, 0.5, 0.2, true)]);

        let (before, before_timing) = evaluator.evaluate_timed(0.4, 0);
        let (during, during_timing) = evaluator.evaluate_timed(0.55, 0);
        let (after, after_timing) = evaluator.evaluate_timed(0.8, 0);

        assert!(matches!(before.status, OutputFrameStatus::Live));
        assert_eq!(before_timing.visited_prepared_effects, 0);
        assert!(matches!(during.status, OutputFrameStatus::Error(_)));
        assert_eq!(during_timing.visited_prepared_effects, 1);
        assert!(matches!(after.status, OutputFrameStatus::Live));
        assert_eq!(after_timing.visited_prepared_effects, 0);
    }

    #[test]
    fn generator_heavy_sequence_visits_only_indexed_prepared_children() {
        let (analysis, document) = thirty_output_controller_analysis_and_sequence();
        let mut evaluator =
            SequenceFrameEvaluator::new(&analysis, &document).expect("renderer should build");

        let (first_frame, timing) = evaluator.evaluate_timed(41.0, 1);
        let second_frame = evaluator.evaluate(41.0, 2);

        assert_eq!(frame_colors(&first_frame), frame_colors(&second_frame));
        assert!(evaluator.prepared_effect_count() > document.effects.len());
        assert!(timing.visited_prepared_effects < evaluator.prepared_effect_count() as u32);
        assert!(timing.active_prepared_effects >= timing.active_authored_effects);
    }

    #[test]
    fn reusable_sequence_evaluator_updates_frame_output_over_time() {
        let (analysis, document) = club_rig_analysis_and_sequence();
        let mut evaluator =
            SequenceFrameEvaluator::new(&analysis, &document).expect("renderer should build");

        let first = evaluator.evaluate(2.0, 1);
        let second = evaluator.evaluate(6.0, 2);

        assert_ne!(frame_colors(&first), frame_colors(&second));
        assert!(lit_pixel_count(&first) > 0);
        assert!(lit_pixel_count(&second) > 0);
    }

    #[test]
    fn selected_effect_preview_filters_the_reusable_evaluator() {
        let (analysis, document) = club_rig_analysis_and_sequence();
        let mut evaluator =
            SequenceFrameEvaluator::new(&analysis, &document).expect("renderer should build");
        let first_ids = [1].into_iter().collect();
        let second_ids = [23].into_iter().collect();

        let first = evaluator.evaluate_effect_preview_filtered(1.0, 1, Some(&first_ids));
        let second = evaluator.evaluate_effect_preview_filtered(1.0, 2, Some(&second_ids));

        assert_ne!(frame_colors(&first), frame_colors(&second));
        assert!(lit_pixel_count(&first) > 0);
        assert!(lit_pixel_count(&second) > 0);
    }
}
