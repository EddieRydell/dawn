use std::time::{Duration, Instant};

use dawn_backend::{PreviewRenderTiming, PreviewSnapshot};
use tauri::Manager;

use crate::{
    dto::{PreviewSnapshotDto, PreviewStateEventDto, PreviewTimingDto},
    events,
    preview_frame_runtime::PreviewFrameRuntime,
    state::AppState,
};

const IDLE_SLEEP: Duration = Duration::from_millis(100);
const ACTIVE_EVENT_INTERVAL: Duration = Duration::from_millis(33);

pub(crate) fn start(app: tauri::AppHandle) {
    tauri::async_runtime::spawn_blocking(move || run_loop(app));
}

fn run_loop(app: tauri::AppHandle) {
    let started = Instant::now();
    let mut previous_emit = Instant::now();
    let mut last_published_generation = None;
    let mut had_sink = false;
    let mut frame_runtime = PreviewFrameRuntime::default();
    let mut last_idle_event_identity: Option<PreviewEventIdentity> = None;
    loop {
        let loop_started = Instant::now();
        let state = app.state::<AppState>();
        let audio = state.audio();
        let mut completed_render_timing = None;

        let host_state = match state.lock_backend() {
            Ok(backend) => backend.preview_host_state(),
            Err(error) => {
                events::emit_backend_error(&app, error);
                spin_sleep::sleep(IDLE_SLEEP);
                continue;
            }
        };
        let has_sink = state
            .lock_preview_transport()
            .map(|transport| transport.has_sinks())
            .unwrap_or(false);
        let audio_loading = host_state.audio_playback_status.is_loading();
        let active = host_state.is_playing
            || host_state.effect_preview_active
            || host_state.preview_updating
            || audio_loading
            || frame_runtime.has_in_flight();

        match frame_runtime.try_take_completed() {
            Ok(Some(render_output)) => {
                let render_timing = render_output.timing;
                match state.lock_backend() {
                    Ok(mut backend) => {
                        if backend.complete_preview_frame_render(render_output) {
                            completed_render_timing = Some(render_timing);
                        }
                    }
                    Err(error) => {
                        events::emit_backend_error(&app, error);
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                events::emit_backend_error(&app, error);
            }
        }

        if !active {
            if has_sink
                && (!had_sink || last_published_generation != Some(host_state.frame_generation))
            {
                match (state.lock_backend(), state.lock_preview_transport()) {
                    (Ok(backend), Ok(mut transport)) => {
                        transport.publish_frame(
                            backend.preview_frame(),
                            host_state.is_playing,
                            started.elapsed().as_secs_f32(),
                        );
                        last_published_generation = Some(host_state.frame_generation);
                    }
                    (Err(error), _) | (_, Err(error)) => {
                        events::emit_backend_error(&app, error);
                    }
                }
            }
            had_sink = has_sink;
            spin_sleep::sleep(IDLE_SLEEP);
            continue;
        }
        had_sink = has_sink;

        let audio_clock = if host_state.has_valid_audio && (host_state.is_playing || audio_loading)
        {
            match audio.clock() {
                Ok(clock) => Some(clock),
                Err(error) => {
                    events::emit_backend_error(&app, error);
                    None
                }
            }
        } else {
            None
        };
        let audio_position_seconds = audio_clock.as_ref().map(|clock| clock.position_seconds);
        let tick_result = {
            let mut backend = match state.lock_backend() {
                Ok(backend) => backend,
                Err(error) => {
                    events::emit_backend_error(&app, error);
                    spin_sleep::sleep(IDLE_SLEEP);
                    continue;
                }
            };
            match backend.update_preview_clock(audio_clock) {
                Ok(tick) => {
                    let render_task = if frame_runtime.has_in_flight() {
                        None
                    } else {
                        backend.begin_preview_frame_render()
                    };
                    Ok((tick, render_task))
                }
                Err(error) => Err(error),
            }
        };

        let (mut tick, render_task) = match tick_result {
            Ok(output) => output,
            Err(error) => {
                events::emit_backend_error(&app, error.to_string());
                spin_sleep::sleep(IDLE_SLEEP);
                continue;
            }
        };

        if let Some(render_task) = render_task {
            if let Err(error) = frame_runtime.submit(render_task) {
                events::emit_backend_error(&app, error);
            }
        }
        if let Some(render_timing) = completed_render_timing {
            match state.lock_backend() {
                Ok(backend) => {
                    tick.snapshot = backend.preview_snapshot();
                    tick.render_timing = render_timing;
                }
                Err(error) => {
                    events::emit_backend_error(&app, error);
                }
            }
        }

        let published_frame = if has_sink {
            match state.lock_preview_transport() {
                Ok(mut transport) => {
                    transport.publish_frame(
                        &tick.snapshot.frame,
                        tick.snapshot.is_playing,
                        started.elapsed().as_secs_f32(),
                    );
                    last_published_generation = Some(tick.snapshot.frame.generation);
                    true
                }
                Err(error) => {
                    events::emit_backend_error(&app, error);
                    false
                }
            }
        } else {
            false
        };

        let loop_elapsed_ms = loop_started.elapsed().as_secs_f64() * 1000.0;
        let mut timing = PreviewTimingDto::from_render(
            completed_render_timing.unwrap_or_else(PreviewRenderTiming::default),
            started.elapsed().as_secs_f64(),
            tick.target_fps,
            loop_elapsed_ms,
            has_sink,
            published_frame,
        );
        timing.audio_position_seconds = audio_position_seconds;
        timing.snapshot_position_seconds = tick.snapshot.position_seconds;
        timing.frame_position_seconds = tick.snapshot.frame.time_seconds;
        timing.snapshot_minus_audio_ms = audio_position_seconds
            .map(|position| (tick.snapshot.position_seconds - position) * 1000.0);
        timing.frame_minus_audio_ms = audio_position_seconds
            .map(|position| (tick.snapshot.frame.time_seconds - position) * 1000.0);
        timing.event_interval_ms = previous_emit.elapsed().as_secs_f64() * 1000.0;

        let active_identity = PreviewEventIdentity::from_snapshot(&tick.snapshot, has_sink);
        let should_emit = if active {
            previous_emit.elapsed() >= ACTIVE_EVENT_INTERVAL
        } else if last_idle_event_identity.as_ref() != Some(&active_identity) {
            last_idle_event_identity = Some(active_identity);
            true
        } else {
            false
        };
        if should_emit {
            let event = PreviewStateEventDto {
                preview: PreviewSnapshotDto::from(&tick.snapshot),
                timing,
            };
            if let Err(error) = events::emit_preview_state(&app, event) {
                events::emit_backend_error(&app, error);
            }
            previous_emit = Instant::now();
        }

        let target_fps = if has_sink {
            tick.target_fps
        } else {
            tick.target_fps.min(30)
        };
        let target_sleep = Duration::from_secs_f64(1.0 / f64::from(target_fps.max(1)));
        let elapsed = loop_started.elapsed();
        if target_sleep > elapsed {
            spin_sleep::sleep(target_sleep - elapsed);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct PreviewEventIdentity {
    source_label: String,
    is_playing: bool,
    preview_updating: bool,
    effect_preview_active: bool,
    duration_seconds: f64,
    clock_source: String,
    audio_playback_status: dawn_backend::AudioPlaybackStatus,
    status: String,
    frame_generation: u64,
    has_sink: bool,
}

impl PreviewEventIdentity {
    fn from_snapshot(snapshot: &PreviewSnapshot, has_sink: bool) -> Self {
        Self {
            source_label: snapshot.source_label.clone(),
            is_playing: snapshot.is_playing,
            preview_updating: snapshot.preview_updating,
            effect_preview_active: snapshot.effect_preview_active,
            duration_seconds: snapshot.duration_seconds,
            clock_source: snapshot.clock_source.clone(),
            audio_playback_status: snapshot.audio_playback_status,
            status: snapshot.status.clone(),
            frame_generation: snapshot.frame.generation,
            has_sink,
        }
    }
}
