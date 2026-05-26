use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use dawn_project::document::DocumentViewId;
use floem::event::{Event, EventListener, EventPropagation};
use floem::file::FileDialogOptions;
use floem::file_action::open_file;
use floem::keyboard::Key;
use floem::kurbo::Size;
use floem::peniko::Brush;
use floem::prelude::*;
use floem::style::Foreground;
use floem::window::{close_window, WindowConfig, WindowId};
use floem::{action, views::drag_window_area, Application};
use lucide_floem::{Icon, StrokeWidth};

use crate::actions::AppAction;
use crate::app_model::AppModel;
use crate::editor_session::EditorViewMode;
use crate::ui::components::dropdown_menu::{
    dropdown_menu_layer, DropdownMenuController, DropdownMenuEntry,
};
use crate::ui::components::modal::modal_layer;
use crate::ui::components::{ui_button, ui_label, ui_static_label, ui_text_input};
use crate::ui::{fonts, theme};

pub fn run() {
    fonts::assert_required_fonts_available();

    let config = WindowConfig::default()
        .title("Dawn")
        .size(Size::new(theme::WINDOW_WIDTH, theme::WINDOW_HEIGHT))
        .resizable(true)
        .undecorated(true)
        .undecorated_shadow(true)
        .apply_default_theme(false);
    Application::new().window(app_view, Some(config)).run();
}

fn app_view(window_id: WindowId) -> impl IntoView {
    let model = Rc::new(RefCell::new(AppModel::default()));
    let snapshot = RwSignal::new(model.borrow().snapshot());
    let dropdown_menu = DropdownMenuController::new(theme::dropdown_menu_style());

    let dispatch = {
        let model = Rc::clone(&model);
        Rc::new(move |action: AppAction| {
            let before_revision = model.borrow().pending_persistence_revision();
            let result = model.borrow_mut().dispatch(action);
            if let Err(error) = result {
                model.borrow_mut().status = error;
            }
            let after_revision = model.borrow().pending_persistence_revision();
            snapshot.set(model.borrow().snapshot());
            if after_revision.is_some() && after_revision != before_revision {
                let timer_model = Rc::clone(&model);
                let timer_snapshot = snapshot;
                let revision = after_revision.expect("revision was checked");
                action::exec_after(Duration::from_millis(1500), move |_| {
                    let result = timer_model
                        .borrow_mut()
                        .dispatch(AppAction::FlushDeferredPersistence { revision });
                    if let Err(error) = result {
                        timer_model.borrow_mut().status = error;
                    }
                    timer_snapshot.set(timer_model.borrow().snapshot());
                });
            }
        }) as crate::ui::UiDispatch
    };

    let save_shortcut = Rc::clone(&dispatch);
    stack((
        v_stack((
            title_bar(
                window_id,
                snapshot,
                dropdown_menu.clone(),
                Rc::clone(&dispatch),
            ),
            crate::ui::workbench::workbench_view(
                snapshot,
                dropdown_menu.clone(),
                Rc::clone(&dispatch),
            ),
            status_bar(snapshot),
        ))
        .style(|s| s.size_full()),
        layout_fixture_import_modal(snapshot, Rc::clone(&dispatch)),
        layout_fixture_name_modal(snapshot, Rc::clone(&dispatch)),
        dropdown_menu_layer(dropdown_menu.clone()),
    ))
    .on_event(EventListener::KeyDown, move |event| {
        if let Event::KeyDown(event) = event {
            if event.modifiers.control()
                && !event.modifiers.shift()
                && !event.modifiers.alt()
                && !event.modifiers.meta()
                && matches!(&event.key.logical_key, Key::Character(key) if key.eq_ignore_ascii_case("s"))
            {
                save_shortcut(AppAction::SaveActiveFile);
                return EventPropagation::Stop;
            }
        }
        EventPropagation::Continue
    })
    .on_resize(move |rect| dropdown_menu.set_window_size(rect.size()))
    .style(theme::app_root_style)
}

