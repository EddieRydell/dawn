use std::rc::Rc;

use dawn_project::document::{FixtureDefinitionDocument, FixtureDocument};
use dawn_project::model::{FixtureId, Geometry};
use dawn_project::render::{GeometryRenderGuide, GeometryRenderPoint};
use floem::prelude::*;

use crate::actions::AppAction;
use crate::app_model::AppSnapshot;
use crate::ui::components::canvas::{
    canvas_with_state, CanvasItem, CanvasItemInteraction, CanvasItemKind, CanvasLayer, CanvasScene,
    CanvasState,
};
use crate::ui::components::{ui_button, ui_static_label};
use crate::ui::editor::gui::EditorGuiUiState;
use crate::ui::theme;

pub fn fixture_viewer(
    state: AppSnapshot,
    gui_state: EditorGuiUiState,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let Some(document) = state.active_fixture_document.clone() else {
        return empty().into_any();
    };
    let Some(active_fixture) = selected_fixture(&document).cloned() else {
        return empty_fixture_document().into_any();
    };

    let canvas_state = gui_state.fixture_canvas(&document.path, &active_fixture.object_key);
    let scene_fixture = active_fixture.clone();
    let fixtures = document.fixtures.clone();
    let selected_object_key = active_fixture.object_key.clone();

    v_stack((
        fixture_header(&active_fixture),
        fixture_body(
            move || fixture_canvas_scene(&scene_fixture),
            canvas_state,
            active_fixture.object_key.clone(),
            fixtures,
            selected_object_key,
            dispatch,
        ),
    ))
    .style(|s| {
        s.height_full()
            .padding(theme::SPACE_12)
            .gap(theme::SPACE_8)
            .background(theme::color(theme::SURFACE))
    })
    .into_any()
}

fn fixture_header(fixture: &FixtureDefinitionDocument) -> impl IntoView {
    h_stack((
        ui_static_label("Fixture").style(|s| s.font_bold()),
        ui_static_label(format!("{}  {}", fixture.object_key, fixture.name)),
        ui_static_label(format!(
            "{:?}  bulb {:.2}  {}",
            fixture.color_model, fixture.bulb_size, fixture.geometry_summary
        )),
        empty().style(|s| s.flex_grow(1.0).min_width(0.0)),
    ))
    .style(|s| {
        s.width_full()
            .items_center()
            .gap(theme::SPACE_12)
            .padding_bottom(theme::SPACE_4)
            .border_bottom(theme::BORDER_WIDTH)
            .border_color(theme::color(theme::BORDER))
    })
}

fn fixture_body(
    scene: impl Fn() -> CanvasScene + 'static,
    canvas_state: CanvasState,
    object_key: String,
    fixtures: Vec<FixtureDefinitionDocument>,
    selected_object_key: String,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let drag_dispatch = Rc::clone(&dispatch);
    h_stack((
        canvas_with_state(canvas_state, scene)
            .on_drag_end(move |ids, dx, dy| {
                drag_dispatch(AppAction::NudgeFixtureGeometryHandles {
                    object_key: object_key.clone(),
                    handles: ids.into_iter().map(handle_index).collect(),
                    dx,
                    dy,
                });
            })
            .style(|s| {
                s.flex_grow(1.0)
                    .height_full()
                    .min_width(0.0)
                    .min_height(0.0)
                    .border(theme::BORDER_WIDTH)
                    .border_color(theme::color(theme::BORDER))
                    .border_radius(theme::CONTROL_RADIUS)
            }),
        fixture_right_rail(fixtures, selected_object_key, dispatch),
    ))
    .style(|s| {
        s.flex_grow(1.0)
            .height_full()
            .min_width(0.0)
            .min_height(0.0)
            .gap(theme::SPACE_12)
    })
}

fn fixture_right_rail(
    fixtures: Vec<FixtureDefinitionDocument>,
    selected_object_key: String,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    scroll(
        v_stack((
            ui_static_label("Definitions").style(|s| s.font_bold()),
            v_stack_from_iter(fixtures.into_iter().map(move |fixture| {
                fixture_row(fixture, selected_object_key.clone(), Rc::clone(&dispatch))
            })),
        ))
        .style(|s| s.width_full().gap(theme::SPACE_6)),
    )
    .style(|s| {
        s.width(theme::DEFAULT_RIGHT_PANE_WIDTH)
            .height_full()
            .min_height(0.0)
            .padding_left(theme::SPACE_12)
            .border_left(theme::BORDER_WIDTH)
            .border_color(theme::color(theme::BORDER))
    })
}

