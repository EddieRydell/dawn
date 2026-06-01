use std::collections::{BTreeMap, HashSet};
use std::time::Instant;

use dawn_project::analysis::ProjectAnalysis;
use dawn_project::document::{
    SequenceDocument, SequenceEffectParamDocument, SequenceMarkCollectionDocument,
};
use dawn_project::effect_script::{
    run_generator, BytecodeStats, CompiledEffect, EffectSampleScratch, EffectScriptKind,
    FixtureContext, GeneratorTarget, GeneratorTargetPixel, PixelContext, PreparedEffectParams,
    RuntimeError, RuntimeValue,
};
use dawn_project::model::{
    Color, Distance, DistanceSpan, EffectParam, FixtureId, Resolved, SequenceEffectScope,
};
use dawn_project::path::{resolve_import_path, PathStringExt, Utf8PathBuf};
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
    pub sampled_pixels: u32,
}

#[derive(Debug, Clone)]
pub struct SequenceFrameEvaluator {
    source: OutputSourceMetadata,
    bounds: GeometryRenderBounds,
    fixture_templates: Vec<OutputFixtureFrame>,
    effects: Vec<PreparedSequenceEffect>,
}

impl SequenceFrameEvaluator {
    pub fn new(analysis: &ProjectAnalysis, document: &SequenceDocument) -> Result<Self, String> {
        Self::new_filtered(analysis, document, None)
    }

    pub fn new_filtered(
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        effect_filter: Option<&HashSet<u32>>,
    ) -> Result<Self, String> {
        let Some(project) = analysis.resolved.as_ref() else {
            return Err("Project must resolve before preview is available".to_string());
        };
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

        let mut effects = Vec::new();
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
                        Ok(children) => effects.extend(children),
                        Err(error) => effects.push(PreparedSequenceEffect {
                            id: effect.id,
                            start_seconds: effect.start_seconds,
                            duration_seconds: effect.duration_seconds,
                            render: PreparedEffectRender::BadParams(error),
                        }),
                    }
                }
                Some(script) => effects.push(PreparedSequenceEffect {
                    id: effect.id,
                    start_seconds: effect.start_seconds,
                    duration_seconds: effect.duration_seconds,
                    render: prepare_sample_render(
                        script,
                        &render.params,
                        &document.mark_collections,
                        effect.start_seconds,
                        effect.scope,
                        &render.target_pixels,
                        &fixture_templates,
                    ),
                }),
                None => effects.push(PreparedSequenceEffect {
                    id: effect.id,
                    start_seconds: effect.start_seconds,
                    duration_seconds: effect.duration_seconds,
                    render: PreparedEffectRender::MissingScript(render.script_key.clone()),
                }),
            }
        }

        Ok(Self {
            source: OutputSourceMetadata {
                label: format!("Sequence {}", document.object_key),
                kind: OutputSourceKind::Sequence,
                duration_seconds: document.duration_seconds,
                fps: document.frame_rate,
            },
            bounds: render_plan.bounds,
            fixture_templates,
            effects,
        })
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
        let mut active_effects = 0u32;
        let mut sampled_pixels = 0u32;

        let effect_loop_started = Instant::now();
        for effect in &mut self.effects {
            let local_seconds = if time_seconds < effect.start_seconds
                || time_seconds >= effect.start_seconds + effect.duration_seconds
            {
                continue;
            } else {
                time_seconds - effect.start_seconds
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
                status = effect.render.error_status();
                continue;
            };

            active_effects = active_effects.saturating_add(1);
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
                    Err(error) => status = OutputFrameStatus::Error(error.to_string()),
                }
                sampled_pixels = sampled_pixels.saturating_add(1);
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
                active_effects,
                sampled_pixels,
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
        let mut active_effects = 0u32;
        let mut sampled_pixels = 0u32;

        let effect_loop_started = Instant::now();
        for effect in &mut self.effects {
            if effect_filter
                .map(|ids| !ids.contains(&effect.id))
                .unwrap_or(false)
            {
                continue;
            }
            if effect.duration_seconds <= 0.0 {
                continue;
            }
            let local_seconds = preview_seconds.rem_euclid(effect.duration_seconds);
            let progress = (local_seconds / effect.duration_seconds).clamp(0.0, 1.0);

            let PreparedEffectRender::Ready {
                script,
                target_pixels,
                prepared_params,
                scratch,
                ..
            } = &mut effect.render
            else {
                status = effect.render.error_status();
                continue;
            };

            active_effects = active_effects.saturating_add(1);
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
                    Err(error) => status = OutputFrameStatus::Error(error.to_string()),
                }
                sampled_pixels = sampled_pixels.saturating_add(1);
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
                active_effects,
                sampled_pixels,
            },
        )
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
    let parent_path = Utf8PathBuf::from(render.script_key.clone());
    children
        .into_iter()
        .map(|child| {
            let import = generator
                .imports
                .iter()
                .find(|import| import.alias == child.alias)
                .ok_or_else(|| RuntimeError {
                    message: format!("generator import alias `{}` was not found", child.alias),
                })?;
            let child_path =
                resolve_import_path(&parent_path, &Utf8PathBuf::from(import.path.clone()))
                    .to_slash_string();
            let child_script = analysis
                .compiled_script_for_key(&child_path)
                .ok_or_else(|| RuntimeError {
                    message: format!("compiled child script `{child_path}` was not found"),
                })?;
            if child_script.kind != EffectScriptKind::Sample || child_script.name != child.effect {
                return Err(RuntimeError {
                    message: format!(
                        "emitted child `{}.{}` is not a sample effect",
                        child.alias, child.effect
                    ),
                });
            }
            let prepared_params = child_script.prepare_params(&child.params)?;
            Ok(PreparedSequenceEffect {
                id: parent_id,
                start_seconds: parent_start_seconds + child.start_seconds,
                duration_seconds: child.duration_seconds,
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
    render: PreparedEffectRender,
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
    use dawn_project::model::{Color, SequenceEffectScope};
    use dawn_project::path::{utf8_path, Utf8PathBuf};

    use dawn_project::effect_script::{GeneratorTarget, GeneratorTargetPixel};

    use super::{
        generator_targets_for_scope, pixel_context_for_effect, OutputFrame, SequenceFrameEvaluator,
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
