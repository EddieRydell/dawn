use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State};
use tauri_specta::{Builder, collect_commands};

use crate::desktop_state::DesktopState;
use crate::dto::{
    AppSettings, AppSnapshot, AudioTransportState, DocumentViewId, EditorViewMode, GuiDocument,
    GuiDocumentRequest, GuiEditCommand, GuiEditResult, NewSequenceRequest,
    OperatorRewriteResolution, OperatorRewriteValidation, ProjectSearchRequest,
    ProjectSearchResponse, SequenceAutomationMapping, SequenceAutomationTarget,
    SequenceClipRasterRequest, SequenceClipRasterResponse, SequenceClipRasterResultBatch,
    SequenceGuiEdit, SequenceSelectionEdit, SequenceSelectionEditResult, WorkspaceExplorerState,
    WorkspaceLayoutState, WorkspacePathChangePlan, WorkspacePathChangeRequest,
};
use crate::persistence::{
    PersistedEditorViewStateUpdate, PersistedPreviewWindowState,
    PersistedSequenceViewportStateUpdate, ProjectRestoreState,
};

mod app;
mod audio;
mod editor;
mod operator_rewrite;
mod output;
mod packages;
mod preview;
mod project;
mod sequence;
mod workspace;

pub(crate) use app::*;
pub(crate) use audio::*;
pub(crate) use editor::*;
pub(crate) use operator_rewrite::*;
pub(crate) use output::*;
pub(crate) use packages::*;
pub(crate) use preview::*;
pub(crate) use project::*;
pub(crate) use sequence::*;
pub(crate) use workspace::*;

pub(crate) fn register(builder: Builder<tauri::Wry>) -> Builder<tauri::Wry> {
    builder.commands(collect_commands![
        get_snapshot,
        sync_packages,
        check_package_updates,
        update_packages,
        remove_package_dependency,
        fork_package_dependency,
        open_package_page,
        update_app_settings,
        save_workspace_layout_state,
        save_workspace_explorer_state,
        search_project,
        plan_workspace_path_change,
        apply_workspace_path_change,
        get_restored_view_state,
        open_project_dialog,
        open_project,
        choose_new_project_parent_directory,
        create_new_project,
        create_sequence,
        open_file,
        close_file,
        set_active_file,
        update_active_text,
        autosave_active_text,
        set_editor_view_mode,
        save_editor_view_state,
        save_sequence_viewport_state,
        undo_active_edit,
        redo_active_edit,
        get_gui_document,
        request_sequence_clip_rasters,
        take_sequence_clip_raster_results,
        apply_gui_edit,
        finish_composition_graph_editing,
        rebind_detached_automation,
        discard_detached_automation,
        apply_sequence_selection_edit,
        choose_sequence_audio,
        flush_autosave,
        validate_operator_rewrite,
        apply_operator_rewrite,
        cancel_operator_rewrite,
        reload_active_buffer_from_disk,
        create_file,
        create_directory,
        delete_path,
        reload_project,
        toggle_project_tree,
        load_sequence_audio,
        unload_audio,
        audio_play,
        audio_pause,
        audio_stop,
        audio_rewind_to_zero,
        audio_seek,
        set_live_output_active,
        set_preview_window_open,
    ])
}

fn publish_audio_snapshot(app: &AppHandle, snapshot: AppSnapshot) -> AppSnapshot {
    let _ = app.emit("audio_transport_changed", snapshot.audio_transport.clone());
    snapshot
}

fn start_audio_transport_poll(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(50));
            let state = app.state::<DesktopState>();
            let snapshot = state.snapshot();
            let _ = app.emit("audio_transport_changed", snapshot.audio_transport.clone());
            if !matches!(snapshot.audio_transport.state, AudioTransportState::Playing) {
                break;
            }
        }
    });
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

fn audio_import_path(
    snapshot: &AppSnapshot,
    request: &GuiDocumentRequest,
    selected_path: &Path,
) -> Option<String> {
    if !matches!(request.view, DocumentViewId::Sequence) {
        return None;
    }
    let selected = selected_path.canonicalize().ok()?;
    let project_root = snapshot.project_root.as_deref().map(PathBuf::from)?;
    let document_path = project_root.join(&request.path);
    let document_dir = document_path.parent()?;
    let document_dir = document_dir.canonicalize().ok()?;
    relative_path(&document_dir, &selected).or_else(|| selected.to_str().map(ToString::to_string))
}

fn relative_path(from: &Path, to: &Path) -> Option<String> {
    let from_parts = path_parts(from)?;
    let to_parts = path_parts(to)?;
    if from_parts.prefix != to_parts.prefix || from_parts.rooted != to_parts.rooted {
        return None;
    }
    let common = from_parts
        .normal
        .iter()
        .zip(&to_parts.normal)
        .take_while(|(left, right)| left.eq_ignore_ascii_case(right))
        .count();
    let mut path = PathBuf::new();
    for _ in common..from_parts.normal.len() {
        path.push("..");
    }
    for part in &to_parts.normal[common..] {
        path.push(part);
    }
    path.to_str().map(|value| value.replace('\\', "/"))
}

struct PathParts {
    prefix: Option<String>,
    rooted: bool,
    normal: Vec<String>,
}

fn path_parts(path: &Path) -> Option<PathParts> {
    let mut prefix = None;
    let mut rooted = false;
    let mut normal = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(value) => {
                prefix = Some(value.as_os_str().to_str()?.to_string());
            }
            Component::RootDir => rooted = true,
            Component::CurDir => {}
            Component::ParentDir => normal.push("..".to_string()),
            Component::Normal(value) => normal.push(value.to_str()?.to_string()),
        }
    }
    Some(PathParts {
        prefix,
        rooted,
        normal,
    })
}
