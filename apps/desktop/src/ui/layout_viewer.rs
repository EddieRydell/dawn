use std::rc::Rc;

use floem::prelude::*;

use crate::actions::AppAction;
use crate::app_model::AppSnapshot;
use crate::ui::theme;

pub fn layout_viewer(state: AppSnapshot, dispatch: crate::ui::UiDispatch) -> impl IntoView {
    let Some(document) = state.active_layout_document.clone() else {
        return empty().into_any();
    };
    let fixtures = document.fixtures.clone();
    let groups = document.groups.clone();
    let bounds = document.render_bounds;

    h_stack((
        v_stack((
            static_label("Layout").style(|s| s.font_bold()),
            label(move || format!("{}  {:?}", document.name, document.units)),
            label(move || {
                format!(
                    "Bounds {:.1},{:.1} to {:.1},{:.1}",
                    bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y
                )
            }),
            scroll(v_stack_from_iter(fixtures.into_iter().map(
                move |fixture| {
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
                        static_label(fixture.id.clone()).style(|s| s.font_bold()),
                        static_label(format!(
                            "{}  {}",
                            fixture.resolved_fixture.name,
                            fixture.resolved_fixture.geometry_summary
                        )),
                        static_label(format!(
                            "x {:.2}  y {:.2}  z {:.2}",
                            fixture.transform.position.x,
                            fixture.transform.position.y,
                            fixture.transform.position.z
                        )),
                        h_stack((
                            button("Left").action(move || {
                                nudge_left(AppAction::NudgeLayoutFixture {
                                    id: id_left.clone(),
                                    dx: -0.25,
                                    dy: 0.0,
                                })
                            }),
                            button("Right").action(move || {
                                nudge_right(AppAction::NudgeLayoutFixture {
                                    id: id_right.clone(),
                                    dx: 0.25,
                                    dy: 0.0,
                                })
                            }),
                            button("Up").action(move || {
                                nudge_up(AppAction::NudgeLayoutFixture {
                                    id: id_up.clone(),
                                    dx: 0.0,
                                    dy: 0.25,
                                })
                            }),
                            button("Down").action(move || {
                                nudge_down(AppAction::NudgeLayoutFixture {
                                    id: id_down.clone(),
                                    dx: 0.0,
                                    dy: -0.25,
                                })
                            }),
                            button("Duplicate").action(move || {
                                duplicate(AppAction::DuplicateLayoutFixture {
                                    id: id_duplicate.clone(),
                                })
                            }),
                            button("Delete").action(move || {
                                delete(AppAction::DeleteLayoutFixture {
                                    id: id_delete.clone(),
                                })
                            }),
                        ))
                        .style(|s| s.gap(4.0).items_center()),
                    ))
                    .style(|s| {
                        s.padding(8.0)
                            .gap(3.0)
                            .border_bottom(1.0)
                            .border_color(theme::color(theme::BORDER))
                    })
                },
            )))
            .style(|s| s.flex_grow(1.0).min_height(0.0)),
        ))
        .style(|s| s.flex_grow(1.0).min_width(0.0).gap(8.0)),
        v_stack((
            static_label("Groups").style(|s| s.font_bold()),
            v_stack_from_iter(groups.into_iter().map(|group| {
                static_label(format!("{}  {}", group.name, group.members.join(", ")))
            })),
        ))
        .style(|s| {
            s.width(240.0)
                .padding_left(12.0)
                .gap(6.0)
                .border_left(1.0)
                .border_color(theme::color(theme::BORDER))
        }),
    ))
    .style(|s| {
        s.height_full()
            .padding(12.0)
            .gap(12.0)
            .background(theme::color(theme::SURFACE))
    })
    .into_any()
}
