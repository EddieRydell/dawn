#![deny(clippy::disallowed_methods)]
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

mod app_runtime;
mod audio_runtime;
mod bindings;
mod commands;
mod effect_previews;
mod live_output;
mod preview;
mod preview_transport;
mod state;
mod window_layout;

pub use bindings::{check_bindings, export_bindings, specta_builder};
pub use effect_previews::{SequenceEffectPreviewBatchDto, SequenceEffectPreviewDto};
pub use preview::{
    PreviewSceneDto, PreviewSceneFixtureDto, PreviewStateEventDto, PreviewTimingDto,
};
use tauri::Manager;

pub fn run() -> Result<(), tauri::Error> {
    let builder = specta_builder();
    tauri::Builder::default()
        .manage(state::AppState::default())
        .invoke_handler(builder.invoke_handler())
        .setup(|app| {
            let _ = app.get_webview_window("main");
            window_layout::restore_main_window_layout(app.handle())
                .map_err(std::io::Error::other)?;
            window_layout::register_main_window_layout_events(app.handle())
                .map_err(std::io::Error::other)?;
            preview::start_preview_worker(app.handle().clone());
            let state = app.state::<state::AppState>();
            preview::open_preview_window_on_startup(app.handle().clone(), state)
                .map_err(std::io::Error::other)?;
            Ok(())
        })
        .run(tauri::generate_context!())
}
