use std::rc::Rc;

use dawn_project::document::DocumentViewId;
use dawn_project::path::ProjectPath;
use floem::prelude::*;

use crate::actions::AppAction;
use crate::app_model::AppSnapshot;
use crate::editor_session::EditorViewMode;
use crate::ui::theme;

pub fn editor_view(state: AppSnapshot, dispatch: crate::ui::UiDispatch) -> impl IntoView {
    v_stack((
        tab_strip(state.clone(), Rc::clone(&dispatch)),
        mode_strip(state.clone(), Rc::clone(&dispatch)),
        editor_body(state, dispatch).style(|s| s.flex_grow(1.0).min_height(0.0)),
    ))
    .style(|s| s.height_full().background(theme::color(theme::SURFACE)))
}

fn tab_strip(state: AppSnapshot, dispatch: crate::ui::UiDispatch) -> impl IntoView {
    if state.tabs.is_empty() {
        h_stack((static_label("No open editors"),))
            .style(|s| {
                s.height(34.0)
                    .items_center()
                    .padding_horiz(10.0)
                    .border_bottom(1.0)
                    .border_color(theme::color(theme::BORDER))
                    .background(theme::color(theme::PANEL))
            })
            .into_any()
    } else {
        h_stack_from_iter(state.tabs.into_iter().map(move |tab| {
            let activate = Rc::clone(&dispatch);
            let close = Rc::clone(&dispatch);
            let path = tab.path.clone();
            let close_path = tab.path.clone();
            let active = state.active_file.as_ref() == Some(&tab.path);
            let dirty = if tab.is_dirty() { "*" } else { "" };
            let title = format!(
                "{}{}",
                path.file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_slash_string()),
                dirty
            );
            h_stack((
                button(title).action(move || activate(AppAction::SetActiveFile(path.clone()))),
                button("x").action(move || close(AppAction::CloseFile(close_path.clone()))),
            ))
            .style(move |s| {
                let bg = if active {
                    theme::color(theme::SURFACE)
                } else {
                    theme::color(theme::PANEL_DARK)
                };
                s.height(32.0)
                    .items_center()
                    .padding_horiz(4.0)
                    .border_right(1.0)
                    .border_color(theme::color(theme::BORDER))
                    .background(bg)
            })
        }))
        .style(|s| {
            s.height(34.0)
                .items_center()
                .border_bottom(1.0)
                .border_color(theme::color(theme::BORDER))
                .background(theme::color(theme::PANEL))
        })
        .into_any()
    }
}

fn mode_strip(state: AppSnapshot, dispatch: crate::ui::UiDispatch) -> impl IntoView {
    let Some(buffer) = state.active_buffer.clone() else {
        return empty().into_any();
    };
    let text_dispatch = Rc::clone(&dispatch);
    let gui_dispatch = Rc::clone(&dispatch);
    let save = Rc::clone(&dispatch);
    let path = buffer.path.clone();
    let gui_path = buffer.path.clone();
    let is_dawn = is_dawn_path(&buffer.path);
    let has_gui = state.active_descriptor.as_ref().is_some_and(|descriptor| {
        descriptor.available_views.contains(&DocumentViewId::Layout)
            || descriptor
                .available_views
                .contains(&DocumentViewId::Fixture)
    });

    let mut controls = Vec::new();
    controls.push(
        button("Text")
            .action(move || {
                text_dispatch(AppAction::SetEditorViewMode {
                    path: path.clone(),
                    mode: EditorViewMode::Text,
                })
            })
            .into_any(),
    );
    if is_dawn {
        controls.push(
            button("GUI")
                .action(move || {
                    if has_gui {
                        gui_dispatch(AppAction::SetEditorViewMode {
                            path: gui_path.clone(),
                            mode: EditorViewMode::Gui,
                        })
                    }
                })
                .into_any(),
        );
    }
    controls.push(
        button("Save")
            .action(move || save(AppAction::SaveActiveFile))
            .into_any(),
    );
    controls.push(
        label(move || {
            state
                .active_descriptor
                .as_ref()
                .map(|descriptor| {
                    descriptor
                        .objects
                        .iter()
                        .map(|object| object.kind.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_else(|| "Text file".to_string())
        })
        .style(|s| s.margin_left(8.0).color(theme::color(theme::MUTED)))
        .into_any(),
    );

    h_stack_from_iter(controls)
        .style(|s| {
            s.height(32.0)
                .items_center()
                .gap(6.0)
                .padding_horiz(10.0)
                .border_bottom(1.0)
                .border_color(theme::color(theme::BORDER))
                .background(theme::color(theme::PANEL))
        })
        .into_any()
}

fn editor_body(state: AppSnapshot, dispatch: crate::ui::UiDispatch) -> impl IntoView {
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
                crate::ui::layout_viewer::layout_viewer(state, dispatch).into_any()
            } else if state.active_fixture_document.is_some() {
                crate::ui::fixture_viewer::fixture_viewer(state, dispatch).into_any()
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
    text_editor(text)
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
                .font_family("Cascadia Mono".to_string())
                .font_size(13.0)
                .background(theme::color(theme::SURFACE))
        })
}

fn center_message(message: &'static str) -> impl IntoView {
    container(static_label(message)).style(|s| {
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
