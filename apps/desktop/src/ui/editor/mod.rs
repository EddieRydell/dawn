use std::rc::Rc;

use dawn_project::analysis::{DiagnosticCode, ProjectAnalysis, ProjectDiagnostic};
use dawn_project::document::{DocumentViewId, FixtureDocument, LayoutDocument, SequenceDocument};
use dawn_project::path::{PathStringExt, Utf8PathBuf};
use floem::event::{Event, EventListener};
use floem::peniko::Brush;
use floem::prelude::*;
use floem::reactive::create_memo;
use floem::style::{CursorStyle, Foreground};
use floem::text::FamilyOwned;
use floem::views::editor::text::SimpleStyling;
use lucide_floem::{Icon, StrokeWidth};

use crate::actions::AppAction;
use crate::app_model::{AppSnapshot, PreviewRigKind};
use crate::editor_session::EditorViewMode;
use crate::ui::components::{ui_static_label, ui_text_editor};
use crate::ui::editor::gui::EditorGuiUiState;
use crate::ui::theme;

pub mod gui;

pub fn editor_view(
    snapshot: crate::ui::UiSnapshot,
    playback_clock: crate::ui::UiPlaybackClock,
    gui_state: EditorGuiUiState,
    dropdown_menu: crate::ui::components::dropdown_menu::DropdownMenuController,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    v_stack((
        dyn_container(move || snapshot.get(), {
            let dispatch = Rc::clone(&dispatch);
            move |state| tab_strip(state, Rc::clone(&dispatch)).into_any()
        }),
        editor_body(snapshot, playback_clock, gui_state, dropdown_menu, dispatch)
            .style(|s| s.flex_grow(1.0).min_height(0.0)),
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
        let tab_dispatch = Rc::clone(&dispatch);
        let tabs = h_stack_from_iter(state.tabs.into_iter().map(move |tab| {
            let active = active_file.as_ref() == Some(&tab.path);
            let dirty = if tab.is_dirty() { "*" } else { "" };
            let title = format!(
                "{}{}",
                tab.path
                    .file_name()
                    .map(str::to_string)
                    .unwrap_or_else(|| tab.path.to_slash_string()),
                dirty
            );
            editor_tab(title, tab.path, active, Rc::clone(&tab_dispatch))
        }));
        h_stack((
            tabs.style(|s| s.height_full().min_width(0.0)),
            empty().style(|s| s.flex_grow(1.0).min_width(0.0)),
            editor_mode_toggle(active_buffer, dispatch),
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
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let Some(buffer) = active_buffer else {
        return empty().into_any();
    };

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
    path: Utf8PathBuf,
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
    snapshot: crate::ui::UiSnapshot,
    playback_clock: crate::ui::UiPlaybackClock,
    gui_state: EditorGuiUiState,
    dropdown_menu: crate::ui::components::dropdown_menu::DropdownMenuController,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let body_route = create_memo(move |_| EditorBodyRoute::from_snapshot(&snapshot.get()));
    dyn_container(
        move || body_route.get(),
        move |route| {
            editor_body_for_key(
                snapshot,
                playback_clock,
                route,
                gui_state.clone(),
                dropdown_menu.clone(),
                Rc::clone(&dispatch),
            )
        },
    )
}

fn editor_body_for_key(
    snapshot: crate::ui::UiSnapshot,
    playback_clock: crate::ui::UiPlaybackClock,
    route: EditorBodyRoute,
    gui_state: EditorGuiUiState,
    dropdown_menu: crate::ui::components::dropdown_menu::DropdownMenuController,
    dispatch: crate::ui::UiDispatch,
) -> floem::AnyView {
    let Some(active) = route.active else {
        return center_message("Open a file from the project explorer").into_any();
    };

    match active {
        ActiveEditorRoute::Text { path, text } => {
            if is_effect_script_path(&path) {
                effect_script_editor(snapshot, path, text, dispatch).into_any()
            } else {
                source_editor(path, text, dispatch).into_any()
            }
        }
        ActiveEditorRoute::GuiFallback { path, text } => {
            source_editor(path, text, dispatch).into_any()
        }
        ActiveEditorRoute::Gui { view } => match view {
            GuiEditorRoute::Layout(document) => crate::ui::editor::gui::layout::layout_viewer(
                document,
                snapshot,
                gui_state,
                dropdown_menu,
                dispatch,
            )
            .into_any(),
            GuiEditorRoute::Fixture(document) => crate::ui::editor::gui::fixture::fixture_viewer(
                document, snapshot, gui_state, dispatch,
            )
            .into_any(),
            GuiEditorRoute::Sequence(document) => {
                crate::ui::editor::gui::sequence::sequence_viewer(
                    document,
                    snapshot,
                    playback_clock,
                    gui_state,
                    dropdown_menu,
                    dispatch,
                )
                .into_any()
            }
            GuiEditorRoute::Missing => center_message("No GUI editor for this document").into_any(),
        },
    }
}

#[derive(Debug, Clone)]
struct EditorBodyRoute {
    key: EditorBodyKey,
    active: Option<ActiveEditorRoute>,
}

impl PartialEq for EditorBodyRoute {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl EditorBodyRoute {
    fn from_snapshot(state: &AppSnapshot) -> Self {
        let key = EditorBodyKey::from_snapshot(state);
        let active = state.active_buffer.as_ref().map(|buffer| match key.mode {
            EditorViewMode::Text => ActiveEditorRoute::Text {
                path: buffer.path.clone(),
                text: buffer.text.clone(),
            },
            EditorViewMode::Gui => {
                if !is_dawn_path(&buffer.path) {
                    ActiveEditorRoute::GuiFallback {
                        path: buffer.path.clone(),
                        text: buffer.text.clone(),
                    }
                } else {
                    ActiveEditorRoute::Gui {
                        view: GuiEditorRoute::from_snapshot(state),
                    }
                }
            }
        });
        Self { key, active }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct EditorBodyKey {
    path: Option<Utf8PathBuf>,
    mode: EditorViewMode,
    gui_kind: EditorGuiKind,
    gui_object_key: Option<String>,
    gui_document_ready: bool,
}

impl EditorBodyKey {
    fn from_snapshot(state: &AppSnapshot) -> Self {
        let mode = state
            .active_buffer
            .as_ref()
            .map(|buffer| buffer.view_mode)
            .unwrap_or(EditorViewMode::Text);
        let (gui_kind, gui_object_key) = if mode == EditorViewMode::Gui {
            (
                EditorGuiKind::from_snapshot(state),
                active_gui_object_key(state),
            )
        } else {
            (EditorGuiKind::None, None)
        };
        Self {
            path: state.active_file.clone(),
            mode,
            gui_kind,
            gui_object_key,
            gui_document_ready: mode == EditorViewMode::Gui && active_gui_document_ready(state),
        }
    }
}

#[derive(Debug, Clone)]
enum ActiveEditorRoute {
    Text { path: Utf8PathBuf, text: String },
    GuiFallback { path: Utf8PathBuf, text: String },
    Gui { view: GuiEditorRoute },
}

#[derive(Debug, Clone)]
enum GuiEditorRoute {
    Layout(LayoutDocument),
    Fixture(FixtureDocument),
    Sequence(SequenceDocument),
    Missing,
}

impl GuiEditorRoute {
    fn from_snapshot(state: &AppSnapshot) -> Self {
        match EditorGuiKind::from_snapshot(state) {
            EditorGuiKind::Layout => state
                .active_layout_document
                .clone()
                .map(Self::Layout)
                .unwrap_or(Self::Missing),
            EditorGuiKind::Fixture => state
                .active_fixture_document
                .clone()
                .map(Self::Fixture)
                .unwrap_or(Self::Missing),
            EditorGuiKind::Sequence => state
                .active_sequence_document
                .clone()
                .map(Self::Sequence)
                .unwrap_or(Self::Missing),
            EditorGuiKind::None => Self::Missing,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorGuiKind {
    Layout,
    Fixture,
    Sequence,
    None,
}

impl EditorGuiKind {
    fn from_snapshot(state: &AppSnapshot) -> Self {
        let Some(descriptor) = state.active_descriptor.as_ref() else {
            return Self::None;
        };
        if descriptor
            .available_views
            .contains(&DocumentViewId::Sequence)
        {
            Self::Sequence
        } else if descriptor
            .available_views
            .contains(&DocumentViewId::Fixture)
        {
            Self::Fixture
        } else if descriptor.available_views.contains(&DocumentViewId::Layout) {
            Self::Layout
        } else {
            Self::None
        }
    }
}

fn active_gui_object_key(state: &AppSnapshot) -> Option<String> {
    match EditorGuiKind::from_snapshot(state) {
        EditorGuiKind::Layout => state
            .active_layout_document
            .as_ref()
            .map(|document| document.object_key.clone())
            .or_else(|| default_object_key(state, DocumentViewId::Layout)),
        EditorGuiKind::Fixture => state
            .active_fixture_document
            .as_ref()
            .and_then(|document| document.selected_object_key.clone())
            .or_else(|| default_object_key(state, DocumentViewId::Fixture)),
        EditorGuiKind::Sequence => state
            .active_sequence_document
            .as_ref()
            .map(|document| document.object_key.clone())
            .or_else(|| default_object_key(state, DocumentViewId::Sequence)),
        EditorGuiKind::None => None,
    }
}

fn active_gui_document_ready(state: &AppSnapshot) -> bool {
    match EditorGuiKind::from_snapshot(state) {
        EditorGuiKind::Layout => state.active_layout_document.is_some(),
        EditorGuiKind::Fixture => state.active_fixture_document.is_some(),
        EditorGuiKind::Sequence => state.active_sequence_document.is_some(),
        EditorGuiKind::None => false,
    }
}

fn default_object_key(state: &AppSnapshot, view: DocumentViewId) -> Option<String> {
    state
        .active_descriptor
        .as_ref()
        .and_then(|descriptor| descriptor.default_object_keys.get(&view).cloned())
}

fn source_editor(
    _path: Utf8PathBuf,
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
        .editor_style(|s| {
            s.cursor_color(theme::color(theme::TEXT_INVERTED))
                .current_line_color(theme::color(theme::SURFACE_CONTROL_HOVER))
        })
        .placeholder("File contents")
        .update(move |event| {
            let Some(editor) = event.editor else {
                return;
            };
            let next_text = editor.rope_text().text.to_string();
            dispatch_updates(AppAction::UpdateActiveText(next_text));
        })
        .style(|s| {
            s.width_full()
                .height_full()
                .background(theme::color(theme::SURFACE))
        })
}

fn effect_script_editor(
    snapshot: crate::ui::UiSnapshot,
    path: Utf8PathBuf,
    text: String,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let header_path = path.clone();
    let header_dispatch = Rc::clone(&dispatch);
    v_stack((
        dyn_container(
            move || {
                let state = snapshot.get();
                (state.analysis, state.diagnostics, header_path.clone())
            },
            move |(analysis, diagnostics, path)| {
                effect_script_header(path, analysis, diagnostics, Rc::clone(&header_dispatch))
                    .into_any()
            },
        ),
        source_editor(path, text, dispatch).style(|s| s.flex_grow(1.0).min_height(0.0)),
    ))
    .style(|s| s.width_full().height_full())
}

fn effect_script_header(
    path: Utf8PathBuf,
    analysis: Option<ProjectAnalysis>,
    diagnostics: Vec<ProjectDiagnostic>,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let script = analysis
        .as_ref()
        .and_then(|analysis| analysis.scripts.get(&path.to_slash_string()));
    let script_diagnostics = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.path == path && diagnostic.code == DiagnosticCode::Script)
        .collect::<Vec<_>>();
    let has_script_errors = !script_diagnostics.is_empty();
    let status = if script_diagnostics.is_empty()
        && script
            .as_ref()
            .is_some_and(|script| script.result.as_ref().is_ok())
    {
        "Compiled"
    } else {
        "Compile errors"
    };
    let name = script
        .and_then(|script| script.result.as_ref().ok())
        .map(|script| script.name.clone())
        .unwrap_or_else(|| "Effect script".to_string());
    let params = script
        .and_then(|script| script.result.as_ref().ok())
        .map(|script| {
            script
                .params
                .iter()
                .map(|param| format!("{}: {}", param.name, param.value_type))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|summary| !summary.is_empty())
        .unwrap_or_else(|| "No parameters".to_string());
    let diagnostic_summary = match script_diagnostics.len() {
        0 => "No script diagnostics".to_string(),
        1 => script_diagnostics[0].message.clone(),
        count => format!("{count} script diagnostics"),
    };

    let open_preview = Rc::clone(&dispatch);
    v_stack((
        h_stack((
            ui_static_label(status).style(move |s| {
                let color = if !has_script_errors {
                    theme::color(theme::TEXT)
                } else {
                    theme::color(theme::DANGER)
                };
                s.font_size(theme::FONT_SMALL)
                    .font_bold()
                    .color(color)
                    .set(Foreground, Brush::Solid(color))
            }),
            ui_static_label(name).style(|s| {
                s.font_size(theme::FONT_SMALL)
                    .font_bold()
                    .color(theme::color(theme::TEXT))
                    .set(Foreground, Brush::Solid(theme::color(theme::TEXT)))
            }),
            empty().style(|s| s.flex_grow(1.0).min_width(0.0)),
            effect_rig_button(PreviewRigKind::Strand, Rc::clone(&dispatch)),
            effect_rig_button(PreviewRigKind::VerticalStrand, Rc::clone(&dispatch)),
            effect_rig_button(PreviewRigKind::Circle, Rc::clone(&dispatch)),
            effect_rig_button(PreviewRigKind::Grid, Rc::clone(&dispatch)),
            crate::ui::components::ui_button("Open Preview").action(move || {
                open_preview(AppAction::OpenPreviewWindow);
            }),
        ))
        .style(|s| s.items_center().gap(theme::SPACE_8)),
        ui_static_label(params).style(|s| {
            s.width_full()
                .font_size(theme::FONT_SMALL)
                .color(theme::color(theme::MUTED))
                .set(Foreground, Brush::Solid(theme::color(theme::MUTED)))
                .text_ellipsis()
        }),
        ui_static_label(diagnostic_summary).style(|s| {
            s.width_full()
                .font_size(theme::FONT_SMALL)
                .color(theme::color(theme::MUTED))
                .set(Foreground, Brush::Solid(theme::color(theme::MUTED)))
                .text_ellipsis()
        }),
    ))
    .style(|s| {
        s.width_full()
            .padding_horiz(theme::SPACE_10)
            .padding_vert(theme::SPACE_8)
            .gap(theme::SPACE_4)
            .border_bottom(theme::BORDER_WIDTH)
            .border_color(theme::color(theme::BORDER))
            .background(theme::color(theme::PANEL))
    })
}

fn effect_rig_button(rig: PreviewRigKind, dispatch: crate::ui::UiDispatch) -> impl IntoView {
    crate::ui::components::ui_button(rig.label()).action(move || {
        dispatch(AppAction::SelectPreviewRig(rig));
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

fn is_dawn_path(path: &Utf8PathBuf) -> bool {
    path.as_std_path()
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension == "dawn")
}

fn is_effect_script_path(path: &Utf8PathBuf) -> bool {
    path.file_name()
        .is_some_and(|name| name.ends_with(".effect.dawn"))
}
