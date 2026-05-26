use std::collections::BTreeMap;
use std::rc::Rc;

use dawn_project::effect_script::{FixtureContext, PixelContext, RuntimeValue};
use dawn_project::model::{
    Color, ColorModel, EffectParam, Fixture, FixtureId, Geometry, Point3, Resolved, Rotation3,
    Scale3, ScriptSource, SequenceEffect, Transform,
};
use dawn_project::path::{PathStringExt, Utf8PathBuf};
use dawn_project::render::{
    geometry_render_plan, layout_render_bounds, transform_geometry_render_plan,
    GeometryRenderBounds, GeometryRenderGuide,
};
use floem::prelude::*;

use crate::actions::AppAction;
use crate::app_model::{AppSnapshot, PreviewRigKind};
use crate::ui::components::canvas::{
    canvas, CanvasItem, CanvasItemInteraction, CanvasItemKind, CanvasLayer, CanvasScene,
};
use crate::ui::components::{ui_button, ui_label, ui_static_label};
use crate::ui::theme;

pub fn preview_view(state: AppSnapshot, dispatch: crate::ui::UiDispatch) -> impl IntoView {
    let play = Rc::clone(&dispatch);
    let pause = Rc::clone(&dispatch);
    let back = Rc::clone(&dispatch);
    let forward = Rc::clone(&dispatch);
    let open_sequence = Rc::clone(&dispatch);
    let active_file = state.active_file.clone();
    let time = state.playback.time;
    let preview = EffectPreview::from_snapshot(&state);

    v_stack((
        preview_header(preview.clone()),
        h_stack((
            ui_button("Use Active Sequence").action(move || {
                if let Some(path) = active_file.clone() {
                    if path.to_slash_string().ends_with(".sequence.dawn") {
                        open_sequence(AppAction::OpenSequence(path));
                    }
                }
            }),
            ui_button("Play").action(move || play(AppAction::Play)),
            ui_button("Pause").action(move || pause(AppAction::Pause)),
        ))
        .style(|s| s.gap(theme::SPACE_6)),
        h_stack((
            ui_button(format!("-{:.2}", theme::PREVIEW_STEP_SECONDS)).action(move || {
                back(AppAction::Seek(
                    (time - theme::PREVIEW_STEP_SECONDS).max(0.0),
                ))
            }),
            ui_label(move || format!("{time:.2}s / {:.2}s", theme::PREVIEW_DURATION_SECONDS)),
            ui_button(format!("+{:.2}", theme::PREVIEW_STEP_SECONDS)).action(move || {
                forward(AppAction::Seek(
                    (time + theme::PREVIEW_STEP_SECONDS).min(theme::PREVIEW_DURATION_SECONDS),
                ))
            }),
        ))
        .style(|s| s.gap(theme::SPACE_8).items_center()),
        rig_selector(preview.synthetic, state.preview_rig, Rc::clone(&dispatch)),
        preview_canvas(
            preview.clone(),
            state.selected_preview_fixture,
            time,
            Rc::clone(&dispatch),
        )
        .style(|s| {
            s.flex_grow(1.0)
                .min_height(180.0)
                .border(theme::BORDER_WIDTH)
                .border_color(theme::color(theme::BORDER))
        }),
        preview_targets(preview, state.selected_preview_fixture, dispatch),
    ))
    .style(|s| {
        s.height_full()
            .padding(theme::SPACE_10)
            .gap(theme::SPACE_10)
    })
}

fn rig_selector(
    synthetic: bool,
    active: PreviewRigKind,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    if !synthetic {
        return empty().into_any();
    }
    h_stack((
        rig_button(PreviewRigKind::Strand, active, Rc::clone(&dispatch)),
        rig_button(PreviewRigKind::VerticalStrand, active, Rc::clone(&dispatch)),
        rig_button(PreviewRigKind::Circle, active, Rc::clone(&dispatch)),
        rig_button(PreviewRigKind::Grid, active, dispatch),
    ))
    .style(|s| s.gap(theme::SPACE_6).items_center())
    .into_any()
}

fn rig_button(
    rig: PreviewRigKind,
    active: PreviewRigKind,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    ui_button(rig.label())
        .action(move || dispatch(AppAction::SelectPreviewRig(rig)))
        .style(move |s| {
            let background = if rig == active {
                theme::color(theme::SURFACE_CONTROL_ACTIVE)
            } else {
                theme::color(theme::SURFACE_CONTROL)
            };
            s.background(background)
        })
}

