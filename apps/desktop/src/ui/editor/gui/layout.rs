use std::rc::Rc;

use dawn_project::document::{LayoutDocument, LayoutFixturePlacement, LayoutGroupDocument};
use dawn_project::model::{DistanceUnit, FixtureId};
use dawn_project::render::{
    transform_geometry_render_plan, GeometryRenderBounds, GeometryRenderGuide,
};
use floem::file::{FileDialogOptions, FileSpec};
use floem::file_action::open_file;
use floem::prelude::*;

use crate::actions::AppAction;
use crate::ui::components::canvas::{
    canvas_with_state, CanvasItem, CanvasItemInteraction, CanvasItemKind, CanvasLayer, CanvasScene,
    CanvasState,
};
use crate::ui::components::dropdown_menu::{DropdownMenuController, DropdownMenuEntry};
use crate::ui::components::{ui_button, ui_static_label};
use crate::ui::editor::gui::EditorGuiUiState;
use crate::ui::theme;

pub fn layout_viewer(
    document: LayoutDocument,
    snapshot: crate::ui::UiSnapshot,
    gui_state: EditorGuiUiState,
    dropdown_menu: DropdownMenuController,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let canvas_state = gui_state.layout_canvas(&document.path, &document.object_key);
    let fixtures = document.fixtures.clone();
    let groups = document.groups.clone();
    let bounds = document.render_bounds;
    let name = document.name.clone();
    let units = document.units;
    let scene_document = document.clone();

    v_stack((
        layout_header(name, units, bounds),
        layout_body(
            move || {
                snapshot
                    .get()
                    .active_layout_document
                    .as_ref()
                    .map(layout_canvas_scene)
                    .unwrap_or_else(|| layout_canvas_scene(&scene_document))
            },
            canvas_state,
            fixtures,
            groups,
            dropdown_menu,
            dispatch,
        ),
    ))
    .style(|s| {
        s.width_full()
            .height_full()
            .padding(theme::SPACE_12)
            .gap(theme::SPACE_8)
            .background(theme::color(theme::SURFACE))
    })
    .into_any()
}

