use std::rc::Rc;

use floem::event::{Event, EventListener};
use floem::peniko::Brush;
use floem::prelude::*;
use floem::style::{CursorStyle, Foreground};
use lucide_floem::{Icon, StrokeWidth};

use crate::ui::components::ui_static_label;
use crate::ui::theme;

pub fn workbench_view(
    snapshot: crate::ui::UiSnapshot,
    playback_clock: crate::ui::UiPreviewSnapshot,
    dropdown_menu: crate::ui::components::dropdown_menu::DropdownMenuController,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let explorer = crate::ui::project_tree::ExplorerUiState::new();
    let editor_gui = crate::ui::editor::gui::EditorGuiUiState::new();
    h_stack((
        project_tree_slot(
            snapshot,
            explorer,
            dropdown_menu.clone(),
            Rc::clone(&dispatch),
        ),
        crate::ui::editor::editor_view(
            snapshot,
            playback_clock,
            editor_gui,
            dropdown_menu,
            Rc::clone(&dispatch),
        )
        .style(|s| {
            s.flex_grow(1.0)
                .flex_basis(0.0)
                .min_width(theme::MIN_EDITOR_WIDTH)
                .height_full()
        }),
        inspector_slot(snapshot, dispatch),
    ))
    .style(|s| s.width_full().height_full().min_height(0.0))
    .style(|s| s.width_full().flex_grow(1.0).min_height(0.0))
}

fn project_tree_slot(
    snapshot: crate::ui::UiSnapshot,
    explorer: crate::ui::project_tree::ExplorerUiState,
    dropdown_menu: crate::ui::components::dropdown_menu::DropdownMenuController,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    dyn_container(
        move || snapshot.get(),
        move |state| {
            if !state.workbench_layout.project_tree_visible {
                return empty().into_any();
            }
            explorer.reset_for_root(state.project_root.clone());
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
            .into_any()
        },
    )
}

fn inspector_slot(
    snapshot: crate::ui::UiSnapshot,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    dyn_container(
        move || snapshot.get(),
        move |state| {
            if !state.workbench_layout.inspector_visible {
                return empty().into_any();
            }
            inspector_pane(state.clone(), Rc::clone(&dispatch))
                .style(move |s| {
                    s.width(state.workbench_layout.inspector_width)
                        .height_full()
                        .border_left(theme::BORDER_WIDTH)
                        .border_color(theme::color(theme::BORDER))
                })
                .into_any()
        },
    )
}

fn inspector_pane(
    state: crate::app_model::AppSnapshot,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    use crate::layout_persistence::InspectorTab;

    let diagnostics = Rc::clone(&dispatch);
    let close = Rc::clone(&dispatch);
    let active = state.workbench_layout.active_inspector_tab;

    let body = match active {
        InspectorTab::Diagnostics => {
            crate::ui::inspector::diagnostics::diagnostics_view(state.clone(), Rc::clone(&dispatch))
                .into_any()
        }
        InspectorTab::Preview => {
            crate::ui::inspector::diagnostics::diagnostics_view(state.clone(), Rc::clone(&dispatch))
                .into_any()
        }
    };

    v_stack((
        h_stack((
            inspector_tab("Diagnostics", true, move || {
                diagnostics(AppAction::SetInspectorTab(InspectorTab::Diagnostics))
            }),
            empty().style(|s| s.flex_grow(1.0).min_width(0.0)),
            close_pane_button(move || close(AppAction::ToggleInspector))
                .style(|s| s.margin_right(theme::SPACE_6)),
        ))
        .style(|s| {
            s.height(theme::TAB_STRIP_HEIGHT)
                .items_center()
                .border_bottom(theme::BORDER_WIDTH)
                .border_color(theme::color(theme::BORDER))
                .background(theme::color(theme::PANEL))
        }),
        body.style(|s| s.flex_grow(1.0).min_height(0.0)),
    ))
    .style(|s| s.height_full().background(theme::color(theme::SURFACE)))
}

fn close_pane_button(action: impl Fn() + 'static) -> impl IntoView {
    container(Icon::X.style(|s| {
        s.size(13.0, 13.0)
            .set(StrokeWidth, 1.8)
            .set(Foreground, Brush::Solid(theme::color(theme::MUTED)))
    }))
    .on_event_stop(EventListener::PointerDown, move |event| {
        if let Event::PointerDown(event) = event {
            if event.button.is_primary() {
                action();
            }
        }
    })
    .style(|s| {
        s.size(20.0, 20.0)
            .items_center()
            .justify_center()
            .border_radius(theme::CONTROL_RADIUS)
            .cursor(CursorStyle::Pointer)
            .hover(|s| {
                s.background(theme::color(theme::SURFACE_CONTROL_HOVER))
                    .set(Foreground, Brush::Solid(theme::color(theme::TEXT)))
            })
    })
}

fn inspector_tab(label: &'static str, active: bool, action: impl Fn() + 'static) -> impl IntoView {
    container(ui_static_label(label).style(move |s| {
        let text_color = if active {
            theme::color(theme::TEXT)
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
            theme::color(theme::SURFACE)
        } else {
            theme::color(theme::PANEL_DARK)
        };
        s.height(theme::TAB_HEIGHT)
            .items_center()
            .padding_horiz(theme::SPACE_10)
            .border_right(theme::BORDER_WIDTH)
            .border_color(theme::color(theme::BORDER))
            .background(bg)
            .cursor(CursorStyle::Pointer)
            .hover(move |s| {
                if active {
                    s
                } else {
                    s.background(theme::color(theme::PANEL))
                }
            })
    })
}

use crate::actions::AppAction;
