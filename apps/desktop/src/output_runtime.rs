use std::collections::BTreeMap;

use dawn_project::analysis::ProjectAnalysis;
use dawn_project::document::{SequenceDocument, SequenceEffectParamDocument};
use dawn_project::effect_script::{FixtureContext, PixelContext, RuntimeValue};
use dawn_project::model::{
    Color, ColorModel, EffectParam, Fixture, FixtureId, Geometry, Point3, Resolved, Rotation3,
    Scale3, Transform,
};
use dawn_project::path::{PathStringExt, Utf8PathBuf};
use dawn_project::render::{
    geometry_render_plan, layout_render_plan, transform_geometry_render_plan, GeometryRenderBounds,
    GeometryRenderPoint,
};

use crate::app_model::{AppSnapshot, PreviewRigKind};

pub const EFFECT_PREVIEW_LOOP_MS: u64 = 8_000;

#[derive(Debug, Clone)]
pub struct OutputFrame {
    pub source: OutputSourceMetadata,
    pub time_ms: u64,
    pub generation: u64,
    pub status: OutputFrameStatus,
    pub bounds: GeometryRenderBounds,
    pub fixtures: Vec<OutputFixtureFrame>,
}

#[derive(Debug, Clone)]
pub struct OutputSourceMetadata {
    pub label: String,
    pub kind: OutputSourceKind,
    pub duration_ms: u64,
    pub fps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputSourceKind {
    Sequence,
    SoloClip,
    EffectScript,
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
    pub bulb_radius: f64,
    pub pixels: Vec<OutputPixelFrame>,
}

#[derive(Debug, Clone)]
pub struct OutputPixelFrame {
    pub position: GeometryRenderPoint,
    pub color: Color,
}

#[derive(Debug, Clone)]
pub enum OutputPreviewSource {
    Sequence {
        document: SequenceDocument,
        solo_effect_id: Option<u32>,
    },
    EffectScript {
        path: Utf8PathBuf,
        rig: PreviewRigKind,
    },
}

pub trait OutputSink {
    fn write_frame(&self, frame: OutputFrame);
}

#[derive(Debug, Clone, Default)]
pub struct InProcessOutputRuntime {
    generation: u64,
}

impl InProcessOutputRuntime {
    pub fn evaluate_snapshot(&mut self, snapshot: &AppSnapshot) -> OutputFrame {
        self.generation = self.generation.saturating_add(1);
        match OutputPreviewSource::from_snapshot(snapshot) {
            Some(source) => self.evaluate_source(snapshot, source),
            None => empty_frame(self.generation, "Open a sequence or .effect.dawn file"),
        }
    }

