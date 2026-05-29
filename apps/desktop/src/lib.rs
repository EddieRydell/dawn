#![deny(clippy::disallowed_methods)]
#![cfg_attr(not(windows), deny(unsafe_code))]
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
use std::thread;
use std::time::{Duration, Instant};

use dawn_app_core::actions::AppAction;
use dawn_app_core::app_model::{AppModel, DispatchOutcome};
use dawn_app_core::dto::{
    AppSnapshotDto, EditorViewModeDto, FixtureGuiEditDto, GeometryRenderBoundsDto,
    GeometryRenderPointDto, LayoutGuiEditDto, SequenceGuiEditDto,
};
use dawn_app_core::layout_persistence::PreviewWindowLayout;
use dawn_app_core::output_runtime::runtime_params_from_document;
use dawn_app_core::output_runtime::OutputFrame;
use dawn_app_core::preview_session::PreviewSnapshot;
use dawn_project::document::{SequenceEffectDocument, SequenceEffectPixelDocument};
use dawn_project::effect_script::{FixtureContext, PixelContext};
use dawn_project::path::{serialized_import_path, utf8_path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};

mod preview_transport;

use preview_transport::{PreviewTransportMode, PreviewTransportRuntime};

pub struct AppState {
    model: Mutex<AppModel>,
    effect_preview_cache: Mutex<HashMap<EffectPreviewCacheKey, SequenceEffectPreviewDto>>,
    preview_transport: Mutex<PreviewTransportRuntime>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            model: Mutex::new(AppModel::default()),
            effect_preview_cache: Mutex::new(HashMap::new()),
            preview_transport: Mutex::new(PreviewTransportRuntime::default()),
        }
    }
}

