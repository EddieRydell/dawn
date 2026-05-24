use std::rc::Rc;

use floem::prelude::*;

use crate::ui::theme;

pub fn workbench_view(
    snapshot: crate::ui::UiSnapshot,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let explorer = crate::ui::project_tree::ExplorerUiState::new();
    dyn_container(
        move || snapshot.get(),
        move |state| {
            explorer.reset_for_root(state.project.as_ref().map(|project| project.root.clone()));
            let mut panes = Vec::new();
            if state.panel_layout.left_visible {
                panes.push(
                    crate::ui::project_tree::project_tree_view(
                        state.clone(),
                        explorer.clone(),
                        Rc::clone(&dispatch),
                    )
                    .style(move |s| {
                        s.width(state.panel_layout.left_width)
                            .height_full()
                            .border_right(1.0)
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
                            .min_width(320.0)
                            .height_full()
                    })
                    .into_any(),
            );

            if state.panel_layout.right_visible {
                panes.push(
                    right_pane(state.clone(), Rc::clone(&dispatch))
                        .style(move |s| {
                            s.width(state.panel_layout.right_width)
                                .height_full()
                                .border_left(1.0)
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

fn right_pane(
    state: crate::app_model::AppSnapshot,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    use crate::layout_persistence::RightPaneTab;

    let diagnostics = Rc::clone(&dispatch);
    let preview = Rc::clone(&dispatch);
    let inspector = Rc::clone(&dispatch);
    let active = state.panel_layout.active_right_tab;

    let body = match active {
        RightPaneTab::Diagnostics => {
            crate::ui::diagnostics::diagnostics_view(state.clone(), Rc::clone(&dispatch)).into_any()
        }
        RightPaneTab::Preview => {
            crate::ui::preview::preview_view(state.clone(), Rc::clone(&dispatch)).into_any()
        }
        RightPaneTab::Inspector => inspector_view(state.clone()).into_any(),
    };

    v_stack((
        h_stack((
            tab_button(
                "Diagnostics",
                active == RightPaneTab::Diagnostics,
                move || diagnostics(AppAction::SetRightPaneTab(RightPaneTab::Diagnostics)),
            ),
            tab_button("Preview", active == RightPaneTab::Preview, move || {
                preview(AppAction::SetRightPaneTab(RightPaneTab::Preview))
            }),
            tab_button("Inspector", active == RightPaneTab::Inspector, move || {
                inspector(AppAction::SetRightPaneTab(RightPaneTab::Inspector))
            }),
        ))
        .style(|s| {
            s.height(32.0)
                .items_center()
                .gap(4.0)
                .padding_horiz(8.0)
                .background(theme::color(theme::PANEL))
        }),
        body.style(|s| s.flex_grow(1.0).min_height(0.0)),
    ))
    .style(|s| s.height_full().background(theme::color(theme::SURFACE)))
}

fn tab_button(label: &'static str, active: bool, action: impl Fn() + 'static) -> impl IntoView {
    button(label).action(action).style(move |s| {
        let bg = if active {
            theme::color(theme::SELECTED)
        } else {
            theme::color(theme::PANEL)
        };
        s.height(24.0).padding_horiz(8.0).background(bg)
    })
}

fn inspector_view(state: crate::app_model::AppSnapshot) -> impl IntoView {
    let descriptor = state.active_descriptor.clone();
    let object_rows = descriptor
        .as_ref()
        .map(|descriptor| {
            descriptor
                .objects
                .iter()
                .map(|object| format!("{}  {}", object.kind, object.key))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    v_stack((
        static_label("Inspector").style(|s| s.font_bold().margin_bottom(8.0)),
        label(move || {
            descriptor
                .as_ref()
                .map(|descriptor| descriptor.path.clone())
                .unwrap_or_else(|| "No active document".to_string())
        }),
        v_stack_from_iter(object_rows.into_iter().map(static_label)).style(|s| s.margin_top(10.0)),
    ))
    .style(|s| s.padding(10.0).gap(4.0))
}

use crate::actions::AppAction;
