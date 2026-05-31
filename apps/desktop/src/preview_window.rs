use dawn_app_core::layout_persistence::PreviewWindowLayout;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::state::{lock_model, AppState, CommandResult};

pub(crate) fn open_or_focus_preview_window(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    if let Some(window) = app.get_webview_window("preview") {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        return Ok(());
    }

    let layout = lock_model(&state)?.workbench_layout.preview_window.clone();
    let window =
        WebviewWindowBuilder::new(&app, "preview", WebviewUrl::App("/?view=preview".into()))
            .title("Dawn Preview")
            .position(layout.x, layout.y)
            .inner_size(layout.width, layout.height)
            .build()
            .map_err(|error| error.to_string())?;
    let app_for_event = app.clone();
    window.on_window_event(move |event| {
        if matches!(
            event,
            WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed
        ) {
            persist_preview_window_layout(&app_for_event);
        }
    });
    window.set_focus().map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn persist_preview_window_layout(app: &AppHandle) {
    let Some(window) = app.get_webview_window("preview") else {
        return;
    };
    let Ok(position) = window.outer_position() else {
        return;
    };
    let Ok(size) = window.inner_size() else {
        return;
    };
    let state = app.state::<AppState>();
    if let Ok(mut model) = lock_model(&state) {
        let _ = model.set_preview_window_layout(PreviewWindowLayout {
            x: position.x.into(),
            y: position.y.into(),
            width: size.width.into(),
            height: size.height.into(),
        });
    };
}
