#![deny(clippy::disallowed_methods)]
#![deny(unsafe_code)]
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

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use dawn_app_core::actions::AppAction;
use dawn_app_core::app_model::{AppModel, DispatchOutcome};
use dawn_app_core::dto::{
    AppSnapshotDto, EditorViewModeDto, FixtureGuiEditDto, LayoutGuiEditDto, SequenceGuiEditDto,
};
use dawn_app_core::output_runtime::runtime_params_from_document;
use dawn_project::document::{SequenceEffectDocument, SequenceEffectPixelDocument};
use dawn_project::effect_script::{FixtureContext, PixelContext};
use dawn_project::path::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter, Manager, State};

pub struct AppState {
    model: Mutex<AppModel>,
    effect_preview_cache: Mutex<HashMap<EffectPreviewCacheKey, SequenceEffectPreviewDto>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            model: Mutex::new(AppModel::default()),
            effect_preview_cache: Mutex::new(HashMap::new()),
        }
    }
}

type CommandResult<T> = Result<T, String>;
const PREVIEW_MAX_COLUMNS: usize = 360;
const PREVIEW_MAX_ROWS: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectPreviewBatchDto {
    pub previews: Vec<SequenceEffectPreviewDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceEffectPreviewDto {
    pub effect_id: u32,
    pub duration_ms: u32,
    pub source_pixel_count: u32,
    pub sampled_pixel_indices: Vec<u32>,
    pub columns: u32,
    pub rows: u32,
    pub colors: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EffectPreviewCacheKey {
    sequence_path: String,
    object_key: String,
    effect_id: u32,
    duration_ms: u64,
    frame_rate: u32,
    script_key: String,
    script_source: String,
    params_json: String,
    target_pixels_json: String,
    sampled_pixel_indices: Vec<usize>,
    max_columns: usize,
    max_rows: usize,
}

#[specta::specta]
#[tauri::command]
fn get_snapshot(state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    Ok(lock_model(&state)?.snapshot_dto())
}

#[specta::specta]
#[tauri::command]
fn open_project_dialog(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<AppSnapshotDto> {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Open Dawn Project")
        .pick_folder()
    else {
        return get_snapshot(state);
    };
    dispatch(&app, &state, AppAction::OpenProject(path))
}

#[specta::specta]
#[tauri::command]
fn open_project(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::OpenProject(PathBuf::from(path)))
}

#[specta::specta]
#[tauri::command]
fn open_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::OpenFile(project_path(path)))
}

#[specta::specta]
#[tauri::command]
fn close_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::CloseFile(project_path(path)))
}

#[specta::specta]
#[tauri::command]
fn set_active_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::SetActiveFile(project_path(path)))
}

#[specta::specta]
#[tauri::command]
fn update_active_text(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::UpdateActiveText(text))
}

#[specta::specta]
#[tauri::command]
fn set_active_view_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    mode: EditorViewModeDto,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::SetActiveViewMode(mode))
}

#[specta::specta]
#[tauri::command]
fn undo_active_edit(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::UndoActiveEdit)
}

#[specta::specta]
#[tauri::command]
fn redo_active_edit(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::RedoActiveEdit)
}

#[specta::specta]
#[tauri::command]
fn apply_sequence_gui_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    edit: SequenceGuiEditDto,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::ApplySequenceGuiEdit(edit))
}

#[specta::specta]
#[tauri::command]
fn get_sequence_effect_previews(
    state: State<'_, AppState>,
    path: String,
    object_key: String,
    effect_ids: Vec<u32>,
) -> CommandResult<SequenceEffectPreviewBatchDto> {
    let model = lock_model(&state)?;
    let analysis = model
        .analysis
        .as_ref()
        .ok_or_else(|| "project analysis is not available".to_string())?
        .clone();
    let document = model.workspace.sequence_document(
        project_path(path),
        &object_key,
        model.editors.dirty_overlays(),
    )?;
    drop(model);

    let requested = effect_ids
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let mut previews = Vec::new();
    for effect in document
        .effects
        .iter()
        .filter(|effect| requested.contains(&effect.id))
    {
        if let Some(preview) = preview_for_effect(
            &state,
            &analysis,
            &document.path,
            &document.object_key,
            document.frame_rate,
            effect,
        )? {
            previews.push(preview);
        }
    }

    Ok(SequenceEffectPreviewBatchDto { previews })
}

#[specta::specta]
#[tauri::command]
fn apply_layout_gui_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    edit: LayoutGuiEditDto,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::ApplyLayoutGuiEdit(edit))
}

#[specta::specta]
#[tauri::command]
fn apply_fixture_gui_edit(
    app: AppHandle,
    state: State<'_, AppState>,
    edit: FixtureGuiEditDto,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::ApplyFixtureGuiEdit(edit))
}

#[specta::specta]
#[tauri::command]
fn flush_autosave(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::FlushAutosave)
}

#[specta::specta]
#[tauri::command]
fn create_file(
    app: AppHandle,
    state: State<'_, AppState>,
    parent: String,
    name: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(
        &app,
        &state,
        AppAction::CreateFile {
            parent: project_path(parent),
            name,
        },
    )
}

