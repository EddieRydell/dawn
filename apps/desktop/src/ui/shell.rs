use std::cell::RefCell;
use std::rc::Rc;

use floem::file::FileDialogOptions;
use floem::file_action::open_file;
use floem::kurbo::Size;
use floem::menu::{Menu, MenuItem};
use floem::peniko::Color;
use floem::prelude::*;
use floem::window::{close_window, WindowConfig, WindowId};
use floem::{action, views::drag_window_area, Application};

use crate::actions::AppAction;
use crate::app_model::AppModel;
use crate::editor_session::EditorViewMode;
use crate::ui::theme;

pub fn run() {
    let config = WindowConfig::default()
        .title("Dawn")
        .size(Size::new(1240.0, 800.0))
        .resizable(true)
        .undecorated(true)
        .undecorated_shadow(true);
    Application::new().window(app_view, Some(config)).run();
}

fn app_view(window_id: WindowId) -> impl IntoView {
    let model = Rc::new(RefCell::new(AppModel::default()));
    let snapshot = RwSignal::new(model.borrow().snapshot());

    let dispatch = {
        let model = Rc::clone(&model);
        Rc::new(move |action: AppAction| {
            let result = model.borrow_mut().dispatch(action);
            if let Err(error) = result {
                model.borrow_mut().status = error;
            }
            snapshot.set(model.borrow().snapshot());
        }) as crate::ui::UiDispatch
    };

    v_stack((
        title_bar(window_id, snapshot, Rc::clone(&dispatch)),
        crate::ui::workbench::workbench_view(snapshot, Rc::clone(&dispatch)),
        status_bar(snapshot),
    ))
    .style(move |s| {
        s.size_full()
            .background(theme::color(theme::BACKGROUND))
            .font_family("Segoe UI".to_string())
            .color(theme::color(theme::TEXT))
    })
}

fn title_bar(
    window_id: WindowId,
    snapshot: crate::ui::UiSnapshot,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let close_dispatch = Rc::clone(&dispatch);

    h_stack((
        h_stack((
            static_label("Dawn").style(|s| {
                s.font_size(12.0)
                    .font_bold()
                    .margin_right(10.0)
                    .padding_left(10.0)
            }),
            menu_tab("File", file_menu(Rc::clone(&dispatch))),
            menu_tab("Edit", edit_menu(Rc::clone(&dispatch))),
            menu_tab("View", view_menu(snapshot, Rc::clone(&dispatch))),
            menu_tab("Run", run_menu(Rc::clone(&dispatch))),
            menu_tab("Help", help_menu(Rc::clone(&dispatch))),
        ))
        .style(|s| s.height_full().items_center().gap(2.0)),
        drag_window_area(
            label(move || {
                snapshot
                    .get()
                    .active_file
                    .map(|path| format!("Dawn - {}", path.to_slash_string()))
                    .unwrap_or_else(|| "Dawn".to_string())
            })
            .style(|s| {
                s.width_full()
                    .height_full()
                    .items_center()
                    .justify_center()
                    .font_size(12.0)
                    .color(theme::color(theme::MUTED))
            }),
        )
        .style(|s| s.flex_grow(1.0).height_full().min_width(0.0)),
        h_stack((
            title_button("_").action(action::minimize_window),
            title_button("[]").action(action::toggle_window_maximized),
            title_button("X")
                .action(move || {
                    close_dispatch(AppAction::CloseProject);
                    close_window(window_id);
                })
                .style(|s| {
                    s.hover(|s| {
                        s.background(theme::color(theme::DANGER))
                            .color(Color::WHITE)
                    })
                }),
        ))
        .style(|s| s.height_full().items_center()),
    ))
    .style(move |s| {
        s.height(32.0)
            .width_full()
            .items_center()
            .border_bottom(1.0)
            .border_color(theme::color(theme::BORDER))
            .background(theme::color(theme::PANEL_DARK))
    })
}

fn menu_tab(label_text: &'static str, menu: impl Fn() -> Menu + 'static) -> impl IntoView {
    container(static_label(label_text))
        .style(|s| {
            s.height(24.0)
                .items_center()
                .padding_horiz(9.0)
                .background(theme::color(theme::PANEL_DARK))
                .hover(|s| s.background(theme::color(theme::SELECTED)))
        })
        .popout_menu(menu)
}

