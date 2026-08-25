use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn load_sequence_audio(
    request: GuiDocumentRequest,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    publish_audio_snapshot(&app, state.load_sequence_audio(request))
}

#[tauri::command]
#[specta::specta]
pub(crate) fn unload_audio(app: AppHandle, state: State<'_, DesktopState>) -> AppSnapshot {
    publish_audio_snapshot(&app, state.unload_audio())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn audio_play(app: AppHandle, state: State<'_, DesktopState>) -> AppSnapshot {
    let snapshot = publish_audio_snapshot(&app, state.audio_play());
    if matches!(snapshot.audio_transport.state, AudioTransportState::Playing) {
        start_audio_transport_poll(app);
    }
    snapshot
}

#[tauri::command]
#[specta::specta]
pub(crate) fn audio_pause(app: AppHandle, state: State<'_, DesktopState>) -> AppSnapshot {
    publish_audio_snapshot(&app, state.audio_pause())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn audio_stop(app: AppHandle, state: State<'_, DesktopState>) -> AppSnapshot {
    publish_audio_snapshot(&app, state.audio_stop())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn audio_rewind_to_zero(app: AppHandle, state: State<'_, DesktopState>) -> AppSnapshot {
    publish_audio_snapshot(&app, state.audio_rewind_to_zero())
}

#[tauri::command]
#[specta::specta]
pub(crate) fn audio_seek(
    position_seconds: f64,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    publish_audio_snapshot(&app, state.audio_seek(position_seconds))
}
