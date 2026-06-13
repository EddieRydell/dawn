#![cfg_attr(not(windows), deny(unsafe_code))]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

use tauri::Manager;

pub mod audio;
pub mod bindings;
pub mod commands;
pub mod dto;
pub mod gui;
pub mod persistence;
pub mod preview;
pub mod sequence_clip_raster;
pub mod show_render;
pub mod state;

pub fn run() -> Result<(), tauri::Error> {
    let bindings = bindings::builder();

    tauri::Builder::default()
        .manage(state::DesktopState::new())
        .manage(preview::PreviewWindowService::new())
        .setup(|app| {
            let handle = app.handle().clone();
            let state = app.state::<state::DesktopState>();
            match state.persistence().load(&handle) {
                Ok(last_project) => {
                    if let Some(window_state) = state.persistence().main_window() {
                        if let Some(window) = app.get_window("main") {
                            persistence::apply_window_state(&window, &window_state);
                        }
                    }
                    if let Some(window) = app.get_window("main") {
                        let close_app = handle.clone();
                        window.on_window_event(move |event| {
                            if !matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                                return;
                            }
                            let state = close_app.state::<state::DesktopState>();
                            if let Some(main) = close_app
                                .get_window("main")
                                .and_then(|window| persistence::read_window_state(&window))
                            {
                                let _ = state.persistence().record_main_window(main);
                            }
                            let preview = close_app.state::<preview::PreviewWindowService>();
                            let _ = preview.close_for_main_shutdown(&close_app, state.persistence());
                        });
                    }
                    if let Some(project) = last_project {
                        state.open_project_path(&project);
                    }
                    if state.persistence().preview_window().open {
                        let preview = app.state::<preview::PreviewWindowService>();
                        let _ = preview.open_or_focus(handle, state.persistence().preview_window());
                    }
                }
                Err(error) => {
                    state.set_persistence_error(format!(
                        "Desktop state was not restored: {error}. Persistence is disabled until restart."
                    ));
                }
            }
            Ok(())
        })
        .invoke_handler(bindings.invoke_handler())
        .run(tauri::generate_context!())
}