type CommandResult<T> = Result<T, String>;
const PREVIEW_MAX_COLUMNS: usize = 360;
const PREVIEW_MAX_ROWS: usize = 50;
const PREVIEW_STATE_EVENT_INTERVAL: Duration = Duration::from_millis(33);

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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewStateEventDto {
    pub source_label: String,
    pub is_playing: bool,
    pub position_ms: u32,
    pub home_ms: u32,
    pub duration_ms: u32,
    pub audio: Option<dawn_app_core::dto::SequenceAudioDto>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSceneDto {
    pub generation: u32,
    pub source_label: String,
    pub bounds: GeometryRenderBoundsDto,
    pub pixel_count: u32,
    pub fixtures: Vec<PreviewSceneFixtureDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSceneFixtureDto {
    pub id: u32,
    pub name: String,
    pub bulb_radius: f64,
    pub first_pixel_index: u32,
    pub pixels: Vec<GeometryRenderPointDto>,
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
fn choose_sequence_audio(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<AppSnapshotDto> {
    let (project_root, sequence_path) = {
        let model = lock_model(&state)?;
        let snapshot = model.snapshot();
        let Some(sequence_path) = snapshot.active_file else {
            return Err("no active sequence file is selected".to_string());
        };
        if !matches!(
            snapshot.active_gui_document,
            Some(dawn_app_core::app_model::ActiveGuiDocument::Sequence(_))
        ) {
            return Err("active document is not a sequence".to_string());
        }
        (model.project_root.clone(), sequence_path)
    };

    let Some(project_root) = project_root else {
        return Err("no project is open".to_string());
    };
    let project_root = Utf8PathBuf::from(project_root);
    let sequence_path = if sequence_path.is_absolute() {
        sequence_path
    } else {
        project_root.join(sequence_path)
    };

    let mut dialog = rfd::FileDialog::new()
        .set_title("Choose Sequence Audio")
        .add_filter("Audio", &["mp3", "wav", "flac", "m4a", "aac", "ogg"]);
    let audio_dir = project_root.join("audio");
    if audio_dir.is_dir() {
        dialog = dialog.set_directory(audio_dir.as_std_path());
    }

    let Some(path) = dialog.pick_file() else {
        return get_snapshot(state);
    };
    let import = serialized_import_path(&sequence_path, &utf8_path(path)?);
    dispatch(
        &app,
        &state,
        AppAction::ApplySequenceGuiEdit(SequenceGuiEditDto::SetAudio {
            import: Some(import),
        }),
    )
}

#[specta::specta]
#[tauri::command]
fn clear_sequence_audio(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<AppSnapshotDto> {
    dispatch(
        &app,
        &state,
        AppAction::ApplySequenceGuiEdit(SequenceGuiEditDto::SetAudio { import: None }),
    )
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
async fn open_preview_window(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
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

#[specta::specta]
#[tauri::command]
fn preview_play(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::PreviewPlay)
}

#[specta::specta]
#[tauri::command]
fn preview_pause(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::PreviewPause)
}

#[specta::specta]
#[tauri::command]
fn preview_stop(app: AppHandle, state: State<'_, AppState>) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::PreviewStop)
}

#[specta::specta]
#[tauri::command]
fn preview_rewind_to_zero(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::PreviewRewindToZero)
}

#[specta::specta]
#[tauri::command]
fn preview_seek(
    app: AppHandle,
    state: State<'_, AppState>,
    position_ms: u32,
) -> CommandResult<AppSnapshotDto> {
    dispatch(&app, &state, AppAction::PreviewSeek(position_ms.into()))
}

#[specta::specta]
#[tauri::command]
fn get_preview_scene(state: State<'_, AppState>) -> CommandResult<PreviewSceneDto> {
    let snapshot = lock_model(&state)?.preview.snapshot();
    Ok(preview_scene_from_frame(
        &snapshot.frame,
        snapshot.source_label,
    ))
}

#[specta::specta]
#[tauri::command]
fn get_preview_transport_mode() -> CommandResult<PreviewTransportMode> {
    Ok(PreviewTransportRuntime::mode())
}

#[specta::specta]
#[tauri::command]
fn init_preview_transport(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let Some(window) = app.get_webview_window("preview") else {
        return Err("preview window is not open".to_string());
    };
    let pixel_count = preview_pixel_count(&lock_model(&state)?.preview.snapshot().frame);
    lock_preview_transport(&state)?.init_window(&window, pixel_count)
}

#[specta::specta]
#[tauri::command]
fn dispose_preview_transport(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let label = app
        .get_webview_window("preview")
        .map(|window| window.label().to_string())
        .unwrap_or_else(|| "preview".to_string());
    lock_preview_transport(&state)?.dispose_window(&label);
    Ok(())
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
            choose_sequence_audio,
            clear_sequence_audio,
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
            open_preview_window,
            preview_play,
            preview_pause,
            preview_stop,
            preview_rewind_to_zero,
            preview_seek,
            get_preview_scene,
            init_preview_transport,
            dispose_preview_transport,
            get_preview_transport_mode
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

pub fn run() -> Result<(), tauri::Error> {
    let builder = specta_builder();
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(builder.invoke_handler())
        .setup(|app| {
            let _ = app.get_webview_window("main");
            start_preview_worker(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
}

fn start_preview_worker(app: AppHandle) {
    thread::spawn(move || {
        let mut last_published_generation: Option<u64> = None;
        let mut had_sink = false;
        let mut last_event_at = Instant::now() - PREVIEW_STATE_EVENT_INTERVAL;
        let mut last_event_identity: Option<PreviewEventIdentity> = None;
        loop {
            let state = app.state::<AppState>();
            let started = Instant::now();
            let has_sink = lock_preview_transport(&state)
                .map(|runtime| runtime.has_sinks())
                .unwrap_or(false);
            if has_sink && !had_sink {
                last_published_generation = None;
            }
            had_sink = has_sink;

            let (snapshot, target_fps) = match lock_model(&state) {
                Ok(mut model) => {
                    model.tick_preview_clock();
                    let mut snapshot = model.preview.snapshot();
                    let should_render_frame = has_sink
                        && (snapshot.is_playing
                            || last_published_generation != Some(snapshot.frame.generation));
                    if should_render_frame {
                        model.render_preview_frame();
                        snapshot = model.preview.snapshot();
                    }
                    (snapshot, model.preview_target_fps())
                }
                Err(_) => {
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
            };
            let backend_ms = started.elapsed().as_secs_f32() * 1000.0;
            let frame_generation = snapshot.frame.generation;
            let should_publish_frame = has_sink
                && (snapshot.is_playing || last_published_generation != Some(frame_generation));
            if should_publish_frame {
                if let Ok(mut runtime) = lock_preview_transport(&state) {
                    runtime.publish_frame(&snapshot.frame, snapshot.is_playing, backend_ms);
                    last_published_generation = Some(frame_generation);
                }
            }

            let event_identity = PreviewEventIdentity::from(&snapshot);
            let should_emit_event = if snapshot.is_playing {
                last_event_identity.as_ref() != Some(&event_identity)
                    || last_event_at.elapsed() >= PREVIEW_STATE_EVENT_INTERVAL
            } else {
                last_event_identity.as_ref() != Some(&event_identity)
            };
            if should_emit_event {
                emit_preview_state_snapshot(&app, &snapshot);
                last_event_at = Instant::now();
                last_event_identity = Some(event_identity);
            }

            let fps = if has_sink {
                target_fps.max(1)
            } else if snapshot.is_playing {
                target_fps.clamp(1, 30)
            } else {
                10
            };
            let target = Duration::from_millis((1000 / fps as u64).max(1));
            let elapsed = started.elapsed();
            if elapsed < target {
                thread::sleep(target - elapsed);
            }
        }
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewEventIdentity {
    source_label: String,
    is_playing: bool,
    position_ms: u64,
    home_ms: u64,
    duration_ms: u64,
    audio_path: Option<String>,
    audio_exists: bool,
    status: String,
}

impl From<&PreviewSnapshot> for PreviewEventIdentity {
    fn from(snapshot: &PreviewSnapshot) -> Self {
        Self {
            source_label: snapshot.source_label.clone(),
            is_playing: snapshot.is_playing,
            position_ms: if snapshot.is_playing {
                0
            } else {
                snapshot.position_ms
            },
            home_ms: snapshot.home_ms,
            duration_ms: snapshot.duration_ms,
            audio_path: snapshot
                .audio
                .as_ref()
                .map(|audio| audio.resolved_path.clone()),
            audio_exists: snapshot.audio.as_ref().is_some_and(|audio| audio.exists),
            status: snapshot.status.clone(),
        }
    }
}

fn persist_preview_window_layout(app: &AppHandle) {
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
        emit_preview_state_dto(app, &snapshot)?;
    }
    Ok(snapshot)
}

fn emit_preview_state_dto(app: &AppHandle, snapshot: &AppSnapshotDto) -> CommandResult<()> {
    app.emit(
        "preview_state_changed",
        PreviewStateEventDto {
            source_label: snapshot.preview.source_label.clone(),
            is_playing: snapshot.preview.is_playing,
            position_ms: snapshot.preview.position_ms,
            home_ms: snapshot.preview.home_ms,
            duration_ms: snapshot.preview.duration_ms,
            audio: snapshot.preview.audio.clone(),
            status: snapshot.preview.status.clone(),
        },
    )
    .map_err(|error| error.to_string())
}

fn emit_preview_state_snapshot(app: &AppHandle, snapshot: &PreviewSnapshot) {
    let _ = app.emit(
        "preview_state_changed",
        PreviewStateEventDto {
            source_label: snapshot.source_label.clone(),
            is_playing: snapshot.is_playing,
            position_ms: snapshot.position_ms.min(u32::MAX as u64) as u32,
            home_ms: snapshot.home_ms.min(u32::MAX as u64) as u32,
            duration_ms: snapshot.duration_ms.min(u32::MAX as u64) as u32,
            audio: snapshot.audio.clone().map(Into::into),
            status: snapshot.status.clone(),
        },
    );
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

fn lock_preview_transport<'a>(
    state: &'a State<'_, AppState>,
) -> CommandResult<std::sync::MutexGuard<'a, PreviewTransportRuntime>> {
    state
        .preview_transport
        .lock()
        .map_err(|_| "preview transport lock is poisoned".to_string())
}

fn preview_pixel_count(frame: &OutputFrame) -> usize {
    frame
        .fixtures
        .iter()
        .map(|fixture| fixture.pixels.len())
        .sum()
}

fn preview_scene_from_frame(frame: &OutputFrame, source_label: String) -> PreviewSceneDto {
    let mut first_pixel_index = 0usize;
    let fixtures = frame
        .fixtures
        .iter()
        .map(|fixture| {
            let pixels = fixture
                .pixels
                .iter()
                .map(|pixel| pixel.position.into())
                .collect::<Vec<_>>();
            let dto = PreviewSceneFixtureDto {
                id: fixture.id.0,
                name: fixture.name.clone(),
                bulb_radius: fixture.bulb_radius,
                first_pixel_index: first_pixel_index.min(u32::MAX as usize) as u32,
                pixels,
            };
            first_pixel_index = first_pixel_index.saturating_add(fixture.pixels.len());
            dto
        })
        .collect::<Vec<_>>();
    PreviewSceneDto {
        generation: frame.generation.min(u32::MAX as u64) as u32,
        source_label,
        bounds: frame.bounds.into(),
        pixel_count: first_pixel_index.min(u32::MAX as usize) as u32,
        fixtures,
    }
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