    pub fn evaluate_source(
        &mut self,
        snapshot: &AppSnapshot,
        source: OutputPreviewSource,
    ) -> OutputFrame {
        self.generation = self.generation.saturating_add(1);
        let Some(analysis) = snapshot.analysis.as_ref() else {
            return empty_frame(self.generation, "No project analysis");
        };

        match source {
            OutputPreviewSource::Sequence {
                document,
                solo_effect_id,
            } => evaluate_sequence_frame(
                analysis,
                &document,
                solo_effect_id,
                snapshot.sequence_playhead_ms,
                self.generation,
            ),
            OutputPreviewSource::EffectScript { path, rig } => evaluate_effect_frame(
                analysis,
                path,
                rig,
                snapshot.playback.time_ms,
                self.generation,
            ),
        }
    }
}

impl OutputPreviewSource {
    pub fn from_snapshot(snapshot: &AppSnapshot) -> Option<Self> {
        if let Some(document) = snapshot.active_sequence_document.clone() {
            return Some(Self::Sequence {
                document,
                solo_effect_id: snapshot
                    .solo_selected_clip
                    .then_some(snapshot.selected_sequence_effect)
                    .flatten(),
            });
        }
        active_effect_script_path(snapshot).map(|path| Self::EffectScript {
            path,
            rig: snapshot.preview_rig,
        })
    }
}

fn evaluate_sequence_frame(
    analysis: &ProjectAnalysis,
    document: &SequenceDocument,
    solo_effect_id: Option<u32>,
    time_ms: u64,
    generation: u64,
) -> OutputFrame {
    let Some(project) = analysis.resolved.as_ref() else {
        return empty_frame(
            generation,
            "Project must resolve before preview is available",
        );
    };
    let render_plan = layout_render_plan(&project.display.layout.fixtures);
    let mut fixtures = render_plan
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

    let mut status = OutputFrameStatus::Live;
    for effect in &document.effects {
        if let Some(solo_effect_id) = solo_effect_id {
            if effect.id != solo_effect_id {
                continue;
            }
        }
        let Some(render) = &effect.render else {
            continue;
        };
        let local_ms = if solo_effect_id == Some(effect.id) {
            if effect.duration_ms == 0 {
                0
            } else {
                time_ms.saturating_sub(effect.start_ms) % effect.duration_ms
            }
        } else if time_ms < effect.start_ms
            || time_ms >= effect.start_ms.saturating_add(effect.duration_ms)
        {
            continue;
        } else {
            time_ms.saturating_sub(effect.start_ms)
        };
        let progress = if effect.duration_ms == 0 {
            0.0
        } else {
            (local_ms as f64 / effect.duration_ms as f64).clamp(0.0, 1.0)
        };
        let params = runtime_params_from_document(&render.params);
        for pixel in &render.target_pixels {
            let Some(fixture) = fixtures.get_mut(pixel.fixture_index) else {
                continue;
            };
            let Some(output_pixel) = fixture.pixels.get_mut(pixel.pixel_index) else {
                continue;
            };
            match analysis.sample_effect_script_key(
                &render.script_key,
                progress,
                local_ms as f64 / 1_000.0,
                FixtureContext {
                    index: pixel.fixture_index,
                },
                PixelContext {
                    index: pixel.pixel_index,
                },
                params.clone(),
            ) {
                Ok(color) => add_clamped(&mut output_pixel.color, color),
                Err(error) => status = OutputFrameStatus::Error(error),
            }
        }
    }

    let (kind, label, duration_ms) = if let Some(effect_id) = solo_effect_id {
        let label = document
            .effects
            .iter()
            .find(|effect| effect.id == effect_id)
            .map(|effect| format!("Solo clip {}  {}", effect.id, effect.script))
            .unwrap_or_else(|| "Solo selected clip".to_string());
        let duration_ms = document
            .effects
            .iter()
            .find(|effect| effect.id == effect_id)
            .map(|effect| effect.duration_ms)
            .unwrap_or(document.duration_ms);
        (OutputSourceKind::SoloClip, label, duration_ms)
    } else {
        (
            OutputSourceKind::Sequence,
            format!("Sequence {}", document.object_key),
            document.duration_ms,
        )
    };

    OutputFrame {
        source: OutputSourceMetadata {
            label,
            kind,
            duration_ms,
            fps: document.frame_rate,
        },
        time_ms,
        generation,
        status,
        bounds: render_plan.bounds,
        fixtures,
    }
}

fn evaluate_effect_frame(
    analysis: &ProjectAnalysis,
    path: Utf8PathBuf,
    rig: PreviewRigKind,
    time_ms: u64,
    generation: u64,
) -> OutputFrame {
    let fixtures = synthetic_rig(rig);
    let bounds = synthetic_bounds(rig);
    let mut status = OutputFrameStatus::Live;
    let params = analysis.default_runtime_params_for_script(&path);
    let script_name = analysis
        .compiled_script_for_path(&path)
        .map(|script| script.name.clone())
        .unwrap_or_else(|| "Effect script".to_string());
    let loop_ms = time_ms % EFFECT_PREVIEW_LOOP_MS;
    let progress = (loop_ms as f64 / EFFECT_PREVIEW_LOOP_MS as f64).clamp(0.0, 1.0);

    let output_fixtures = fixtures
        .into_iter()
        .map(|fixture| {
            let plan = transform_geometry_render_plan(
                &geometry_render_plan(&fixture.fixture.geometry, fixture.fixture.bulb_size),
                &fixture.transform,
            );
            let pixels = plan
                .emitters
                .iter()
                .enumerate()
                .map(|(pixel_index, position)| {
                    let color = analysis
                        .sample_effect_script(
                            &path,
                            progress,
                            loop_ms as f64 / 1_000.0,
                            FixtureContext {
                                index: fixture.index,
                            },
                            PixelContext { index: pixel_index },
                            params.clone(),
                        )
                        .unwrap_or_else(|error| {
                            status = OutputFrameStatus::Error(error);
                            Color::new(255, 64, 64)
                        });
                    OutputPixelFrame {
                        position: *position,
                        color,
                    }
                })
                .collect();
            OutputFixtureFrame {
                id: fixture.id,
                name: fixture.name,
                bulb_radius: plan.bulb_radius,
                pixels,
            }
        })
        .collect();

    OutputFrame {
        source: OutputSourceMetadata {
            label: format!("{script_name} on {}", rig.label()),
            kind: OutputSourceKind::EffectScript,
            duration_ms: EFFECT_PREVIEW_LOOP_MS,
            fps: 60,
        },
        time_ms: loop_ms,
        generation,
        status,
        bounds,
        fixtures: output_fixtures,
    }
}

fn add_clamped(target: &mut Color, color: Color) {
    target.red = target.red.saturating_add(color.red);
    target.green = target.green.saturating_add(color.green);
    target.blue = target.blue.saturating_add(color.blue);
}

fn runtime_params_from_document(
    params: &[SequenceEffectParamDocument],
) -> BTreeMap<String, RuntimeValue> {
    params
        .iter()
        .filter_map(|param| {
            runtime_value_from_param(&param.value).map(|value| (param.name.clone(), value))
        })
        .collect()
}

fn runtime_value_from_param(param: &EffectParam<Resolved>) -> Option<RuntimeValue> {
    match param {
        EffectParam::Integer { value } => Some(RuntimeValue::Int(*value as i64)),
        EffectParam::Float { value } => Some(RuntimeValue::Float(*value)),
        EffectParam::Boolean { value } => Some(RuntimeValue::Bool(*value)),
        EffectParam::Enum { value } => Some(RuntimeValue::Enum(value.clone())),
        EffectParam::Flags { value } => Some(RuntimeValue::Flags(value.clone())),
        EffectParam::Color { value } => Some(RuntimeValue::Color(*value)),
        EffectParam::Curve { curve } => Some(RuntimeValue::Curve(curve.clone())),
    }
}

fn active_effect_script_path(state: &AppSnapshot) -> Option<Utf8PathBuf> {
    state
        .active_file
        .as_ref()
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.ends_with(".effect.dawn"))
        })
        .and_then(|active| {
            state.analysis.as_ref()?.scripts.keys().find_map(|path| {
                path.ends_with(&active.to_slash_string())
                    .then(|| Utf8PathBuf::from(path.as_str()))
            })
        })
}

