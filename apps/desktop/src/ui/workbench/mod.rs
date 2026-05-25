use std::rc::Rc;

use floem::prelude::*;

use crate::ui::components::ui_button;
use crate::ui::theme;

pub fn workbench_view(
    snapshot: crate::ui::UiSnapshot,
    dropdown_menu: crate::ui::dropdown_menu::DropdownMenuController,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let explorer = crate::ui::project_tree::ExplorerUiState::new();
    dyn_container(
        move || snapshot.get(),
        move |state| {
            explorer.reset_for_root(state.project_root.clone());
            let mut panes = Vec::new();
            if state.workbench_layout.project_tree_visible {
                panes.push(
                    crate::ui::project_tree::project_tree_view(
                        state.clone(),
                        explorer.clone(),
                        dropdown_menu.clone(),
                        Rc::clone(&dispatch),
                    )
                    .style(move |s| {
                        s.width(state.workbench_layout.project_tree_width)
                            .height_full()
                            .border_right(theme::BORDER_WIDTH)
                            .border_color(theme::color(theme::BORDER))
                    })
                    .into_any(),
                );
            }

            panes.push(
                crate::ui::editor::editor_view(state.clone(), Rc::clone(&dispatch))
                    .style(|s| {
                        s.flex_grow(1.0)
                            .flex_basis(0.0)
                            .min_width(theme::MIN_EDITOR_WIDTH)
                            .height_full()
                    })
                    .into_any(),
            );

            if state.workbench_layout.inspector_visible {
                panes.push(
                    inspector_pane(state.clone(), Rc::clone(&dispatch))
                        .style(move |s| {
                            s.width(state.workbench_layout.inspector_width)
                                .height_full()
                                .border_left(theme::BORDER_WIDTH)
                                .border_color(theme::color(theme::BORDER))
                        })
                        .into_any(),
                );
            }

            h_stack_from_iter(panes)
                .style(|s| s.width_full().height_full().min_height(0.0))
                .into_any()
        },
    )
    .style(|s| s.width_full().flex_grow(1.0).min_height(0.0))
}

fn inspector_pane(
    state: crate::app_model::AppSnapshot,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    use crate::layout_persistence::InspectorTab;

    let diagnostics = Rc::clone(&dispatch);
    let preview = Rc::clone(&dispatch);
    let active = state.workbench_layout.active_inspector_tab;

    let body = match active {
        InspectorTab::Diagnostics => {
            crate::ui::inspector::diagnostics::diagnostics_view(state.clone(), Rc::clone(&dispatch))
                .into_any()
        }
        InspectorTab::Preview => {
            crate::ui::inspector::preview::preview_view(state.clone(), Rc::clone(&dispatch))
                .into_any()
        }
    };

    v_stack((
        h_stack((
            tab_button(
                "Diagnostics",
                active == InspectorTab::Diagnostics,
                move || diagnostics(AppAction::SetInspectorTab(InspectorTab::Diagnostics)),
            ),
            tab_button("Preview", active == InspectorTab::Preview, move || {
                preview(AppAction::SetInspectorTab(InspectorTab::Preview))
            }),
        ))
        .style(|s| {
            s.height(theme::TOOLBAR_HEIGHT)
                .items_center()
                .gap(theme::SPACE_4)
                .padding_horiz(theme::SPACE_8)
                .background(theme::color(theme::PANEL))
        }),
        body.style(|s| s.flex_grow(1.0).min_height(0.0)),
    ))
    .style(|s| s.height_full().background(theme::color(theme::SURFACE)))
}

fn tab_button(label: &'static str, active: bool, action: impl Fn() + 'static) -> impl IntoView {
    ui_button(label).action(action).style(move |s| {
        let bg = if active {
            theme::color(theme::SELECTED)
        } else {
            theme::color(theme::PANEL)
        };
        s.height(theme::ROW_HEIGHT)
            .padding_horiz(theme::SPACE_8)
            .background(bg)
    })
}

use crate::actions::AppAction;
