use tauri::{Runtime, Window};

use super::model::PersistedWindowState;

pub fn read_window_state<R: Runtime>(window: &Window<R>) -> Option<PersistedWindowState> {
    let position = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    Some(PersistedWindowState {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
        maximized: window.is_maximized().unwrap_or(false),
    })
}

pub fn apply_window_state<R: Runtime>(window: &Window<R>, state: &PersistedWindowState) {
    use tauri::{LogicalSize, PhysicalPosition};
    let _ = window.set_size(LogicalSize::new(state.width as f64, state.height as f64));
    let _ = window.set_position(PhysicalPosition::new(state.x, state.y));
    if state.maximized {
        let _ = window.maximize();
    }
}
