use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn set_live_output_active(
    active: bool,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    let snapshot = state.set_live_output_active(active);
    let _ = app.emit("live_output_changed", snapshot.live_output.clone());
    if active {
        start_live_output_poll(app);
    }
    snapshot
}

fn start_live_output_poll(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(50));
            let state = app.state::<DesktopState>();
            let snapshot = state.snapshot();
            let _ = app.emit("live_output_changed", snapshot.live_output.clone());
            if matches!(
                snapshot.live_output.state,
                crate::dto::LiveOutputState::Disabled | crate::dto::LiveOutputState::Error
            ) {
                break;
            }
        }
    });
}