fn preview_header(preview: EffectPreview) -> impl IntoView {
    let script_name = preview
        .script_name
        .clone()
        .unwrap_or_else(|| "No effect script".to_string());
    let status = preview
        .status
        .clone()
        .unwrap_or_else(|| "Open an .effect.dawn file or a project sequence".to_string());
    v_stack((
        ui_static_label("Effect Preview").style(|s| s.font_bold()),
        ui_static_label(script_name),
        ui_static_label(status).style(|s| {
            s.color(theme::color(theme::MUTED))
                .font_size(theme::FONT_SMALL)
        }),
    ))
    .style(|s| {
        s.width_full()
            .gap(theme::SPACE_4)
            .padding_bottom(theme::SPACE_8)
            .border_bottom(theme::BORDER_WIDTH)
            .border_color(theme::color(theme::BORDER))
    })
}

fn preview_canvas(
    preview: EffectPreview,
    selected_fixture: Option<FixtureId>,
    time: f64,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let select = Rc::clone(&dispatch);
    canvas(move || preview.scene(selected_fixture, time))
        .on_select(move |fixture_id| {
            select(AppAction::SelectPreviewFixture(Some(fixture_id)));
        })
        .style(|s| s.width_full().height_full())
}

fn preview_targets(
    preview: EffectPreview,
    selected_fixture: Option<FixtureId>,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let all = Rc::clone(&dispatch);
    let fixtures = preview.fixtures.clone();
    scroll(
        v_stack((
            ui_static_label("Targets").style(|s| s.font_bold()),
            target_button(
                "All fixtures".to_string(),
                selected_fixture.is_none(),
                move || {
                    all(AppAction::SelectPreviewFixture(None));
                },
            ),
            v_stack_from_iter(fixtures.into_iter().map(move |fixture| {
                let select = Rc::clone(&dispatch);
                let fixture_id = fixture.id;
                target_button(
                    format!("{}  {}", fixture.id, fixture.name),
                    selected_fixture == Some(fixture_id),
                    move || {
                        select(AppAction::SelectPreviewFixture(Some(fixture_id)));
                    },
                )
            })),
        ))
        .style(|s| s.width_full().gap(theme::SPACE_6)),
    )
    .style(|s| s.width_full().max_height(220.0).min_height(0.0))
}

fn target_button(label: String, active: bool, action: impl Fn() + 'static) -> impl IntoView {
    ui_button(label).action(action).style(move |s| {
        let background = if active {
            theme::color(theme::SURFACE_CONTROL_ACTIVE)
        } else {
            theme::color(theme::SURFACE_CONTROL)
        };
        s.width_full().justify_start().background(background)
    })
}

#[derive(Debug, Clone)]
struct EffectPreview {
    script_path: Option<Utf8PathBuf>,
    script_name: Option<String>,
    status: Option<String>,
    fixtures: Vec<PreviewFixture>,
    synthetic: bool,
    bounds: GeometryRenderBounds,
    params: BTreeMap<String, RuntimeValue>,
    analysis: Option<dawn_project::analysis::ProjectAnalysis>,
}

#[derive(Debug, Clone)]
struct PreviewFixture {
    index: usize,
    id: FixtureId,
    name: String,
    transform: dawn_project::model::Transform,
    fixture: dawn_project::model::Fixture,
}

impl EffectPreview {
    fn from_snapshot(state: &AppSnapshot) -> Self {
        let Some(analysis) = state.analysis.clone() else {
            return Self::empty("No project analysis");
        };
        let Some(project) = analysis.resolved.clone() else {
            return Self::empty("Project must resolve before preview is available");
        };
        let script_path = active_or_first_script_path(state, &project);
        let (script_name, mut params, status) = match script_path.as_ref() {
            Some(path) => {
                let script_name = analysis
                    .compiled_script_for_path(path)
                    .map(|script| script.name.clone());
                let params = analysis.default_runtime_params_for_script(path);
                (
                    script_name,
                    params,
                    Some(format!("Sampling {}", path.to_slash_string())),
                )
            }
            None => (
                None,
                BTreeMap::new(),
                Some("No compiled effect script found".to_string()),
            ),
        };
        let synthetic = active_effect_script_path(state).is_some();
        if let Some(path) = script_path.as_ref() {
            if let Some(effect) = first_sequence_effect_for_script(&project, path) {
                params.extend(effect.params.iter().filter_map(|(name, param)| {
                    runtime_value_from_param(param).map(|value| (name.clone(), value))
                }));
            }
        }
        let (fixtures, bounds, status) = if synthetic {
            (
                synthetic_rig(state.preview_rig),
                synthetic_bounds(state.preview_rig),
                status.map(|status| format!("{status} on {} rig", state.preview_rig.label())),
            )
        } else {
            (
                project
                    .display
                    .layout
                    .fixtures
                    .iter()
                    .enumerate()
                    .map(|(index, fixture)| PreviewFixture {
                        index,
                        id: fixture.id,
                        name: fixture.name.clone(),
                        transform: fixture.transform,
                        fixture: fixture.fixture.clone(),
                    })
                    .collect(),
                layout_render_bounds(&project.display.layout.fixtures),
                status,
            )
        };
        Self {
            script_path,
            script_name,
            status,
            fixtures,
            synthetic,
            bounds,
            params,
            analysis: Some(analysis),
        }
    }