fn title_button(label: &'static str) -> floem::views::Button {
    button(label).style(|s| {
        s.width(42.0)
            .height(32.0)
            .border_radius(0.0)
            .background(theme::color(theme::PANEL_DARK))
            .hover(|s| s.background(theme::color(theme::SELECTED)))
    })
}

fn file_menu(dispatch: crate::ui::UiDispatch) -> impl Fn() -> Menu {
    move || {
        let open_project = Rc::clone(&dispatch);
        let close_project = Rc::clone(&dispatch);
        let new_project = Rc::clone(&dispatch);
        let save = Rc::clone(&dispatch);
        let settings = Rc::clone(&dispatch);

        Menu::new("File")
            .entry(MenuItem::new("Open Project").action(move || {
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
            }))
            .entry(MenuItem::new("Close Project").action(move || {
                close_project(AppAction::CloseProject);
            }))
            .entry(MenuItem::new("New Project").action(move || {
                new_project(AppAction::NewProject);
            }))
            .separator()
            .entry(MenuItem::new("Save").action(move || {
                save(AppAction::SaveActiveFile);
            }))
            .entry(MenuItem::new("Settings").action(move || {
                settings(AppAction::OpenSettings);
            }))
    }
}

fn edit_menu(dispatch: crate::ui::UiDispatch) -> impl Fn() -> Menu {
    move || {
        let check = Rc::clone(&dispatch);
        Menu::new("Edit").entry(MenuItem::new("Check").action(move || {
            check(AppAction::Check);
        }))
    }
}

fn view_menu(
    snapshot: crate::ui::UiSnapshot,
    dispatch: crate::ui::UiDispatch,
) -> impl Fn() -> Menu {
    move || {
        let gui = Rc::clone(&dispatch);
        let text = Rc::clone(&dispatch);
        let toggle_left = Rc::clone(&dispatch);
        let toggle_right = Rc::clone(&dispatch);
        let reset_layout = Rc::clone(&dispatch);

        Menu::new("View")
            .entry(MenuItem::new("GUI").action(move || {
                if let Some(path) = snapshot.get().active_file {
                    gui(AppAction::SetEditorViewMode {
                        path,
                        mode: EditorViewMode::Gui,
                    });
                }
            }))
            .entry(MenuItem::new("Text").action(move || {
                if let Some(path) = snapshot.get().active_file {
                    text(AppAction::SetEditorViewMode {
                        path,
                        mode: EditorViewMode::Text,
                    });
                }
            }))
            .separator()
            .entry(MenuItem::new("Toggle Left Pane").action(move || {
                toggle_left(AppAction::ToggleLeftPane);
            }))
            .entry(MenuItem::new("Toggle Right Pane").action(move || {
                toggle_right(AppAction::ToggleRightPane);
            }))
            .entry(MenuItem::new("Reset Layout").action(move || {
                reset_layout(AppAction::ResetLayout);
            }))
    }
}

fn run_menu(dispatch: crate::ui::UiDispatch) -> impl Fn() -> Menu {
    move || {
        let play = Rc::clone(&dispatch);
        let pause = Rc::clone(&dispatch);
        let stop = Rc::clone(&dispatch);

        Menu::new("Run")
            .entry(MenuItem::new("Play").action(move || {
                play(AppAction::Play);
            }))
            .entry(MenuItem::new("Pause").action(move || {
                pause(AppAction::Pause);
            }))
            .entry(MenuItem::new("Stop").action(move || {
                stop(AppAction::Stop);
            }))
    }
}

fn help_menu(dispatch: crate::ui::UiDispatch) -> impl Fn() -> Menu {
    move || {
        let about = Rc::clone(&dispatch);
        Menu::new("Help").entry(MenuItem::new("About Dawn").action(move || {
            about(AppAction::About);
        }))
    }
}

fn status_bar(snapshot: crate::ui::UiSnapshot) -> impl IntoView {
    h_stack((
        label(move || snapshot.get().status),
        label(move || {
            let snapshot = snapshot.get();
            match snapshot.analysis {
                Some(analysis) => format!(
                    "{} files  {} objects  {} diagnostics",
                    analysis.reachable_file_count,
                    analysis.object_count,
                    snapshot.diagnostics.len()
                ),
                None => "No analysis".to_string(),
            }
        }),
    ))
    .style(|s| {
        s.height(26.0)
            .width_full()
            .items_center()
            .justify_between()
            .padding_horiz(10.0)
            .border_top(1.0)
            .border_color(theme::color(theme::BORDER))
            .background(Color::rgb8(238, 238, 236))
            .font_size(12.0)
    })
}
