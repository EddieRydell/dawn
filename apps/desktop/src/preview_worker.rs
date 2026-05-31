use std::thread;
use std::time::{Duration, Instant};

use dawn_app_core::preview_session::PreviewSnapshot;
use tauri::{AppHandle, Manager};

use crate::app_runtime::{
    apply_audio_clock_to_model, emit_preview_state_snapshot, valid_sequence_audio,
};
use crate::preview_types::PreviewTimingDto;
use crate::state::{lock_audio_runtime, lock_model, lock_preview_transport, AppState};

const PREVIEW_STATE_EVENT_INTERVAL: Duration = Duration::from_millis(33);

pub(crate) fn start_preview_worker(app: AppHandle) {
    thread::spawn(move || {
        let worker_started = Instant::now();
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
                    let audio_clock = if valid_sequence_audio(&preview_snapshot).is_some() {
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
            if elapsed < target {
                thread::sleep(target - elapsed);
            }
        }
    });
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
    audio_playback_status: String,
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