fn title_bar(
    window_id: WindowId,
    snapshot: crate::ui::UiSnapshot,
    dropdown_menu: DropdownMenuController,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let close_dispatch = Rc::clone(&dispatch);

    h_stack((
        h_stack((
            ui_static_label("Dawn").style(|s| {
                s.font_size(theme::FONT_SMALL)
                    .font_bold()
                    .margin_right(theme::SPACE_10)
                    .padding_left(theme::SPACE_10)
            }),
            menu_tab(
                "File",
                dropdown_menu.clone(),
                file_menu(Rc::clone(&dispatch)),
            ),
            menu_tab(
                "Edit",
                dropdown_menu.clone(),
                edit_menu(Rc::clone(&dispatch)),
            ),
            menu_tab(
                "View",
                dropdown_menu.clone(),
                view_menu(snapshot, Rc::clone(&dispatch)),
            ),
            menu_tab("Run", dropdown_menu.clone(), run_menu(Rc::clone(&dispatch))),
            menu_tab("Help", dropdown_menu, help_menu(Rc::clone(&dispatch))),
        ))
        .style(|s| s.height_full().items_center().gap(theme::SPACE_2)),
        drag_window_area(empty()).style(|s| s.flex_grow(1.0).height_full().min_width(0.0)),
        h_stack((
            title_button(Icon::Minus).action(action::minimize_window),
            title_button(Icon::Square).action(action::toggle_window_maximized),
            title_button(Icon::X)
                .action(move || {
                    close_dispatch(AppAction::Quit);
                    close_window(window_id);
                })
                .style(|s| {
                    s.hover(|s| {
                        s.background(theme::color(theme::DANGER))
                            .color(theme::color(theme::TEXT_INVERTED))
                            .set(Foreground, Brush::Solid(theme::color(theme::TEXT_INVERTED)))
                    })
                }),
        ))
        .style(|s| s.height_full().items_center()),
    ))
    .style(move |s| {
        s.height(theme::TITLE_BAR_HEIGHT)
            .width_full()
            .items_center()
            .border_bottom(theme::BORDER_WIDTH)
            .border_color(theme::color(theme::BORDER))
            .background(theme::color(theme::PANEL_DARK))
    })
}

fn menu_tab(
    label_text: &'static str,
    dropdown_menu: DropdownMenuController,
    menu: impl Fn() -> Vec<DropdownMenuEntry> + 'static,
) -> impl IntoView {
    let tab = container(ui_static_label(label_text).style(|s| s.font_size(theme::FONT_SMALL)));
    let tab_id = tab.id();
    tab.on_event_stop(EventListener::PointerDown, move |event| {
        if let Event::PointerDown(event) = event {
            if event.button.is_primary() {
                dropdown_menu.open_below_view(tab_id, menu());
            }
        }
    })
    .style(|s| {
        s.height(theme::MENU_TAB_HEIGHT)
            .items_center()
            .padding_horiz(theme::SPACE_9)
            .background(theme::color(theme::PANEL_DARK))
            .hover(|s| s.background(theme::color(theme::SELECTED)))
    })
}

fn title_button(icon: Icon) -> floem::views::Button {
    ui_button(icon.style(|s| {
        s.size(theme::TITLE_BUTTON_ICON_SIZE, theme::TITLE_BUTTON_ICON_SIZE)
            .set(StrokeWidth, 1.8)
    }))
    .style(|s| {
        s.width(theme::TITLE_BUTTON_WIDTH)
            .height(theme::TITLE_BAR_HEIGHT)
            .padding_horiz(0.0)
            .padding_vert(0.0)
            .border(0.0)
            .border_radius(theme::SQUARE_RADIUS)
            .background(theme::color(theme::PANEL_DARK))
            .color(theme::color(theme::MUTED))
            .set(Foreground, Brush::Solid(theme::color(theme::MUTED)))
            .hover(|s| {
                s.background(theme::color(theme::SURFACE_CONTROL_HOVER))
                    .color(theme::color(theme::TEXT))
                    .set(Foreground, Brush::Solid(theme::color(theme::TEXT)))
            })
            .active(|s| {
                s.background(theme::color(theme::SURFACE_CONTROL_ACTIVE))
                    .color(theme::color(theme::TEXT_INVERTED))
                    .set(Foreground, Brush::Solid(theme::color(theme::TEXT_INVERTED)))
            })
    })
}

