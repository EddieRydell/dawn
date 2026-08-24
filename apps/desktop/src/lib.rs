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
mod desktop_foundation_tests;
pub mod desktop_state;
pub mod dto;
pub mod gui;
mod gui_geometry;
mod live_output;
pub mod persistence;
pub mod preview;
mod project_templates;
pub mod sequence_clip_raster;
mod sequence_integrity;
pub mod show_render;
mod state_tasks;

pub fn run() -> Result<(), tauri::Error> {
    let bindings = bindings::builder();

    tauri::Builder::default()
        .manage(desktop_state::DesktopState::new())
        .manage(preview::PreviewWindowService::new())
        .register_uri_scheme_protocol("dawn-raster", |context, request| {
            raster_protocol_response(
                context.app_handle().state::<desktop_state::DesktopState>().inner(),
                request,
            )
        })
        .setup(|app| {
            let handle = app.handle().clone();
                            let state = app.state::<desktop_state::DesktopState>();
            match state.persistence().load(&handle) {
                Ok(last_project) => {
                    let settings_snapshot = state.apply_persisted_settings();
                    if let Some(window_state) = state.persistence().main_window()
                        && let Some(window) = app.get_window("main")
                    {
                        persistence::apply_window_state(&window, &window_state);
                    }
                    if let Some(window) = app.get_window("main") {
                        let close_app = handle.clone();
                        window.on_window_event(move |event| {
                            if !matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                                return;
                            }
                            let state = close_app.state::<desktop_state::DesktopState>();
                            if let Some(main) = close_app
                                .get_window("main")
                                .and_then(|window| persistence::read_window_state(&window))
                            {
                                let _ = state.persistence().record_main_window(main);
                            }
                        let preview = close_app.state::<preview::PreviewWindowService>();
                            let _ = preview.close_for_main_shutdown(&close_app, state.persistence());
                            state.shutdown_live_output();
                        });
                    }
                    if settings_snapshot.settings.reopen_last_project
                        && let Some(project) = last_project
                    {
                        state.open_project_path(&project);
                    }
                    if settings_snapshot.settings.reopen_preview_window
                        && state.persistence().preview_window().open
                    {
                        let preview = app.state::<preview::PreviewWindowService>();
                        if preview
                            .open_or_focus(handle, state.persistence().preview_window())
                            .is_ok()
                        {
                            state.update_snapshot(|snapshot| {
                                snapshot.preview_open = true;
                                snapshot.preview_error = None;
                            });
                        }
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

fn raster_protocol_response(
    state: &desktop_state::DesktopState,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let token = request.uri().path().trim_start_matches('/');
    if token.is_empty() {
        return response_with_status(tauri::http::StatusCode::NOT_FOUND, Vec::new());
    }
    match state.sequence_clip_raster_pixels(token) {
        Some(bytes) => response_with_status(tauri::http::StatusCode::OK, bytes),
        None => response_with_status(tauri::http::StatusCode::NOT_FOUND, Vec::new()),
    }
}

fn response_with_status(
    status: tauri::http::StatusCode,
    body: Vec<u8>,
) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(status)
        .header(
            tauri::http::header::CONTENT_TYPE,
            "application/octet-stream",
        )
        .header(tauri::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}
