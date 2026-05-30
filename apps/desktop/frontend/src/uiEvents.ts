export const OPEN_ACTIVE_EDITOR_DIAGNOSTICS_EVENT = "dawn:open-active-editor-diagnostics";

export function openActiveEditorDiagnostics() {
  window.dispatchEvent(new Event(OPEN_ACTIVE_EDITOR_DIAGNOSTICS_EVENT));
}
