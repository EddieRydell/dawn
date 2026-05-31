use std::thread;
use std::time::{Duration, Instant};

use dawn_app_core::dto::{GeometryRenderBoundsDto, GeometryRenderPointDto};
use dawn_app_core::output_runtime::OutputFrame;
use dawn_app_core::preview_session::{AudioPlaybackStatus, PreviewSnapshot};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::app_runtime::{apply_audio_clock_to_model, emit_preview_state_snapshot};
use crate::state::{
    lock_audio_runtime, lock_model, lock_preview_transport, AppState, CommandResult,
};
use crate::window_layout::{persist_window_layout, WorkbenchWindow};

const PREVIEW_STATE_EVENT_INTERVAL: Duration = Duration::from_millis(33);

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewStateEventDto {
    pub source_label: String,
    pub is_playing: bool,
    pub position_seconds: f64,
    pub home_seconds: f64,
    pub duration_seconds: f64,
    pub audio: Option<dawn_app_core::dto::SequenceAudioDto>,
    pub clock_source: String,
    pub audio_playback_status: AudioPlaybackStatus,
    pub status: String,
    pub timing: PreviewTimingDto,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewTimingDto {
    pub backend_seconds: f64,
    pub target_fps: u32,
    pub active_fps: u32,
    pub target_frame_ms: f64,
    pub sleep_planned_ms: f64,
    pub loop_interval_ms: f64,
    pub audio_position_seconds: Option<f64>,
    pub snapshot_position_seconds: f64,
    pub frame_position_seconds: f64,
    pub snapshot_minus_audio_ms: Option<f64>,
    pub frame_minus_audio_ms: Option<f64>,
    pub loop_elapsed_ms: f64,
    pub audio_poll_ms: f64,
    pub audio_apply_ms: f64,
    pub model_update_ms: f64,
    pub render_ms: f64,
    pub publish_ms: f64,
    pub event_interval_ms: f64,
    pub has_sink: bool,
    pub published_frame: bool,
    pub rendered_frame: bool,
}

impl PreviewTimingDto {
    pub(crate) fn empty(backend_seconds: f64) -> Self {
        Self {
            backend_seconds,
            target_fps: 0,
            active_fps: 0,
            target_frame_ms: 0.0,
            sleep_planned_ms: 0.0,
            loop_interval_ms: 0.0,
            audio_position_seconds: None,
            snapshot_position_seconds: 0.0,
            frame_position_seconds: 0.0,
            snapshot_minus_audio_ms: None,
            frame_minus_audio_ms: None,
            loop_elapsed_ms: 0.0,
            audio_poll_ms: 0.0,
            audio_apply_ms: 0.0,
            model_update_ms: 0.0,
            render_ms: 0.0,
            publish_ms: 0.0,
            event_interval_ms: 0.0,
            has_sink: false,
            published_frame: false,
            rendered_frame: false,
        }
    }
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
    pub bulb_radius_meters: f64,
    pub first_pixel_index: u32,
    pub pixels: Vec<GeometryRenderPointDto>,
}

pub(crate) fn start_preview_worker(app: AppHandle) {
    thread::spawn(move || {
        let worker_started = Instant::now();
        let sleeper = spin_sleep::SpinSleeper::default();
        let mut last_published_generation: Option<u64> = None;
        let mut had_sink = false;
        let mut last_event_at = Instant::now() - PREVIEW_STATE_EVENT_INTERVAL;
        let mut last_event_identity: Option<PreviewEventIdentity> = None;
        let mut previous_loop_started = worker_started;
        loop {
            let state = app.state::<AppState>();
            let started = Instant::now();
            let loop_interval_ms =
                started.duration_since(previous_loop_started).as_secs_f64() * 1000.0;
            previous_loop_started = started;
            let has_sink = lock_preview_transport(&state)
                .map(|runtime| runtime.has_sinks())
                .unwrap_or(false);
            if has_sink && !had_sink {
                last_published_generation = None;
            }
            had_sink = has_sink;

            let mut timing = PreviewTimingDto::empty(worker_started.elapsed().as_secs_f64());
            timing.has_sink = has_sink;
            timing.loop_interval_ms = loop_interval_ms;
            let (snapshot, target_fps) = match lock_model(&state) {
                Ok(mut model) => {
                    let model_started = Instant::now();
                    let preview_snapshot = model.preview.snapshot();
                    let audio_poll_started = Instant::now();
                    let audio_clock = if preview_snapshot.audio.is_some() {
                        lock_audio_runtime(&state)
                            .ok()
                            .and_then(|runtime| runtime.clock().ok())
                    } else {
                        None
                    };
                    timing.audio_poll_ms = audio_poll_started.elapsed().as_secs_f64() * 1000.0;
                    timing.audio_position_seconds =
                        audio_clock.as_ref().map(|clock| clock.position_seconds);
                    let rendered_during_clock_apply = if let Some(clock) = audio_clock {
                        let apply_started = Instant::now();
                        let analysis = model.analysis.clone();
                        apply_audio_clock_to_model(&mut model, &clock, analysis.as_ref());
                        timing.audio_apply_ms = apply_started.elapsed().as_secs_f64() * 1000.0;
                        true
                    } else {
                        model.tick_preview_clock();
                        false
                    };
                    let mut snapshot = model.preview.snapshot();
                    let should_render_frame = has_sink
                        && (snapshot.is_playing
                            || last_published_generation != Some(snapshot.frame.generation));
                    if should_render_frame && !rendered_during_clock_apply {
                        let render_started = Instant::now();
                        model.render_preview_frame();
                        timing.render_ms = render_started.elapsed().as_secs_f64() * 1000.0;
                        timing.rendered_frame = true;
                        snapshot = model.preview.snapshot();
                    } else if should_render_frame {
                        timing.rendered_frame = true;
                    }
                    timing.model_update_ms = model_started.elapsed().as_secs_f64() * 1000.0;
                    (snapshot, model.preview_target_fps())
                }
                Err(_) => {
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
            };
            let backend_seconds = worker_started.elapsed().as_secs_f32();
            let frame_generation = snapshot.frame.generation;
            timing.backend_seconds = backend_seconds as f64;
            timing.snapshot_position_seconds = snapshot.position_seconds;
            timing.frame_position_seconds = snapshot.frame.time_seconds;
            if let Some(audio_seconds) = timing.audio_position_seconds {
                timing.snapshot_minus_audio_ms =
                    Some((snapshot.position_seconds - audio_seconds) * 1000.0);
                timing.frame_minus_audio_ms =
                    Some((snapshot.frame.time_seconds - audio_seconds) * 1000.0);
            }
            let should_publish_frame = has_sink
                && (snapshot.is_playing || last_published_generation != Some(frame_generation));
            if should_publish_frame {
                if let Ok(mut runtime) = lock_preview_transport(&state) {
                    let publish_started = Instant::now();
                    runtime.publish_frame(&snapshot.frame, snapshot.is_playing, backend_seconds);
                    timing.publish_ms = publish_started.elapsed().as_secs_f64() * 1000.0;
                    timing.published_frame = true;
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
            let fps = if has_sink {
                target_fps.max(1)
            } else if snapshot.is_playing {
                target_fps.clamp(1, 30)
            } else {
                10
            };
            let target = Duration::from_secs_f64(1.0 / fps as f64);
            let target_deadline = started + target;
            let elapsed = started.elapsed();
            timing.target_fps = target_fps;
            timing.active_fps = fps;
            timing.target_frame_ms = target.as_secs_f64() * 1000.0;
            timing.loop_elapsed_ms = elapsed.as_secs_f64() * 1000.0;
            timing.sleep_planned_ms = if elapsed < target {
                (target - elapsed).as_secs_f64() * 1000.0
            } else {
                0.0
            };
            timing.event_interval_ms = last_event_at.elapsed().as_secs_f64() * 1000.0;
            if should_emit_event {
                emit_preview_state_snapshot(&app, &snapshot, timing);
                last_event_at = Instant::now();
                last_event_identity = Some(event_identity);
            }
            if Instant::now() < target_deadline {
                sleeper.sleep_until(target_deadline);
            }
        }
    });
}

pub(crate) fn preview_pixel_count(frame: &OutputFrame) -> usize {
    frame
        .fixtures
        .iter()
        .map(|fixture| fixture.pixels.len())
        .sum()
}

pub(crate) fn preview_scene_from_frame(
    frame: &OutputFrame,
    source_label: String,
) -> PreviewSceneDto {
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
                bulb_radius_meters: fixture.bulb_radius.as_meters_f64(),
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

#[derive(Debug, Clone, PartialEq)]
struct PreviewEventIdentity {
    source_label: String,
    is_playing: bool,
    position_seconds: f64,
    home_seconds: f64,
    duration_seconds: f64,
    audio_path: Option<String>,
    audio_exists: bool,
    clock_source: String,
    audio_playback_status: AudioPlaybackStatus,
    status: String,
}

impl From<&PreviewSnapshot> for PreviewEventIdentity {
    fn from(snapshot: &PreviewSnapshot) -> Self {
        Self {
            source_label: snapshot.source_label.clone(),
            is_playing: snapshot.is_playing,
            position_seconds: if snapshot.is_playing {
                0.0
            } else {
                snapshot.position_seconds
            },
            home_seconds: snapshot.home_seconds,
            duration_seconds: snapshot.duration_seconds,
            audio_path: snapshot
                .audio
                .as_ref()
                .map(|audio| audio.resolved_path.clone()),
            audio_exists: snapshot.audio.as_ref().is_some_and(|audio| audio.exists),
            clock_source: snapshot.clock_source.clone(),
            audio_playback_status: snapshot.audio_playback_status.clone(),
            status: snapshot.status.clone(),
        }
    }
}