#[specta::specta]
#[tauri::command]
fn create_directory(
    app: AppHandle,
    state: State<'_, AppState>,
    parent: String,
    name: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(
        &app,
        &state,
        AppAction::CreateDirectory {
            parent: project_path(parent),
            name,
        },
    )
}

#[specta::specta]
#[tauri::command]
fn rename_path(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    new_name: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(
        &app,
        &state,
        AppAction::RenamePath {
            path: project_path(path),
            new_name,
        },
    )
}

#[specta::specta]
#[tauri::command]
fn delete_path(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::DeletePath(project_path(path)))
}

#[specta::specta]
#[tauri::command]
fn reload_project(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::ReloadProject)
}

#[specta::specta]
#[tauri::command]
fn toggle_project_tree(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::ToggleProjectTree)
}

#[specta::specta]
#[tauri::command]
fn preview_play(state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    Ok(lock_model(&state)?.snapshot_dto())
}

#[specta::specta]
#[tauri::command]
fn preview_pause(state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    Ok(lock_model(&state)?.snapshot_dto())
}

#[specta::specta]
#[tauri::command]
fn preview_stop(state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    Ok(lock_model(&state)?.snapshot_dto())
}

const BINDINGS_PATH: &str = "apps/desktop/frontend/src/bindings.ts";
const TYPED_ERROR_IMPL: &str = r#"async function typedError<T, E>(result: Promise<T>): Promise<{ status: "ok"; data: T } | { status: "error"; error: E }> {
    try {
        return { status: "ok", data: await result };
    } catch (error: unknown) {
        if (error instanceof Error) throw error;
        return { status: "error", error: error as E };
    }
}"#;

pub fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .typed_error_impl(TYPED_ERROR_IMPL)
        .commands(tauri_specta::collect_commands![
            get_snapshot,
            open_project_dialog,
            open_project,
            open_file,
            close_file,
            set_active_file,
            update_active_text,
            set_active_view_mode,
            undo_active_edit,
            redo_active_edit,
            apply_sequence_gui_edit,
            get_sequence_effect_previews,
            apply_layout_gui_edit,
            apply_fixture_gui_edit,
            flush_autosave,
            create_file,
            create_directory,
            rename_path,
            delete_path,
            reload_project,
            toggle_project_tree,
            preview_play,
            preview_pause,
            preview_stop
        ])
}

pub fn export_bindings() -> Result<(), Box<dyn std::error::Error>> {
    specta_builder().export(specta_typescript::Typescript::default(), BINDINGS_PATH)?;
    normalize_bindings_assertion(BINDINGS_PATH)?;
    Ok(())
}

pub fn check_bindings() -> Result<(), Box<dyn std::error::Error>> {
    let mut check_path = std::env::temp_dir();
    check_path.push(format!("dawn-bindings-check-{}.ts", std::process::id()));
    specta_builder().export(specta_typescript::Typescript::default(), &check_path)?;
    normalize_bindings_assertion(&check_path)?;

    let generated = std::fs::read_to_string(&check_path)?;
    std::fs::remove_file(&check_path)?;
    let current = std::fs::read_to_string(BINDINGS_PATH)?;
    if generated != current {
        return Err("generated bindings are stale; run `pnpm generate-bindings`".into());
    }
    Ok(())
}

fn normalize_bindings_assertion(
    path: impl AsRef<std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path)?;
    let normalized = source.replace(
        "const _assertTypedErrorFollowsContract: <T, E>(result: Promise<T>) => Promise<any> = typedError;",
        "void (typedError satisfies <T, E>(result: Promise<T>) => Promise<{ status: \"ok\"; data: T } | { status: \"error\"; error: E }>);",
    );
    if normalized != source {
        std::fs::write(path, normalized)?;
    }
    Ok(())
}

