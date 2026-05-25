use std::rc::Rc;

use dawn_project::document::{LayoutDocument, LayoutFixturePlacement, LayoutGroupDocument};
use dawn_project::model::DistanceUnit;
use dawn_project::render::{
    transform_geometry_render_plan, GeometryRenderBounds, GeometryRenderGuide,
};
use floem::event::{Event, EventListener};
use floem::peniko::Brush;
use floem::prelude::*;
use floem::reactive::{SignalGet, SignalUpdate};
use floem::style::{CursorStyle, Foreground};

use crate::actions::AppAction;
use crate::app_model::AppSnapshot;
use crate::ui::components::canvas::{
    canvas_with_state, CanvasItem, CanvasItemInteraction, CanvasItemKind, CanvasLayer, CanvasScene,
    CanvasState, CanvasTool,
};
use crate::ui::components::{ui_button, ui_static_label};
use crate::ui::editor::gui::EditorGuiUiState;
use crate::ui::theme;

pub fn layout_viewer(
    state: AppSnapshot,
    gui_state: EditorGuiUiState,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let Some(document) = state.active_layout_document.clone() else {
        return empty().into_any();
    };
    let canvas_state = gui_state.layout_canvas(&document.path, &document.object_key);
    let fixtures = document.fixtures.clone();
    let groups = document.groups.clone();
    let bounds = document.render_bounds;
    let name = document.name.clone();
    let units = document.units;
    let scene_document = document.clone();

    v_stack((
        layout_header(name, units, bounds, canvas_state),
        layout_body(
            move || layout_canvas_scene(&scene_document, canvas_state.selected_target.get()),
            canvas_state,
            fixtures,
            groups,
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

fn layout_header(
    name: String,
    units: DistanceUnit,
    bounds: GeometryRenderBounds,
    canvas_state: CanvasState,
) -> impl IntoView {
    h_stack((
        ui_static_label("Layout").style(|s| s.font_bold()),
        ui_static_label(format!("{name}  {units:?}")),
        ui_static_label(format!(
            "Bounds {:.1},{:.1} to {:.1},{:.1}",
            bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y
        )),
        empty().style(|s| s.flex_grow(1.0).min_width(0.0)),
        canvas_tool_control(canvas_state),
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

fn layout_body(
    scene: impl Fn() -> CanvasScene + 'static,
    canvas_state: CanvasState,
    fixtures: Vec<LayoutFixturePlacement>,
    groups: Vec<LayoutGroupDocument>,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let drag_dispatch = Rc::clone(&dispatch);
    h_stack((
        canvas_with_state(canvas_state, scene)
            .on_drag_end(move |id, dx, dy| {
                drag_dispatch(AppAction::NudgeLayoutFixture { id, dx, dy });
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
        layout_right_rail(fixtures, groups, dispatch),
    ))
    .style(|s| {
        s.flex_grow(1.0)
            .height_full()
            .min_width(0.0)
            .min_height(0.0)
            .gap(theme::SPACE_12)
    })
}

fn canvas_tool_control(canvas_state: CanvasState) -> impl IntoView {
    dyn_container(
        move || canvas_state.tool.get(),
        move |tool| {
            h_stack((
                canvas_tool_button("Pan/Zoom", tool == CanvasTool::PanZoom, move || {
                    canvas_state.tool.set(CanvasTool::PanZoom)
                }),
                canvas_tool_button("Edit", tool == CanvasTool::Edit, move || {
                    canvas_state.tool.set(CanvasTool::Edit)
                }),
            ))
            .style(|s| {
                s.items_center()
                    .padding(theme::SPACE_2)
                    .gap(theme::SPACE_2)
                    .border(theme::BORDER_WIDTH)
                    .border_color(theme::color(theme::BORDER))
                    .border_radius(theme::CONTROL_RADIUS)
                    .background(theme::color(theme::PANEL_DARK))
            })
            .into_any()
        },
    )
}

fn canvas_tool_button(
    label: &'static str,
    active: bool,
    action: impl Fn() + 'static,
) -> impl IntoView {
    container(ui_static_label(label).style(move |s| {
        let text_color = if active {
            theme::color(theme::TEXT_INVERTED)
        } else {
            theme::color(theme::MUTED)
        };
        s.font_size(theme::FONT_SMALL)
            .color(text_color)
            .set(Foreground, Brush::Solid(text_color))
    }))
    .on_event_stop(EventListener::PointerDown, move |event| {
        if let Event::PointerDown(event) = event {
            if event.button.is_primary() {
                action();
            }
        }
    })
    .style(move |s| {
        let bg = if active {
            theme::color(theme::SURFACE_CONTROL_ACTIVE)
        } else {
            theme::color(theme::PANEL_DARK)
        };
        s.height(24.0)
            .items_center()
            .padding_horiz(theme::SPACE_8)
            .border_radius(theme::CONTROL_RADIUS)
            .background(bg)
            .cursor(CursorStyle::Pointer)
            .hover(move |s| {
                if active {
                    s
                } else {
                    s.background(theme::color(theme::SURFACE_CONTROL_HOVER))
                }
            })
    })
}

fn layout_right_rail(
    fixtures: Vec<LayoutFixturePlacement>,
    groups: Vec<LayoutGroupDocument>,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    scroll(
        v_stack((
            fixture_controls_section(fixtures, dispatch),
            groups_section(groups),
        ))
        .style(|s| s.width_full().gap(theme::SPACE_12)),
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

fn fixture_controls_section(
    fixtures: Vec<LayoutFixturePlacement>,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    v_stack((
        ui_static_label("Fixtures").style(|s| s.font_bold()),
        v_stack_from_iter(
            fixtures
                .into_iter()
                .map(move |fixture| fixture_row(fixture, Rc::clone(&dispatch))),
        ),
    ))
    .style(|s| s.width_full().gap(theme::SPACE_6))
}

fn fixture_row(fixture: LayoutFixturePlacement, dispatch: crate::ui::UiDispatch) -> impl IntoView {
    let nudge_left = Rc::clone(&dispatch);
    let nudge_right = Rc::clone(&dispatch);
    let nudge_up = Rc::clone(&dispatch);
    let nudge_down = Rc::clone(&dispatch);
    let duplicate = Rc::clone(&dispatch);
    let delete = Rc::clone(&dispatch);
    let id = fixture.id.clone();
    let id_left = id.clone();
    let id_right = id.clone();
    let id_up = id.clone();
    let id_down = id.clone();
    let id_duplicate = id.clone();
    let id_delete = id.clone();

    v_stack((
        ui_static_label(fixture.id.clone()).style(|s| s.font_bold()),
        ui_static_label(format!(
            "{}  {}",
            fixture.resolved_fixture.name, fixture.resolved_fixture.geometry_summary
        )),
        ui_static_label(format!(
            "x {:.2}  y {:.2}  z {:.2}",
            fixture.transform.position.x,
            fixture.transform.position.y,
            fixture.transform.position.z
        )),
        v_stack((
            h_stack((
                ui_button("Left").action(move || {
                    nudge_left(AppAction::NudgeLayoutFixture {
                        id: id_left.clone(),
                        dx: -theme::LAYOUT_NUDGE_STEP,
                        dy: 0.0,
                    })
                }),
                ui_button("Right").action(move || {
                    nudge_right(AppAction::NudgeLayoutFixture {
                        id: id_right.clone(),
                        dx: theme::LAYOUT_NUDGE_STEP,
                        dy: 0.0,
                    })
                }),
                ui_button("Up").action(move || {
                    nudge_up(AppAction::NudgeLayoutFixture {
                        id: id_up.clone(),
                        dx: 0.0,
                        dy: theme::LAYOUT_NUDGE_STEP,
                    })
                }),
                ui_button("Down").action(move || {
                    nudge_down(AppAction::NudgeLayoutFixture {
                        id: id_down.clone(),
                        dx: 0.0,
                        dy: -theme::LAYOUT_NUDGE_STEP,
                    })
                }),
            ))
            .style(|s| s.gap(theme::SPACE_4).items_center()),
            h_stack((
                ui_button("Duplicate").action(move || {
                    duplicate(AppAction::DuplicateLayoutFixture {
                        id: id_duplicate.clone(),
                    })
                }),
                ui_button("Delete").action(move || {
                    delete(AppAction::DeleteLayoutFixture {
                        id: id_delete.clone(),
                    })
                }),
            ))
            .style(|s| s.gap(theme::SPACE_4).items_center()),
        ))
        .style(|s| s.gap(theme::SPACE_4)),
    ))
    .style(|s| {
        s.width_full()
            .padding(theme::SPACE_8)
            .gap(theme::SPACE_3)
            .border_bottom(theme::BORDER_WIDTH)
            .border_color(theme::color(theme::BORDER))
    })
}

fn groups_section(groups: Vec<LayoutGroupDocument>) -> impl IntoView {
    v_stack((
        ui_static_label("Groups").style(|s| s.font_bold()),
        v_stack_from_iter(
            groups.into_iter().map(|group| {
                ui_static_label(format!("{}  {}", group.name, group.members.join(", ")))
            }),
        ),
    ))
    .style(|s| s.width_full().gap(theme::SPACE_6))
}

fn layout_canvas_scene(document: &LayoutDocument, selected_target: Option<String>) -> CanvasScene {
    let mut items = Vec::new();
    let mut layers = Vec::new();
    for fixture in &document.fixtures {
        layers.push(CanvasLayer {
            id: fixture.id.clone(),
            visible: true,
        });
        let color = fixture_color(&fixture.id);
        let selected = selected_target.as_deref() == Some(fixture.id.as_str());
        let interaction = CanvasItemInteraction::Target {
            id: fixture.id.clone(),
            selectable: true,
            draggable: true,
        };
        let plan = transform_geometry_render_plan(
            &fixture.resolved_fixture.render_plan,
            &fixture.transform,
        );
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
                label: Some(fixture.id.clone()),
                selected,
                color,
                interaction: interaction.clone(),
            });
        }
        for (index, emitter) in plan.emitters.into_iter().enumerate() {
            items.push(CanvasItem {
                id: format!("{}:emitter:{index}", fixture.id),
                kind: CanvasItemKind::Point {
                    position: emitter,
                    radius: plan.bulb_radius,
                },
                label: Some(fixture.id.clone()),
                selected,
                color,
                interaction: interaction.clone(),
            });
        }
    }

    CanvasScene {
        bounds: document.render_bounds,
        layers,
        items,
    }
}

fn fixture_color(id: &str) -> floem::peniko::Color {
    const PALETTE: [(u8, u8, u8); 8] = [
        (118, 185, 255),
        (255, 202, 97),
        (142, 220, 154),
        (245, 132, 121),
        (207, 166, 255),
        (104, 213, 207),
        (236, 154, 207),
        (189, 214, 111),
    ];
    let hash = id.bytes().fold(0usize, |accumulator, byte| {
        accumulator.wrapping_mul(31).wrapping_add(byte as usize)
    });
    let (red, green, blue) = PALETTE[hash % PALETTE.len()];
    floem::peniko::Color::rgb8(red, green, blue)
}