fn layout_header(name: String, units: DistanceUnit, bounds: GeometryRenderBounds) -> impl IntoView {
    h_stack((
        ui_static_label("Layout").style(|s| s.font_bold()),
        ui_static_label(format!("{name}  {units:?}")),
        ui_static_label(format!(
            "Bounds {:.1},{:.1} to {:.1},{:.1}",
            bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y
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

fn layout_body(
    scene: impl Fn() -> CanvasScene + 'static,
    canvas_state: CanvasState,
    fixtures: Vec<LayoutFixturePlacement>,
    groups: Vec<LayoutGroupDocument>,
    dropdown_menu: DropdownMenuController,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let drag_dispatch = Rc::clone(&dispatch);
    let drag_begin_dispatch = Rc::clone(&dispatch);
    let drag_end_dispatch = Rc::clone(&dispatch);
    let menu_dispatch = Rc::clone(&dispatch);
    let canvas = canvas_with_state(canvas_state, scene);
    let canvas_id = canvas.id();
    h_stack((
        canvas
            .on_edit_drag_begin(move || {
                drag_begin_dispatch(AppAction::BeginDeferredPersistenceHold);
            })
            .on_edit_drag_end(move || {
                drag_end_dispatch(AppAction::EndDeferredPersistenceHold);
            })
            .on_drag_end(move |ids, dx, dy| {
                drag_dispatch(AppAction::NudgeLayoutFixtures {
                    fixture_ids: ids,
                    dx,
                    dy,
                });
            })
            .on_secondary_click(move |position, x, y| {
                dropdown_menu.open_at_view_point(
                    canvas_id,
                    position,
                    layout_canvas_menu_entries(x, y, Rc::clone(&menu_dispatch)),
                );
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
        s.width_full()
            .flex_grow(1.0)
            .height_full()
            .min_width(0.0)
            .min_height(0.0)
            .gap(theme::SPACE_12)
    })
}

fn layout_canvas_menu_entries(
    x: f64,
    y: f64,
    dispatch: crate::ui::UiDispatch,
) -> Vec<DropdownMenuEntry> {
    let new_fixture = Rc::clone(&dispatch);
    let import_fixture = Rc::clone(&dispatch);
    vec![
        DropdownMenuEntry::item("New Fixture", true, move || {
            new_fixture(AppAction::CreateInlineLayoutFixture { x, y });
        }),
        DropdownMenuEntry::item("Import Fixture", true, move || {
            let import_fixture = Rc::clone(&import_fixture);
            open_file(
                FileDialogOptions::new()
                    .title("Import Fixture")
                    .allowed_types(vec![FileSpec {
                        name: "Dawn",
                        extensions: &["dawn"],
                    }]),
                move |selection| {
                    if let Some(path) = selection.and_then(|info| info.path().first().cloned()) {
                        import_fixture(AppAction::StartImportLayoutFixture {
                            selected_file: path,
                            x,
                            y,
                        });
                    }
                },
            );
        }),
    ]
}

fn layout_right_rail(
    fixtures: Vec<LayoutFixturePlacement>,
    groups: Vec<LayoutGroupDocument>,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    scroll(
        v_stack((
            fixture_controls_section(fixtures.clone(), dispatch),
            groups_section(fixtures, groups),
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
    let id = fixture.id;
    let id_left = id;
    let id_right = id;
    let id_up = id;
    let id_down = id;
    let id_duplicate = id;
    let id_delete = id;

    v_stack((
        ui_static_label(fixture.name.clone()).style(|s| s.font_bold()),
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
                    nudge_left(AppAction::NudgeLayoutFixtures {
                        fixture_ids: vec![id_left],
                        dx: -theme::LAYOUT_NUDGE_STEP,
                        dy: 0.0,
                    })
                }),
                ui_button("Right").action(move || {
                    nudge_right(AppAction::NudgeLayoutFixtures {
                        fixture_ids: vec![id_right],
                        dx: theme::LAYOUT_NUDGE_STEP,
                        dy: 0.0,
                    })
                }),
                ui_button("Up").action(move || {
                    nudge_up(AppAction::NudgeLayoutFixtures {
                        fixture_ids: vec![id_up],
                        dx: 0.0,
                        dy: theme::LAYOUT_NUDGE_STEP,
                    })
                }),
                ui_button("Down").action(move || {
                    nudge_down(AppAction::NudgeLayoutFixtures {
                        fixture_ids: vec![id_down],
                        dx: 0.0,
                        dy: -theme::LAYOUT_NUDGE_STEP,
                    })
                }),
            ))
            .style(|s| s.gap(theme::SPACE_4).items_center()),
            h_stack((
                ui_button("Duplicate").action(move || {
                    duplicate(AppAction::DuplicateLayoutFixture {
                        fixture_id: id_duplicate,
                    })
                }),
                ui_button("Delete").action(move || {
                    delete(AppAction::DeleteLayoutFixture {
                        fixture_id: id_delete,
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

fn groups_section(
    fixtures: Vec<LayoutFixturePlacement>,
    groups: Vec<LayoutGroupDocument>,
) -> impl IntoView {
    let fixture_names = fixtures
        .into_iter()
        .map(|fixture| (fixture.id, fixture.name))
        .collect::<std::collections::BTreeMap<_, _>>();
    v_stack((
        ui_static_label("Groups").style(|s| s.font_bold()),
        v_stack_from_iter(groups.into_iter().map(|group| {
            let members = group
                .members
                .iter()
                .map(|id| {
                    fixture_names
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| id.to_string())
                })
                .collect::<Vec<_>>()
                .join(", ");
            ui_static_label(format!("{}  {}", group.name, members))
        })),
    ))
    .style(|s| s.width_full().gap(theme::SPACE_6))
}

fn layout_canvas_scene(document: &LayoutDocument) -> CanvasScene {
    let mut items = Vec::new();
    let mut layers = Vec::new();
    for fixture in &document.fixtures {
        layers.push(CanvasLayer {
            id: fixture.id.to_string(),
            visible: true,
        });
        let color = fixture_color(&fixture.id);
        let interaction = CanvasItemInteraction::Target {
            fixture_id: fixture.id,
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
                label: Some(fixture.name.clone()),
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
                label: Some(fixture.name.clone()),
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

fn fixture_color(id: &FixtureId) -> floem::peniko::Color {
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
    let hash =
        id.0.to_ne_bytes()
            .into_iter()
            .fold(0usize, |accumulator, byte| {
                accumulator.wrapping_mul(31).wrapping_add(byte as usize)
            });
    let (red, green, blue) = PALETTE[hash % PALETTE.len()];
    floem::peniko::Color::rgb8(red, green, blue)
}