pub fn run() {
    let builder = specta_builder();
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(builder.invoke_handler())
        .setup(|app| {
            let _ = app.get_webview_window("main");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run Dawn desktop");
}

fn dispatch(
    app: &AppHandle,
    state: &State<'_, AppState>,
    action: AppAction,
) -> CommandResult<AppSnapshotDto> {
    let mut model = lock_model(state)?;
    let outcome = model.dispatch(action)?;
    let snapshot = model.snapshot_dto();
    if outcome == DispatchOutcome::SnapshotChanged {
        app.emit("app_snapshot_changed", &snapshot)
            .map_err(|error| error.to_string())?;
    }
    Ok(snapshot)
}

fn lock_model<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<std::sync::MutexGuard<'a, AppModel>> {
    state
        .model
        .lock()
        .map_err(|_| "application state lock is poisoned".to_string())
}

fn project_path(path: String) -> Utf8PathBuf {
    Utf8PathBuf::from(path)
}

fn lock_effect_preview_cache<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<
    std::sync::MutexGuard<'a, HashMap<EffectPreviewCacheKey, SequenceEffectPreviewDto>>,
> {
    state
        .effect_preview_cache
        .lock()
        .map_err(|_| "effect preview cache lock is poisoned".to_string())
}

fn preview_for_effect(
    state: &State<'_, AppState>,
    analysis: &dawn_project::analysis::ProjectAnalysis,
    sequence_path: &str,
    object_key: &str,
    frame_rate: u32,
    effect: &SequenceEffectDocument,
) -> CommandResult<Option<SequenceEffectPreviewDto>> {
    let Some(render) = &effect.render else {
        return Ok(None);
    };
    if frame_rate == 0 || effect.duration_ms == 0 || render.target_pixels.is_empty() {
        return Ok(None);
    }

    let source_pixel_count = render.target_pixels.len();
    let sampled_pixel_indices = evenly_sample_indices(source_pixel_count, PREVIEW_MAX_ROWS);
    let cache_key = EffectPreviewCacheKey {
        sequence_path: sequence_path.to_string(),
        object_key: object_key.to_string(),
        effect_id: effect.id,
        duration_ms: effect.duration_ms,
        frame_rate,
        script_key: render.script_key.clone(),
        script_source: render.script_source.clone(),
        params_json: serde_json::to_string(&render.params).map_err(|error| error.to_string())?,
        target_pixels_json: serde_json::to_string(&render.target_pixels)
            .map_err(|error| error.to_string())?,
        sampled_pixel_indices: sampled_pixel_indices.clone(),
        max_columns: PREVIEW_MAX_COLUMNS,
        max_rows: PREVIEW_MAX_ROWS,
    };
    if let Some(preview) = lock_effect_preview_cache(state)?.get(&cache_key).cloned() {
        return Ok(Some(preview));
    }

    let total_frames = total_preview_frames(effect.duration_ms, frame_rate);
    let sampled_frame_indices = evenly_sample_indices(total_frames, PREVIEW_MAX_COLUMNS);
    let params = runtime_params_from_document(&render.params);
    let mut colors = Vec::with_capacity(sampled_frame_indices.len() * sampled_pixel_indices.len());

    for pixel_index in &sampled_pixel_indices {
        let Some(pixel) = render.target_pixels.get(*pixel_index) else {
            return Ok(None);
        };
        for frame_index in &sampled_frame_indices {
            let local_ms = local_ms_for_frame(*frame_index, frame_rate, effect.duration_ms);
            let progress = (local_ms as f64 / effect.duration_ms as f64).clamp(0.0, 1.0);
            let color = match sample_preview_pixel(
                analysis,
                &render.script_key,
                pixel,
                progress,
                local_ms as f64 / 1_000.0,
                &params,
            ) {
                Ok(color) => color,
                Err(_) => return Ok(None),
            };
            colors.push(pack_rgb(color));
        }
    }

    let preview = SequenceEffectPreviewDto {
        effect_id: effect.id,
        duration_ms: effect.duration_ms.min(u32::MAX as u64) as u32,
        source_pixel_count: source_pixel_count.min(u32::MAX as usize) as u32,
        sampled_pixel_indices: sampled_pixel_indices
            .iter()
            .map(|index| (*index).min(u32::MAX as usize) as u32)
            .collect(),
        columns: sampled_frame_indices.len().min(u32::MAX as usize) as u32,
        rows: sampled_pixel_indices.len().min(u32::MAX as usize) as u32,
        colors,
    };
    lock_effect_preview_cache(state)?.insert(cache_key, preview.clone());
    Ok(Some(preview))
}

fn sample_preview_pixel(
    analysis: &dawn_project::analysis::ProjectAnalysis,
    script_key: &str,
    pixel: &SequenceEffectPixelDocument,
    progress: f64,
    seconds: f64,
    params: &std::collections::BTreeMap<String, dawn_project::effect_script::RuntimeValue>,
) -> Result<dawn_project::model::Color, String> {
    analysis.sample_effect_script_key(
        script_key,
        progress,
        seconds,
        FixtureContext {
            index: pixel.fixture_index,
        },
        PixelContext {
            index: pixel.pixel_index,
        },
        params.clone(),
    )
}

fn total_preview_frames(duration_ms: u64, frame_rate: u32) -> usize {
    let frames = ((duration_ms as u128) * (frame_rate as u128)).div_ceil(1_000);
    frames.max(1).min(usize::MAX as u128) as usize
}

fn local_ms_for_frame(frame_index: usize, frame_rate: u32, duration_ms: u64) -> u64 {
    let local_ms = ((frame_index as u128) * 1_000) / frame_rate as u128;
    (local_ms as u64).min(duration_ms.saturating_sub(1))
}

fn evenly_sample_indices(source_count: usize, max_count: usize) -> Vec<usize> {
    if source_count == 0 || max_count == 0 {
        return Vec::new();
    }
    let count = source_count.min(max_count);
    if count == 1 {
        return vec![0];
    }
    (0..count)
        .map(|index| {
            ((index as f64) * ((source_count - 1) as f64) / ((count - 1) as f64)).round() as usize
        })
        .collect()
}

fn pack_rgb(color: dawn_project::model::Color) -> u32 {
    ((color.red as u32) << 16) | ((color.green as u32) << 8) | color.blue as u32
}