    fn empty(message: &str) -> Self {
        Self {
            script_path: None,
            script_name: None,
            status: Some(message.to_string()),
            fixtures: Vec::new(),
            synthetic: false,
            bounds: default_bounds(),
            params: BTreeMap::new(),
            analysis: None,
        }
    }

    fn scene(&self, selected_fixture: Option<FixtureId>, time: f64) -> CanvasScene {
        let mut items = Vec::new();
        let mut layers = Vec::new();
        for fixture in &self.fixtures {
            layers.push(CanvasLayer {
                id: fixture.id.to_string(),
                visible: true,
            });
            let plan = transform_geometry_render_plan(
                &geometry_render_plan(&fixture.fixture.geometry, fixture.fixture.bulb_size),
                &fixture.transform,
            );
            let active = selected_fixture.is_none() || selected_fixture == Some(fixture.id);
            let interaction = CanvasItemInteraction::Target {
                fixture_id: fixture.id,
                selectable: true,
                draggable: false,
            };
            let guide_color = if active {
                floem::peniko::Color::rgba8(255, 255, 255, 110)
            } else {
                floem::peniko::Color::rgba8(255, 255, 255, 32)
            };
            for (index, guide) in plan.guides.into_iter().enumerate() {
                let kind = match guide {
                    GeometryRenderGuide::Line { from, to } => CanvasItemKind::Line { from, to },
                    GeometryRenderGuide::Arc {
                        start,
                        end,
                        radius_x,
                        radius_y,
                        rotation,
                        large_arc,
                        sweep_positive,
                    } => CanvasItemKind::Arc {
                        start,
                        end,
                        radius_x,
                        radius_y,
                        rotation,
                        large_arc,
                        sweep_positive,
                    },
                };
                items.push(CanvasItem {
                    id: format!("{}:guide:{index}", fixture.id),
                    kind,
                    label: Some(fixture.name.clone()),
                    color: guide_color,
                    interaction: interaction.clone(),
                });
            }
            for (pixel_index, emitter) in plan.emitters.into_iter().enumerate() {
                let sampled = if active {
                    self.sample(time, fixture.index, pixel_index)
                } else {
                    Color::new(42, 42, 42)
                };
                items.push(CanvasItem {
                    id: format!("{}:emitter:{pixel_index}", fixture.id),
                    kind: CanvasItemKind::Point {
                        position: emitter,
                        radius: plan.bulb_radius,
                    },
                    label: Some(fixture.name.clone()),
                    color: color_to_peniko(sampled),
                    interaction: interaction.clone(),
                });
            }
        }
        CanvasScene {
            bounds: self.bounds,
            layers,
            items,
        }
    }

    fn sample(&self, time: f64, fixture_index: usize, pixel_index: usize) -> Color {
        let Some(analysis) = &self.analysis else {
            return Color::new(255, 64, 64);
        };
        let Some(script_path) = &self.script_path else {
            return Color::new(255, 64, 64);
        };
        analysis
            .sample_effect_script(
                script_path,
                (time / theme::PREVIEW_DURATION_SECONDS).clamp(0.0, 1.0),
                time,
                FixtureContext {
                    index: fixture_index,
                },
                PixelContext { index: pixel_index },
                self.params.clone(),
            )
            .unwrap_or_else(|_| Color::new(255, 64, 64))
    }
}

fn active_or_first_script_path(
    state: &AppSnapshot,
    project: &dawn_project::model::ResolvedProject,
) -> Option<Utf8PathBuf> {
    active_effect_script_path(state).or_else(|| {
        project.sequences.iter().find_map(|sequence| {
            sequence
                .effects
                .iter()
                .find_map(|effect| match &effect.script {
                    ScriptSource::External(path) => Some(path.clone()),
                    ScriptSource::Inline(_) => None,
                })
        })
    })
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

fn first_sequence_effect_for_script<'a>(
    project: &'a dawn_project::model::ResolvedProject,
    script_path: &Utf8PathBuf,
) -> Option<&'a SequenceEffect<Resolved>> {
    project.sequences.iter().find_map(|sequence| {
        sequence.effects.iter().find(
            |effect| matches!(&effect.script, ScriptSource::External(path) if path == script_path),
        )
    })
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

fn color_to_peniko(color: Color) -> floem::peniko::Color {
    floem::peniko::Color::rgb8(color.red, color.green, color.blue)
}

fn synthetic_rig(kind: PreviewRigKind) -> Vec<PreviewFixture> {
    vec![PreviewFixture {
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

fn default_bounds() -> GeometryRenderBounds {
    GeometryRenderBounds {
        min_x: -5.0,
        min_y: -4.0,
        max_x: 5.0,
        max_y: 4.0,
    }
}
