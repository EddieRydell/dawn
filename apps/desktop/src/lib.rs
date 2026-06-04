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

mod app;
mod bindings;
mod commands;
mod preview;
mod project;
mod shell;

pub use bindings::{check_bindings, export_bindings, specta_builder};
pub use preview::{
    PreviewSceneDto, PreviewSceneFixtureDto, PreviewStateEventDto, PreviewTimingDto,
};
pub use preview::{
    SequenceEffectPreviewDto, SequenceEffectPreviewRequestEffectDto,
    SequenceEffectPreviewResultDto, SequenceEffectPreviewResultsDto,
};
use tauri::Manager;

pub fn run() -> Result<(), tauri::Error> {
    let builder = specta_builder();
    tauri::Builder::default()
        .manage(app::state::AppState::default())
        .invoke_handler(builder.invoke_handler())
        .setup(|app| {
            let _ = app.get_webview_window("main");
            shell::window_layout::restore_main_window_layout(app.handle())
                .map_err(std::io::Error::other)?;
            shell::window_layout::register_main_window_layout_events(app.handle())
                .map_err(std::io::Error::other)?;
            preview::start_preview_worker(app.handle().clone());
            let state = app.state::<app::state::AppState>();
            if let Ok(model) = app::state::lock_runtime(&state) {
                let root = model.project_root();
                drop(model);
                if let Ok(mut watcher) = app::state::lock_filesystem_watcher(&state) {
                    let _ = watcher.sync_project_root(app.handle(), root);
                }
            }
            preview::open_preview_window_on_startup(app.handle().clone(), state)
                .map_err(std::io::Error::other)?;
            Ok(())
        })
        .run(tauri::generate_context!())
}
