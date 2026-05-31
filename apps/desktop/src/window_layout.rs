use dawn_app_core::layout_persistence::WindowLayout;
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Position, Size, WebviewWindow, WindowEvent,
};

use crate::state::{lock_model, AppState, CommandResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkbenchWindow {
    Main,
    Preview,
}

impl WorkbenchWindow {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Preview => "preview",
        }
    }
}

pub(crate) fn restore_main_window_layout(app: &AppHandle) -> CommandResult<()> {
    let window = app
        .get_webview_window(WorkbenchWindow::Main.label())
        .ok_or_else(|| "main window is not open".to_string())?;
    let state = app.state::<AppState>();
    let layout = lock_model(&state)?.workbench_layout.main_window.clone();
    apply_window_layout(&window, &layout)
}

pub(crate) fn register_main_window_layout_events(app: &AppHandle) -> CommandResult<()> {
    let window = app
        .get_webview_window(WorkbenchWindow::Main.label())
        .ok_or_else(|| "main window is not open".to_string())?;
    let app_for_event = app.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            persist_window_layout(&app_for_event, WorkbenchWindow::Main);
        }
        WindowEvent::CloseRequested { .. } => {
            let state = app_for_event.state::<AppState>();
            state.begin_shutdown();
            persist_window_layout(&app_for_event, WorkbenchWindow::Main);
            persist_window_layout(&app_for_event, WorkbenchWindow::Preview);
            if let Some(preview) =
                app_for_event.get_webview_window(WorkbenchWindow::Preview.label())
            {
                let _ = preview.close();
            }
        }
        _ => {}
    });
    Ok(())
}

pub(crate) fn persist_window_layout(app: &AppHandle, target: WorkbenchWindow) {
    let Some(window) = app.get_webview_window(target.label()) else {
        return;
    };
    let Ok(layout) = current_window_layout(&window) else {
        return;
    };
    let state = app.state::<AppState>();
    if let Ok(mut model) = lock_model(&state) {
        match target {
            WorkbenchWindow::Main => {
                let _ = model.set_main_window_layout(layout);
            }
            WorkbenchWindow::Preview => {
                let _ = model.set_preview_window_layout(layout);
            }
        }
    };
}

pub(crate) fn apply_window_layout(
    window: &WebviewWindow,
    layout: &WindowLayout,
) -> CommandResult<()> {
    window
        .set_position(Position::Physical(PhysicalPosition::new(
            checked_i32(layout.x, "window x")?,
            checked_i32(layout.y, "window y")?,
        )))
        .map_err(|error| error.to_string())?;
    window
        .set_size(Size::Physical(PhysicalSize::new(
            checked_u32(layout.width, "window width")?,
            checked_u32(layout.height, "window height")?,
        )))
        .map_err(|error| error.to_string())?;
    if layout.maximized {
        window.maximize().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn current_window_layout(window: &WebviewWindow) -> CommandResult<WindowLayout> {
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let size = window.inner_size().map_err(|error| error.to_string())?;
    let maximized = window.is_maximized().map_err(|error| error.to_string())?;
    Ok(WindowLayout {
        x: position.x.into(),
        y: position.y.into(),
        width: size.width.into(),
        height: size.height.into(),
        maximized,
    })
}

fn checked_i32(value: f64, name: &str) -> CommandResult<i32> {
    if value.is_finite() && value >= i32::MIN as f64 && value <= i32::MAX as f64 {
        Ok(value.round() as i32)
    } else {
        Err(format!("{name} must fit in i32"))
    }
}

fn checked_u32(value: f64, name: &str) -> CommandResult<u32> {
    if value.is_finite() && value > 0.0 && value <= u32::MAX as f64 {
        Ok(value.round() as u32)
    } else {
        Err(format!("{name} must be positive and fit in u32"))
    }
}
