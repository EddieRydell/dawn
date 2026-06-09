use dawn_app_core::dto::{GeometryRenderBoundsDto, GeometryRenderPointDto};
use dawn_app_core::output_runtime::{geometry_pixel_count, OutputGeometryModel};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::state::{lock_model, AppState, CommandResult};
use crate::window_layout::{persist_window_layout, WorkbenchWindow};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSceneDto {
    pub generation: u32,
    pub topology_identity: String,
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
    pub bulb_radius_meters: f64,
    pub first_pixel_index: u32,
    pub pixels: Vec<GeometryRenderPointDto>,
}

pub(crate) fn preview_pixel_count(geometry: &OutputGeometryModel) -> usize {
    geometry_pixel_count(geometry)
}

pub(crate) fn preview_scene_from_geometry(
    geometry: &OutputGeometryModel,
    generation: u64,
    source_label: String,
) -> PreviewSceneDto {
    let mut first_pixel_index = 0usize;
    let fixtures = geometry
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
                bulb_radius_meters: fixture.bulb_radius.as_meters_f64(),
                first_pixel_index: first_pixel_index.min(u32::MAX as usize) as u32,
                pixels,
            };
            first_pixel_index = first_pixel_index.saturating_add(fixture.pixels.len());
            dto
        })
        .collect::<Vec<_>>();
    PreviewSceneDto {
        generation: generation.min(u32::MAX as u64) as u32,
        topology_identity: geometry.geometry_id.clone(),
        source_label,
        bounds: geometry.bounds.into(),
        pixel_count: first_pixel_index.min(u32::MAX as usize) as u32,
        fixtures,
    }
}

pub(crate) fn open_or_focus_preview_window(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    open_preview_window(app, state, true)
}

pub(crate) fn open_preview_window_on_startup(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    let should_open = lock_model(&state)?.workbench_layout.preview_window_open;
    if should_open {
        open_preview_window(app, state, false)?;
    }
    Ok(())
}

fn open_preview_window(
    app: AppHandle,
    state: State<'_, AppState>,
    focus: bool,
) -> CommandResult<()> {
    if let Some(window) = app.get_webview_window("preview") {
        window.show().map_err(|error| error.to_string())?;
        if focus {
            window.set_focus().map_err(|error| error.to_string())?;
        }
        return Ok(());
    }

    let layout = {
        let mut model = lock_model(&state)?;
        model.set_preview_window_open(true)?;
        model.workbench_layout.preview_window.clone()
    };
    let window =
        WebviewWindowBuilder::new(&app, "preview", WebviewUrl::App("/?view=preview".into()))
            .title("Dawn Preview")
            .position(layout.x, layout.y)
            .inner_size(layout.width, layout.height)
            .build()
            .map_err(|error| error.to_string())?;
    if layout.maximized {
        window.maximize().map_err(|error| error.to_string())?;
    }
    let app_for_event = app.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            persist_window_layout(&app_for_event, WorkbenchWindow::Preview);
        }
        WindowEvent::CloseRequested { .. } => {
            persist_window_layout(&app_for_event, WorkbenchWindow::Preview);
            let state = app_for_event.state::<AppState>();
            if !state.is_shutting_down() {
                persist_preview_window_open(&app_for_event, false);
            }
        }
        WindowEvent::Destroyed => {
            persist_window_layout(&app_for_event, WorkbenchWindow::Preview);
        }
        _ => {}
    });
    if focus {
        window.set_focus().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn persist_preview_window_open(app: &AppHandle, open: bool) {
    let state = app.state::<AppState>();
    if let Ok(mut model) = lock_model(&state) {
        let _ = model.set_preview_window_open(open);
    };
}