#[derive(Debug, Clone)]
struct SyntheticFixture {
    index: usize,
    id: FixtureId,
    name: String,
    transform: Transform,
    fixture: Fixture,
}

fn synthetic_rig(kind: PreviewRigKind) -> Vec<SyntheticFixture> {
    vec![SyntheticFixture {
        index: 0,
        id: FixtureId(1),
        name: kind.label().to_string(),
        transform: Transform {
            position: Point3::default(),
            rotation: Rotation3::default(),
            scale: Scale3::default(),
        },
        fixture: Fixture {
            name: kind.label().to_string(),
            color_model: ColorModel::Rgb,
            bulb_size: 1.6,
            geometry: synthetic_geometry(kind),
        },
    }]
}

fn synthetic_geometry(kind: PreviewRigKind) -> Geometry {
    match kind {
        PreviewRigKind::Strand => Geometry::Lines {
            points: vec![
                Point3 {
                    x: -4.0,
                    y: 0.0,
                    z: 0.0,
                },
                Point3 {
                    x: 4.0,
                    y: 0.0,
                    z: 0.0,
                },
            ],
            pixels: 48,
        },
        PreviewRigKind::VerticalStrand => Geometry::Lines {
            points: vec![
                Point3 {
                    x: 0.0,
                    y: -3.5,
                    z: 0.0,
                },
                Point3 {
                    x: 0.0,
                    y: 3.5,
                    z: 0.0,
                },
            ],
            pixels: 48,
        },
        PreviewRigKind::Circle => Geometry::Arc {
            center: Point3::default(),
            radius: 2.8,
            start_degrees: 0.0,
            end_degrees: 360.0,
            pixels: 64,
        },
        PreviewRigKind::Grid => {
            let mut points = Vec::new();
            for row in 0..8 {
                for column in 0..8 {
                    points.push(Point3 {
                        x: (column as f64 - 3.5) * 0.75,
                        y: (3.5 - row as f64) * 0.75,
                        z: 0.0,
                    });
                }
            }
            Geometry::Points { points }
        }
    }
}

fn synthetic_bounds(kind: PreviewRigKind) -> GeometryRenderBounds {
    match kind {
        PreviewRigKind::Strand => GeometryRenderBounds {
            min_x: -4.5,
            min_y: -1.0,
            max_x: 4.5,
            max_y: 1.0,
        },
        PreviewRigKind::VerticalStrand => GeometryRenderBounds {
            min_x: -1.0,
            min_y: -4.0,
            max_x: 1.0,
            max_y: 4.0,
        },
        PreviewRigKind::Circle => GeometryRenderBounds {
            min_x: -3.4,
            min_y: -3.4,
            max_x: 3.4,
            max_y: 3.4,
        },
        PreviewRigKind::Grid => GeometryRenderBounds {
            min_x: -3.2,
            min_y: -3.2,
            max_x: 3.2,
            max_y: 3.2,
        },
    }
}

fn empty_frame(generation: u64, message: impl Into<String>) -> OutputFrame {
    OutputFrame {
        source: OutputSourceMetadata {
            label: "No preview source".to_string(),
            kind: OutputSourceKind::Empty,
            duration_ms: 0,
            fps: 0,
        },
        time_ms: 0,
        generation,
        status: OutputFrameStatus::Idle(message.into()),
        bounds: GeometryRenderBounds {
            min_x: -5.0,
            min_y: -4.0,
            max_x: 5.0,
            max_y: 4.0,
        },
        fixtures: Vec::new(),
    }
}
