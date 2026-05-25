use std::rc::Rc;

use dawn_project::document::{DocumentDescriptor, DocumentViewId};
use dawn_project::path::ProjectPath;
use floem::event::{Event, EventListener};
use floem::peniko::Brush;
use floem::prelude::*;
use floem::style::{CursorStyle, Foreground};
use floem::text::FamilyOwned;
use floem::views::editor::text::SimpleStyling;
use lucide_floem::{Icon, StrokeWidth};

use crate::actions::AppAction;
use crate::app_model::AppSnapshot;
use crate::editor_session::EditorViewMode;
use crate::ui::components::{ui_static_label, ui_text_editor};
use crate::ui::editor::gui::EditorGuiUiState;
use crate::ui::theme;

pub mod gui;

pub fn editor_view(
    state: AppSnapshot,
    gui_state: EditorGuiUiState,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    v_stack((
        tab_strip(state.clone(), Rc::clone(&dispatch)),
        editor_body(state, gui_state, dispatch).style(|s| s.flex_grow(1.0).min_height(0.0)),
    ))
    .style(|s| s.height_full().background(theme::color(theme::SURFACE)))
}

fn tab_strip(state: AppSnapshot, dispatch: crate::ui::UiDispatch) -> impl IntoView {
    if state.tabs.is_empty() {
        h_stack((ui_static_label("No open files"),))
            .style(|s| {
                s.height(theme::TAB_STRIP_HEIGHT)
                    .items_center()
                    .padding_horiz(theme::SPACE_10)
                    .border_bottom(theme::BORDER_WIDTH)
                    .border_color(theme::color(theme::BORDER))
                    .background(theme::color(theme::PANEL))
            })
            .into_any()
    } else {
        let active_file = state.active_file.clone();
        let active_buffer = state.active_buffer.clone();
        let active_descriptor = state.active_descriptor.clone();
        let tab_dispatch = Rc::clone(&dispatch);
        let tabs = h_stack_from_iter(state.tabs.into_iter().map(move |tab| {
            let active = active_file.as_ref() == Some(&tab.path);
            let dirty = if tab.is_dirty() { "*" } else { "" };
            let title = format!(
                "{}{}",
                tab.path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| tab.path.to_slash_string()),
                dirty
            );
            editor_tab(title, tab.path, active, Rc::clone(&tab_dispatch))
        }));
        h_stack((
            tabs.style(|s| s.height_full().min_width(0.0)),
            empty().style(|s| s.flex_grow(1.0).min_width(0.0)),
            editor_mode_toggle(active_buffer, active_descriptor, dispatch),
        ))
        .style(|s| {
            s.width_full()
                .height(theme::TAB_STRIP_HEIGHT)
                .items_center()
                .border_bottom(theme::BORDER_WIDTH)
                .border_color(theme::color(theme::BORDER))
                .background(theme::color(theme::PANEL))
        })
        .into_any()
    }
}

fn editor_mode_toggle(
    active_buffer: Option<crate::editor_session::EditorBuffer>,
    active_descriptor: Option<DocumentDescriptor>,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let Some(buffer) = active_buffer else {
        return empty().into_any();
    };
    let has_gui = active_descriptor.as_ref().is_some_and(|descriptor| {
        descriptor.available_views.contains(&DocumentViewId::Layout)
            || descriptor
                .available_views
                .contains(&DocumentViewId::Fixture)
    });
    if !has_gui {
        return empty().into_any();
    }

    let text_path = buffer.path.clone();
    let gui_path = buffer.path.clone();
    let text_dispatch = Rc::clone(&dispatch);
    let gui_dispatch = Rc::clone(&dispatch);
    h_stack((
        editor_mode_button(
            "Text",
            buffer.view_mode == EditorViewMode::Text,
            move || {
                text_dispatch(AppAction::SetEditorViewMode {
                    path: text_path.clone(),
                    mode: EditorViewMode::Text,
                });
            },
        ),
        editor_mode_button("GUI", buffer.view_mode == EditorViewMode::Gui, move || {
            gui_dispatch(AppAction::SetEditorViewMode {
                path: gui_path.clone(),
                mode: EditorViewMode::Gui,
            });
        }),
    ))
    .style(|s| {
        s.items_center()
            .margin_right(theme::SPACE_8)
            .padding(theme::SPACE_2)
            .gap(theme::SPACE_2)
            .border(theme::BORDER_WIDTH)
            .border_color(theme::color(theme::BORDER))
            .border_radius(theme::CONTROL_RADIUS)
            .background(theme::color(theme::PANEL_DARK))
    })
    .into_any()
}