fn file_menu(dispatch: crate::ui::UiDispatch) -> impl Fn() -> Vec<DropdownMenuEntry> {
    move || {
        let open_project = Rc::clone(&dispatch);
        let close_project = Rc::clone(&dispatch);
        let new_project = Rc::clone(&dispatch);
        let save = Rc::clone(&dispatch);
        let settings = Rc::clone(&dispatch);

        vec![
            DropdownMenuEntry::item("Open Project", true, move || {
                let open_project = Rc::clone(&open_project);
                open_file(
                    FileDialogOptions::new()
                        .title("Open Dawn Project")
                        .select_directories(),
                    move |selection| {
                        if let Some(path) = selection.and_then(|info| info.path().first().cloned())
                        {
                            open_project(AppAction::OpenProject(path));
                        }
                    },
                );
            }),
            DropdownMenuEntry::item("Close Project", true, move || {
                close_project(AppAction::CloseProject);
            }),
            DropdownMenuEntry::item("New Project", true, move || {
                new_project(AppAction::NewProject);
            }),
            DropdownMenuEntry::separator(),
            DropdownMenuEntry::item("Save", true, move || {
                save(AppAction::SaveActiveFile);
            }),
            DropdownMenuEntry::item("Settings", true, move || {
                settings(AppAction::OpenSettings);
            }),
        ]
    }
}

fn edit_menu(dispatch: crate::ui::UiDispatch) -> impl Fn() -> Vec<DropdownMenuEntry> {
    move || {
        let check = Rc::clone(&dispatch);
        vec![DropdownMenuEntry::item("Check", true, move || {
            check(AppAction::Check);
        })]
    }
}

fn view_menu(
    snapshot: crate::ui::UiSnapshot,
    dispatch: crate::ui::UiDispatch,
) -> impl Fn() -> Vec<DropdownMenuEntry> {
    move || {
        let text = Rc::clone(&dispatch);
        let gui = Rc::clone(&dispatch);
        let toggle_project_tree = Rc::clone(&dispatch);
        let toggle_inspector = Rc::clone(&dispatch);
        let reset_layout = Rc::clone(&dispatch);
        let state = snapshot.get();
        let active_path = state.active_file.clone();
        let active_mode = state.active_buffer.as_ref().map(|buffer| buffer.view_mode);
        let has_gui = state.active_descriptor.as_ref().is_some_and(|descriptor| {
            descriptor.available_views.contains(&DocumentViewId::Layout)
                || descriptor
                    .available_views
                    .contains(&DocumentViewId::Fixture)
        });
        let text_path = active_path.clone();
        let gui_path = active_path.clone();

        vec![
            DropdownMenuEntry::item(
                "Text Editor",
                active_mode.is_some_and(|mode| mode != EditorViewMode::Text),
                move || {
                    if let Some(path) = text_path.clone() {
                        text(AppAction::SetEditorViewMode {
                            path,
                            mode: EditorViewMode::Text,
                        });
                    }
                },
            ),
            DropdownMenuEntry::item(
                "GUI Editor",
                has_gui && active_mode.is_some_and(|mode| mode != EditorViewMode::Gui),
                move || {
                    if let Some(path) = gui_path.clone() {
                        gui(AppAction::SetEditorViewMode {
                            path,
                            mode: EditorViewMode::Gui,
                        });
                    }
                },
            ),
            DropdownMenuEntry::separator(),
            DropdownMenuEntry::item("Toggle Project Tree", true, move || {
                toggle_project_tree(AppAction::ToggleProjectTree);
            }),
            DropdownMenuEntry::item("Toggle Inspector", true, move || {
                toggle_inspector(AppAction::ToggleInspector);
            }),
            DropdownMenuEntry::item("Reset Layout", true, move || {
                reset_layout(AppAction::ResetLayout);
            }),
        ]
    }
}

fn run_menu(dispatch: crate::ui::UiDispatch) -> impl Fn() -> Vec<DropdownMenuEntry> {
    move || {
        let play = Rc::clone(&dispatch);
        let pause = Rc::clone(&dispatch);
        let stop = Rc::clone(&dispatch);

        vec![
            DropdownMenuEntry::item("Play", true, move || {
                play(AppAction::Play);
            }),
            DropdownMenuEntry::item("Pause", true, move || {
                pause(AppAction::Pause);
            }),
            DropdownMenuEntry::item("Stop", true, move || {
                stop(AppAction::Stop);
            }),
        ]
    }
}

fn help_menu(dispatch: crate::ui::UiDispatch) -> impl Fn() -> Vec<DropdownMenuEntry> {
    move || {
        let about = Rc::clone(&dispatch);
        vec![DropdownMenuEntry::item("About Dawn", true, move || {
            about(AppAction::About);
        })]
    }
}