fn fixture_row(
    fixture: FixtureDefinitionDocument,
    selected_object_key: String,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let select = Rc::clone(&dispatch);
    let smaller = Rc::clone(&dispatch);
    let larger = Rc::clone(&dispatch);
    let duplicate = Rc::clone(&dispatch);
    let delete = Rc::clone(&dispatch);
    let key = fixture.object_key.clone();
    let key_select = key.clone();
    let key_smaller = key.clone();
    let key_larger = key.clone();
    let key_duplicate = key.clone();
    let key_delete = key;

    v_stack((
        h_stack((
            ui_static_label(format!("{}  {}", fixture.object_key, fixture.name))
                .style(|s| s.font_bold().flex_grow(1.0).min_width(0.0)),
            ui_button(if fixture.object_key == selected_object_key {
                "Selected"
            } else {
                "Select"
            })
            .action(move || {
                select(AppAction::SelectFixtureDefinition {
                    object_key: key_select.clone(),
                })
            }),
        ))
        .style(|s| s.width_full().items_center().gap(theme::SPACE_6)),
        ui_static_label(format!(
            "{:?}  bulb {:.2}",
            fixture.color_model, fixture.bulb_size
        )),
        ui_static_label(fixture.geometry_summary),
        ui_static_label(format!(
            "Pixels: {}  Guides: {}",
            fixture.render_plan.emitters.len(),
            fixture.render_plan.guides.len()
        )),
        h_stack((
            ui_button("Bulb -").action(move || {
                smaller(AppAction::AdjustFixtureBulb {
                    object_key: key_smaller.clone(),
                    delta: -theme::FIXTURE_BULB_STEP,
                })
            }),
            ui_button("Bulb +").action(move || {
                larger(AppAction::AdjustFixtureBulb {
                    object_key: key_larger.clone(),
                    delta: theme::FIXTURE_BULB_STEP,
                })
            }),
        ))
        .style(|s| s.gap(theme::SPACE_4).items_center()),
        h_stack((
            ui_button("Duplicate").action(move || {
                duplicate(AppAction::DuplicateFixtureDefinition {
                    object_key: key_duplicate.clone(),
                })
            }),
            ui_button("Delete").action(move || {
                delete(AppAction::DeleteFixtureDefinition {
                    object_key: key_delete.clone(),
                })
            }),
        ))
        .style(|s| s.gap(theme::SPACE_4).items_center()),
    ))
    .style(|s| {
        s.width_full()
            .padding(theme::SPACE_8)
            .gap(theme::SPACE_3)
            .border_bottom(theme::BORDER_WIDTH)
            .border_color(theme::color(theme::BORDER))
    })
}

fn selected_fixture(document: &FixtureDocument) -> Option<&FixtureDefinitionDocument> {
    document
        .selected_object_key
        .as_ref()
        .and_then(|selected| {
            document
                .fixtures
                .iter()
                .find(|fixture| fixture.object_key == *selected)
        })
        .or_else(|| document.fixtures.first())
}

fn fixture_canvas_scene(fixture: &FixtureDefinitionDocument) -> CanvasScene {
    let mut items = Vec::new();
    let color = floem::peniko::Color::rgb8(118, 185, 255);
    for (index, guide) in fixture.render_plan.guides.iter().cloned().enumerate() {
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
            id: format!("{}:guide:{index}", fixture.object_key),
            kind,
            label: Some(fixture.name.clone()),
            color,
            interaction: CanvasItemInteraction::None,
        });
    }
    for (index, emitter) in fixture.render_plan.emitters.iter().copied().enumerate() {
        items.push(CanvasItem {
            id: format!("{}:emitter:{index}", fixture.object_key),
            kind: CanvasItemKind::Point {
                position: emitter,
                radius: fixture.render_plan.bulb_radius,
            },
            label: Some(fixture.name.clone()),
            color,
            interaction: CanvasItemInteraction::None,
        });
    }
    for (index, point) in fixture_geometry_handles(&fixture.geometry)
        .into_iter()
        .enumerate()
    {
        items.push(CanvasItem {
            id: format!("{}:handle:{index}", fixture.object_key),
            kind: CanvasItemKind::Point {
                position: point,
                radius: fixture.render_plan.bulb_radius * 1.6,
            },
            label: Some(fixture.name.clone()),
            color: floem::peniko::Color::rgb8(255, 202, 97),
            interaction: CanvasItemInteraction::Target {
                fixture_id: handle_id(index),
                selectable: true,
                draggable: true,
            },
        });
    }

    CanvasScene {
        bounds: fixture.render_plan.bounds,
        layers: vec![CanvasLayer {
            id: fixture.object_key.clone(),
            visible: true,
        }],
        items,
    }
}

fn fixture_geometry_handles(geometry: &Geometry) -> Vec<GeometryRenderPoint> {
    match geometry {
        Geometry::Points { points } | Geometry::Lines { points, .. } => points
            .iter()
            .map(|point| GeometryRenderPoint {
                x: point.x,
                y: point.y,
                z: point.z,
            })
            .collect(),
        Geometry::Arc { center, .. } => vec![GeometryRenderPoint {
            x: center.x,
            y: center.y,
            z: center.z,
        }],
    }
}

fn handle_id(index: usize) -> FixtureId {
    FixtureId(index.saturating_add(1) as u32)
}

fn handle_index(id: FixtureId) -> usize {
    id.0.saturating_sub(1) as usize
}

fn empty_fixture_document() -> impl IntoView {
    v_stack((
        ui_static_label("Fixture").style(|s| s.font_bold()),
        ui_static_label("No fixture definitions"),
    ))
    .style(|s| {
        s.height_full()
            .padding(theme::SPACE_12)
            .gap(theme::SPACE_8)
            .background(theme::color(theme::SURFACE))
    })
}
