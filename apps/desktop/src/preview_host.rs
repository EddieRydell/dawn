use std::time::{Duration, Instant};

use tauri::Manager;

use crate::{
    dto::{PreviewSnapshotDto, PreviewStateEventDto, PreviewTimingDto},
    events,
    state::AppState,
};

const IDLE_SLEEP: Duration = Duration::from_millis(100);

pub(crate) fn start(app: tauri::AppHandle) {
    tauri::async_runtime::spawn_blocking(move || run_loop(app));
}

fn run_loop(app: tauri::AppHandle) {
    let started = Instant::now();
    let mut previous_emit = Instant::now();
    let mut last_published_generation = None;
    let mut had_sink = false;
    loop {
        let loop_started = Instant::now();
        let state = app.state::<AppState>();
        let audio = state.audio();

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
            || audio_loading;

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
        let tick = {
            let mut backend = match state.lock_backend() {
                Ok(backend) => backend,
                Err(error) => {
                    events::emit_backend_error(&app, error);
                    spin_sleep::sleep(IDLE_SLEEP);
                    continue;
                }
            };
            backend.preview_tick(audio_clock)
        };

        let tick = match tick {
            Ok(tick) => tick,
            Err(error) => {
                events::emit_backend_error(&app, error.to_string());
                spin_sleep::sleep(IDLE_SLEEP);
                continue;
            }
        };

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
            tick.render_timing,
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
        previous_emit = Instant::now();

        let event = PreviewStateEventDto {
            preview: PreviewSnapshotDto::from(&tick.snapshot),
            timing,
        };
        if let Err(error) = events::emit_preview_state(&app, event) {
            events::emit_backend_error(&app, error);
        }

        let target_sleep = Duration::from_secs_f64(1.0 / f64::from(tick.target_fps.max(1)));
        let elapsed = loop_started.elapsed();
        if target_sleep > elapsed {
            spin_sleep::sleep(target_sleep - elapsed);
        }
    }
}