fn editor_mode_button(
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
        let background = if active {
            theme::color(theme::SURFACE_CONTROL_ACTIVE)
        } else {
            theme::color(theme::PANEL_DARK)
        };
        s.height(24.0)
            .min_width(44.0)
            .items_center()
            .justify_center()
            .padding_horiz(theme::SPACE_8)
            .border_radius(theme::CONTROL_RADIUS)
            .background(background)
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

fn editor_tab(
    title: String,
    path: ProjectPath,
    active: bool,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let activate = Rc::clone(&dispatch);
    let close = Rc::clone(&dispatch);
    let activate_path = path.clone();
    let close_path = path;

    h_stack((
        ui_static_label(title).style(move |s| {
            let text_color = if active {
                theme::color(theme::TEXT)
            } else {
                theme::color(theme::MUTED)
            };
            s.flex_grow(1.0)
                .min_width(0.0)
                .padding_left(theme::SPACE_10)
                .font_size(theme::FONT_SMALL)
                .color(text_color)
                .set(Foreground, Brush::Solid(text_color))
                .text_ellipsis()
        }),
        close_tab_button(move || close(AppAction::CloseFile(close_path.clone()))),
    ))
    .on_event_stop(EventListener::PointerDown, move |event| {
        if let Event::PointerDown(event) = event {
            if event.button.is_primary() {
                activate(AppAction::SetActiveFile(activate_path.clone()));
            }
        }
    })
    .style(move |s| {
        let bg = if active {
            theme::color(theme::SURFACE)
        } else {
            theme::color(theme::PANEL_DARK)
        };
        s.width(176.0)
            .height(theme::TAB_HEIGHT)
            .items_center()
            .gap(theme::SPACE_6)
            .padding_right(theme::SPACE_6)
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

fn close_tab_button(action: impl Fn() + 'static) -> impl IntoView {
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

fn editor_body(
    state: AppSnapshot,
    gui_state: EditorGuiUiState,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let Some(buffer) = state.active_buffer.clone() else {
        return center_message("Open a file from the project explorer").into_any();
    };

    match buffer.view_mode {
        EditorViewMode::Text => source_editor(buffer.path, buffer.text, dispatch).into_any(),
        EditorViewMode::Gui => {
            if !is_dawn_path(&buffer.path) {
                return source_editor(buffer.path, buffer.text, dispatch).into_any();
            }
            if state.active_layout_document.is_some() {
                crate::ui::editor::gui::layout::layout_viewer(state, gui_state, dispatch).into_any()
            } else if state.active_fixture_document.is_some() {
                crate::ui::editor::gui::fixture::fixture_viewer(state, dispatch).into_any()
            } else {
                center_message("No GUI editor for this document").into_any()
            }
        }
    }
}

fn source_editor(
    path: ProjectPath,
    text: String,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let dispatch_updates = Rc::clone(&dispatch);
    ui_text_editor(text)
        .with_editor(|editor| {
            let mut styling = SimpleStyling::builder();
            styling
                .font_family(vec![FamilyOwned::Name(theme::MONO_FONT.to_string())])
                .font_size(theme::FONT_EDITOR as usize);
            editor.update_doc(editor.doc(), Some(Rc::new(styling.build())));
        })
        .placeholder("File contents")
        .update(move |event| {
            let Some(editor) = event.editor else {
                return;
            };
            let next_text = editor.rope_text().text.to_string();
            dispatch_updates(AppAction::SetActiveFile(path.clone()));
            dispatch_updates(AppAction::UpdateActiveText(next_text));
        })
        .style(|s| {
            s.width_full()
                .height_full()
                .background(theme::color(theme::SURFACE))
        })
}

fn center_message(message: &'static str) -> impl IntoView {
    container(ui_static_label(message)).style(|s| {
        s.width_full()
            .height_full()
            .items_center()
            .justify_center()
            .color(theme::color(theme::MUTED))
    })
}

fn is_dawn_path(path: &ProjectPath) -> bool {
    path.as_path()
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension == "dawn")
}