fn status_bar(snapshot: crate::ui::UiSnapshot) -> impl IntoView {
    h_stack((
        ui_label(move || snapshot.get().status),
        ui_label(move || {
            let snapshot = snapshot.get();
            match snapshot.analysis {
                Some(analysis) => format!(
                    "{} files  {} objects  {} diagnostics",
                    analysis.reachable_file_count(),
                    analysis.object_count(),
                    snapshot.diagnostics.len()
                ),
                None => "No analysis".to_string(),
            }
        }),
    ))
    .style(|s| {
        s.height(theme::STATUS_BAR_HEIGHT)
            .width_full()
            .items_center()
            .justify_between()
            .padding_horiz(theme::SPACE_10)
            .border_top(theme::BORDER_WIDTH)
            .border_color(theme::color(theme::BORDER))
            .background(theme::color(theme::STATUS_BAR))
            .font_size(theme::FONT_SMALL)
    })
}

fn layout_fixture_import_modal(
    snapshot: crate::ui::UiSnapshot,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let cancel = Rc::clone(&dispatch);
    modal_layer(
        move || snapshot.get().pending_layout_fixture_import,
        move || cancel(AppAction::CancelImportLayoutFixture),
        move |pending| import_modal_card(pending, Rc::clone(&dispatch)),
    )
}

fn layout_fixture_name_modal(
    snapshot: crate::ui::UiSnapshot,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let cancel = Rc::clone(&dispatch);
    modal_layer(
        move || snapshot.get().pending_layout_fixture_name,
        move || cancel(AppAction::CancelLayoutFixtureName),
        move |pending| fixture_name_modal_card(pending, Rc::clone(&dispatch)),
    )
}

fn fixture_name_modal_card(
    pending: crate::app_model::PendingLayoutFixtureName,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let name = RwSignal::new(pending.suggested_name);
    let confirm = Rc::clone(&dispatch);
    let cancel = Rc::clone(&dispatch);

    v_stack((
        ui_static_label("Fixture Name").style(|s| s.font_bold()),
        ui_static_label(pending.context).style(|s| {
            s.color(theme::color(theme::MUTED))
                .font_size(theme::FONT_SMALL)
        }),
        ui_text_input(name).style(|s| s.width_full()),
        h_stack((
            empty().style(|s| s.flex_grow(1.0).min_width(0.0)),
            ui_button("Cancel").action(move || {
                cancel(AppAction::CancelLayoutFixtureName);
            }),
            ui_button("Create").action(move || {
                confirm(AppAction::ConfirmLayoutFixtureName { name: name.get() });
            }),
        ))
        .style(|s| s.width_full().items_center().gap(theme::SPACE_8)),
    ))
    .style(|s| s.width_full().min_height(0.0).gap(theme::SPACE_10))
}

fn import_modal_card(
    pending: crate::app_model::PendingLayoutFixtureImport,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let cancel = Rc::clone(&dispatch);
    let rows = v_stack_from_iter(pending.fixtures.into_iter().map(move |fixture| {
        let import = Rc::clone(&dispatch);
        let object_key = fixture.object_key.clone();
        h_stack((
            v_stack((
                ui_static_label(fixture.object_key).style(|s| s.font_bold()),
                ui_static_label(format!("{}  {}", fixture.name, fixture.geometry_summary)).style(
                    |s| {
                        s.color(theme::color(theme::MUTED))
                            .font_size(theme::FONT_SMALL)
                    },
                ),
            ))
            .style(|s| s.flex_grow(1.0).min_width(0.0).gap(theme::SPACE_3)),
            ui_button("Import").action(move || {
                import(AppAction::ConfirmImportLayoutFixture {
                    object_key: object_key.clone(),
                });
            }),
        ))
        .style(|s| {
            s.width_full()
                .items_center()
                .gap(theme::SPACE_8)
                .padding_vert(theme::SPACE_8)
                .border_bottom(theme::BORDER_WIDTH)
                .border_color(theme::color(theme::BORDER))
        })
    }));

    v_stack((
        ui_static_label("Import Fixture").style(|s| s.font_bold()),
        scroll(rows).style(|s| s.width_full().max_height(340.0).min_height(0.0)),
        h_stack((
            empty().style(|s| s.flex_grow(1.0).min_width(0.0)),
            ui_button("Cancel").action(move || {
                cancel(AppAction::CancelImportLayoutFixture);
            }),
        ))
        .style(|s| s.width_full().items_center()),
    ))
    .style(|s| s.width_full().min_height(0.0).gap(theme::SPACE_10))
}
