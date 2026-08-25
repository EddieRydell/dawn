use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn validate_operator_rewrite(
    token: u32,
    resolution: OperatorRewriteResolution,
    state: State<'_, DesktopState>,
) -> OperatorRewriteValidation {
    state.validate_operator_rewrite(token, resolution)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn apply_operator_rewrite(
    token: u32,
    resolution: OperatorRewriteResolution,
    state: State<'_, DesktopState>,
) -> AppSnapshot {
    state.apply_operator_rewrite(token, resolution)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn cancel_operator_rewrite(token: u32, state: State<'_, DesktopState>) -> AppSnapshot {
    state.cancel_operator_rewrite(token)
}
