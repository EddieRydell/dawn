use super::*;

#[tauri::command]
#[specta::specta]
pub(crate) fn get_gui_document(
    request: GuiDocumentRequest,
    state: State<'_, DesktopState>,
) -> GuiDocument {
    state.get_gui_document(request)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn request_sequence_clip_rasters(
    request: SequenceClipRasterRequest,
    state: State<'_, DesktopState>,
) -> SequenceClipRasterResponse {
    state.request_sequence_clip_rasters(request)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn take_sequence_clip_raster_results(
    request: GuiDocumentRequest,
    request_id: u32,
    state: State<'_, DesktopState>,
) -> SequenceClipRasterResultBatch {
    state.take_sequence_clip_raster_results(request, request_id)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn apply_gui_edit(
    request: GuiDocumentRequest,
    edit: GuiEditCommand,
    state: State<'_, DesktopState>,
) -> GuiEditResult {
    state.apply_gui_edit(request, edit)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn finish_composition_graph_editing(state: State<'_, DesktopState>) -> AppSnapshot {
    state.finish_composition_graph_editing()
}

#[tauri::command]
#[specta::specta]
pub(crate) fn rebind_detached_automation(
    request: GuiDocumentRequest,
    clip_id: u32,
    detached_index: u32,
    target: SequenceAutomationTarget,
    mapping: SequenceAutomationMapping,
    state: State<'_, DesktopState>,
) -> GuiEditResult {
    state.apply_gui_edit(
        request,
        GuiEditCommand::Sequence {
            edit: SequenceGuiEdit::RebindDetachedAutomation {
                clip_id,
                detached_index,
                target,
                mapping,
            },
        },
    )
}

#[tauri::command]
#[specta::specta]
pub(crate) fn discard_detached_automation(
    request: GuiDocumentRequest,
    clip_id: u32,
    detached_index: u32,
    state: State<'_, DesktopState>,
) -> GuiEditResult {
    state.apply_gui_edit(
        request,
        GuiEditCommand::Sequence {
            edit: SequenceGuiEdit::DiscardDetachedAutomation {
                clip_id,
                detached_index,
            },
        },
    )
}

#[tauri::command]
#[specta::specta]
pub(crate) fn apply_sequence_selection_edit(
    edit: SequenceSelectionEdit,
    state: State<'_, DesktopState>,
) -> SequenceSelectionEditResult {
    state.apply_sequence_selection_edit(edit)
}

#[tauri::command]
#[specta::specta]
pub(crate) fn choose_sequence_audio(
    request: GuiDocumentRequest,
    state: State<'_, DesktopState>,
) -> GuiEditResult {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Audio", &["mp3", "wav", "ogg", "flac"])
        .pick_file()
    else {
        return GuiEditResult {
            snapshot: state.snapshot(),
            document: state.get_gui_document(request),
        };
    };
    let snapshot = state.snapshot();
    let import_path = match super::import_external_audio(&snapshot, &path) {
        Ok(path) => path,
        Err(message) => {
            let snapshot = state.update_snapshot(|snapshot| {
                snapshot.status = message;
            });
            return GuiEditResult {
                snapshot,
                document: state.get_gui_document(request),
            };
        }
    };
    if !matches!(request.view, DocumentViewId::Sequence) {
        let snapshot = state.update_snapshot(|snapshot| {
            snapshot.status = "Audio can only be associated with a sequence.".to_string();
        });
        return GuiEditResult {
            snapshot,
            document: state.get_gui_document(request),
        };
    }
    state.apply_gui_edit(
        request,
        GuiEditCommand::Sequence {
            edit: SequenceGuiEdit::SetAudio {
                import_path: Some(import_path),
            },
        },
    )
}
