use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use dawn_app_core::dto::SequenceKeyDto;
use dawn_app_core::output_runtime::{
    empty_frame, empty_geometry, OutputGeometryModel, RenderedOutputFrame,
};
use dawn_app_core::renderer::{render_sequence_frame, RenderFrameInput};
use dawn_app_core::sequence_transport_session::{
    AudioPlaybackStatus, PlaybackRenderMode, PlaybackRenderRequest, PlaybackRenderResult,
    PlaybackRenderTiming, SequenceKey, SequenceTransportSnapshot, SequenceTransportState,
};
use dawn_project::DawnProject;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::app_runtime::{apply_audio_clock_to_model, emit_sequence_render_snapshot};
use crate::audio_runtime::AudioClock;
use crate::state::{lock_audio_runtime, lock_live_output, lock_model, AppState};

const SEQUENCE_STATE_EVENT_INTERVAL: Duration = Duration::from_millis(33);

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceRenderEventDto {
    pub source_label: String,
    pub source_key: Option<SequenceKeyDto>,
    pub render_generation: u32,
    pub render_dirty_revision: u32,
    pub render_updating: bool,
    pub position_seconds: f64,
    pub geometry_identity: String,
    pub timing: SequenceRenderTimingDto,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceRenderTimingDto {
    pub backend_seconds: f64,
    pub target_fps: u32,
    pub active_fps: u32,
    pub target_frame_ms: f64,
    pub sleep_planned_ms: f64,
    pub loop_interval_ms: f64,
    pub audio_position_seconds: Option<f64>,
    pub snapshot_position_seconds: f64,
    pub render_buffer_position_seconds: f64,
    pub snapshot_minus_audio_ms: Option<f64>,
    pub render_buffer_minus_audio_ms: Option<f64>,
    pub loop_elapsed_ms: f64,
    pub loop_total_ms: f64,
    pub loop_accounted_ms: f64,
    pub loop_unaccounted_ms: f64,
    pub sleep_actual_ms: f64,
    pub live_output_lock_ms: f64,
    pub model_lock_wait_ms: f64,
    pub sequence_transport_snapshot_ms: f64,
    pub project_snapshot_ms: f64,
    pub audio_poll_ms: f64,
    pub audio_apply_ms: f64,
    pub model_update_ms: f64,
    pub render_ms: f64,
    pub render_wall_ms: f64,
    pub render_overhead_ms: f64,
    pub render_invalidation_ms: f64,
    pub render_cache_ms: f64,
    pub render_result_ms: f64,
    pub renderer_build_ms: f64,
    pub frame_evaluate_ms: f64,
    pub render_buffer_clone_ms: f64,
    pub frame_effect_loop_ms: f64,
    pub rgb_buffer_ms: f64,
    pub event_emit_ms: f64,
    pub live_output_ms: f64,
    pub rendered_active_effects: u32,
    pub rendered_sampled_pixels: u32,
    pub rendered_frame: bool,
}

impl SequenceRenderTimingDto {
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
            render_buffer_position_seconds: 0.0,
            snapshot_minus_audio_ms: None,
            render_buffer_minus_audio_ms: None,
            loop_elapsed_ms: 0.0,
            loop_total_ms: 0.0,
            loop_accounted_ms: 0.0,
            loop_unaccounted_ms: 0.0,
            sleep_actual_ms: 0.0,
            live_output_lock_ms: 0.0,
            model_lock_wait_ms: 0.0,
            sequence_transport_snapshot_ms: 0.0,
            project_snapshot_ms: 0.0,
            audio_poll_ms: 0.0,
            audio_apply_ms: 0.0,
            model_update_ms: 0.0,
            render_ms: 0.0,
            render_wall_ms: 0.0,
            render_overhead_ms: 0.0,
            render_invalidation_ms: 0.0,
            render_cache_ms: 0.0,
            render_result_ms: 0.0,
            renderer_build_ms: 0.0,
            frame_evaluate_ms: 0.0,
            render_buffer_clone_ms: 0.0,
            frame_effect_loop_ms: 0.0,
            rgb_buffer_ms: 0.0,
            event_emit_ms: 0.0,
            live_output_ms: 0.0,
            rendered_active_effects: 0,
            rendered_sampled_pixels: 0,
            rendered_frame: false,
        }
    }
}

#[derive(Debug, Default)]
struct DeferredSequenceRenderer;

impl DeferredSequenceRenderer {
    fn render(
        &mut self,
        project: Option<&DawnProject>,
        request: PlaybackRenderRequest,
    ) -> Option<PlaybackRenderResult> {
        if request.cancellation.is_cancelled() {
            return None;
        }
        let mut timing = PlaybackRenderTiming::default();
        let Some(project) = project else {
            let geometry = empty_geometry();
            let frame = empty_frame(&geometry, request.generation, "No project");
            return Some(PlaybackRenderResult {
                request,
                geometry,
                frame,
                timing,
            });
        };
        let PlaybackRenderMode::FullSequenceFrame {
            position_seconds, ..
        } = request.kind;
        let output = match render_sequence_frame(RenderFrameInput {
            project,
            sequence: &request.document,
            time_seconds: position_seconds,
            generation: request.generation,
        }) {
            Ok(output) => output,
            Err(error) => {
                let geometry =
                    OutputGeometryModel::from_project(project).unwrap_or_else(|_| empty_geometry());
                let frame = empty_frame(&geometry, request.generation, error.to_string());
                return Some(PlaybackRenderResult {
                    request,
                    geometry,
                    frame,
                    timing,
                });
            }
        };
        timing.apply_evaluation(output.timing.build.total_ms, output.timing.frame);
        if request.cancellation.is_cancelled() {
            return None;
        }
        let result_started = Instant::now();
        timing.render_result_ms = elapsed_ms(result_started);
        Some(PlaybackRenderResult {
            request,
            geometry: output.geometry,
            frame: output.frame,
            timing,
        })
    }
}

pub(crate) fn start_sequence_runtime(app: AppHandle) {
    thread::spawn(move || {
        let worker_started = Instant::now();
        let sleeper = spin_sleep::SpinSleeper::default();
        let mut deferred_renderer = DeferredSequenceRenderer;
        let mut last_event_at = Instant::now() - SEQUENCE_STATE_EVENT_INTERVAL;
        let mut last_event_identity: Option<SequenceRenderEventIdentity> = None;
        let mut last_event_emit_ms = 0.0;
        let mut previous_loop_started = worker_started;
        loop {
            let state = app.state::<AppState>();
            let started = Instant::now();
            let loop_interval_ms =
                started.duration_since(previous_loop_started).as_secs_f64() * 1000.0;
            previous_loop_started = started;
            let mut timing = SequenceRenderTimingDto::empty(worker_started.elapsed().as_secs_f64());
            timing.loop_interval_ms = loop_interval_ms;

            let live_output_lock_started = Instant::now();
            let live_output_enabled = lock_live_output(&state)
                .map(|runtime| runtime.enabled())
                .unwrap_or(false);
            timing.live_output_lock_ms = elapsed_ms(live_output_lock_started);

            timing.event_emit_ms = last_event_emit_ms;
            let audio_poll_started = Instant::now();
            let audio_clock = lock_audio_runtime(&state)
                .ok()
                .and_then(|runtime| runtime.clock().ok());
            timing.audio_poll_ms = elapsed_ms(audio_poll_started);
            timing.audio_position_seconds =
                audio_clock.as_ref().map(|clock| clock.position_seconds);
            let model_lock_started = Instant::now();
            let (mut snapshot, mut target_fps, mut project, deferred_request) =
                match lock_model(&state) {
                    Ok(mut model) => {
                        timing.model_lock_wait_ms = elapsed_ms(model_lock_started);
                        let model_started = Instant::now();
                        let sequence_transport_snapshot_started = Instant::now();
                        let sequence_transport_snapshot = model.sequence_transport.snapshot();
                        timing.sequence_transport_snapshot_ms +=
                            elapsed_ms(sequence_transport_snapshot_started);
                        if let Some(clock) = audio_clock.as_ref() {
                            if !should_apply_audio_clock_to_model(
                                &sequence_transport_snapshot,
                                clock,
                            ) {
                                model.tick_sequence_transport();
                            } else {
                                let project_snapshot_started = Instant::now();
                                let project = model.project.clone();
                                timing.project_snapshot_ms += elapsed_ms(project_snapshot_started);
                                let apply_started = Instant::now();
                                apply_audio_clock_to_model(&mut model, clock, project.as_deref());
                                timing.audio_apply_ms = elapsed_ms(apply_started);
                            }
                        } else {
                            model.tick_sequence_transport();
                        };
                        let sequence_transport_snapshot_started = Instant::now();
                        let snapshot = model.sequence_transport.snapshot();
                        timing.sequence_transport_snapshot_ms +=
                            elapsed_ms(sequence_transport_snapshot_started);
                        let deferred_request = if snapshot.render_updating {
                            model.begin_deferred_sequence_render()
                        } else {
                            None
                        };
                        timing.rendered_frame = deferred_request.is_some();
                        timing.model_update_ms = model_started.elapsed().as_secs_f64() * 1000.0;
                        let project_snapshot_started = Instant::now();
                        let project = model.project.clone();
                        timing.project_snapshot_ms += elapsed_ms(project_snapshot_started);
                        (
                            snapshot,
                            model.sequence_transport_target_fps(),
                            project,
                            deferred_request,
                        )
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                };
            if let Some(request) = deferred_request {
                let render_started = Instant::now();
                let result = deferred_renderer.render(project.as_deref(), request);
                timing.render_wall_ms = elapsed_ms(render_started);
                if let Some(result) = result {
                    record_render_timing(&mut timing, result.timing);
                    timing.render_overhead_ms = (timing.render_wall_ms - timing.render_ms).max(0.0);
                    timing.rendered_frame = true;
                    let model_lock_started = Instant::now();
                    if let Ok(mut model) = lock_model(&state) {
                        timing.model_lock_wait_ms += elapsed_ms(model_lock_started);
                        let model_started = Instant::now();
                        let _ = model.complete_deferred_sequence_render(result);
                        let sequence_transport_snapshot_started = Instant::now();
                        snapshot = model.sequence_transport.snapshot();
                        timing.sequence_transport_snapshot_ms +=
                            elapsed_ms(sequence_transport_snapshot_started);
                        let project_snapshot_started = Instant::now();
                        project = model.project.clone();
                        timing.project_snapshot_ms += elapsed_ms(project_snapshot_started);
                        target_fps = model.sequence_transport_target_fps();
                        timing.model_update_ms += elapsed_ms(model_started);
                    }
                }
            }
            let backend_seconds = worker_started.elapsed().as_secs_f32();
            timing.backend_seconds = backend_seconds as f64;
            timing.snapshot_position_seconds = snapshot.position_seconds;
            timing.render_buffer_position_seconds = snapshot.frame.time_seconds;
            if let Some(audio_seconds) = timing.audio_position_seconds {
                timing.snapshot_minus_audio_ms =
                    Some((snapshot.position_seconds - audio_seconds) * 1000.0);
                timing.render_buffer_minus_audio_ms =
                    Some((snapshot.frame.time_seconds - audio_seconds) * 1000.0);
            }
            if live_output_enabled {
                let live_output_started = Instant::now();
                publish_live_output_frame(
                    &app,
                    &state,
                    project.clone(),
                    &snapshot.geometry,
                    &snapshot.frame,
                );
                timing.live_output_ms = elapsed_ms(live_output_started);
            }

            let event_identity = SequenceRenderEventIdentity::from(&snapshot);
            let should_emit_event = if snapshot.transport_state.should_publish_continuously() {
                last_event_identity.as_ref() != Some(&event_identity)
                    || last_event_at.elapsed() >= SEQUENCE_STATE_EVENT_INTERVAL
            } else {
                last_event_identity.as_ref() != Some(&event_identity)
            };
            let fps = sequence_runtime_fps(&snapshot, target_fps, live_output_enabled);
            let target = Duration::from_secs_f64(1.0 / fps as f64);
            let target_deadline = started + target;
            let elapsed_before_event = started.elapsed();
            timing.target_fps = target_fps;
            timing.active_fps = fps;
            timing.target_frame_ms = target.as_secs_f64() * 1000.0;
            timing.loop_elapsed_ms = elapsed_before_event.as_secs_f64() * 1000.0;
            timing.sleep_planned_ms = if elapsed_before_event < target {
                (target - elapsed_before_event).as_secs_f64() * 1000.0
            } else {
                0.0
            };
            let sleep_started = Instant::now();
            if Instant::now() < target_deadline {
                sleeper.sleep_until(target_deadline);
            }
            timing.sleep_actual_ms = elapsed_ms(sleep_started);
            timing.loop_total_ms = elapsed_ms(started);
            timing.loop_accounted_ms = accounted_loop_ms(&timing);
            timing.loop_unaccounted_ms = (timing.loop_total_ms - timing.loop_accounted_ms).max(0.0);
            if should_emit_event {
                let event_emit_started = Instant::now();
                emit_sequence_render_snapshot(&app, &snapshot, timing);
                last_event_emit_ms = elapsed_ms(event_emit_started);
                last_event_at = Instant::now();
                last_event_identity = Some(event_identity);
            }
        }
    });
}

fn sequence_runtime_fps(
    snapshot: &SequenceTransportSnapshot,
    target_fps: u32,
    live_output_enabled: bool,
) -> u32 {
    if snapshot.transport_state.is_active_playback() || live_output_enabled {
        target_fps.max(1)
    } else {
        10
    }
}

fn record_render_timing(timing: &mut SequenceRenderTimingDto, render_timing: PlaybackRenderTiming) {
    timing.render_ms = render_timing.total_ms;
    timing.renderer_build_ms = render_timing.renderer_build_ms;
    timing.frame_evaluate_ms = render_timing.frame_evaluate_ms;
    timing.render_buffer_clone_ms = render_timing.render_buffer_clone_ms;
    timing.frame_effect_loop_ms = render_timing.effect_loop_ms;
    timing.rgb_buffer_ms = render_timing.rgb_buffer_ms;
    timing.rendered_active_effects = render_timing.active_effects;
    timing.rendered_sampled_pixels = render_timing.sampled_pixels;
    timing.render_invalidation_ms = render_timing.render_invalidation_ms;
    timing.render_cache_ms = render_timing.render_cache_ms;
    timing.render_result_ms = render_timing.render_result_ms;
}

fn accounted_loop_ms(timing: &SequenceRenderTimingDto) -> f64 {
    timing.live_output_lock_ms
        + timing.audio_poll_ms
        + timing.model_lock_wait_ms
        + timing.model_update_ms
        + timing.render_wall_ms
        + timing.live_output_ms
        + timing.event_emit_ms
        + timing.sleep_actual_ms
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn should_apply_audio_clock_to_model(
    sequence_transport_snapshot: &SequenceTransportSnapshot,
    clock: &AudioClock,
) -> bool {
    sequence_transport_snapshot.transport_state == SequenceTransportState::Playing
        || matches!(
            clock.status,
            AudioPlaybackStatus::Playing | AudioPlaybackStatus::Ended
        )
        || clock.ended
}

fn publish_live_output_frame(
    app: &AppHandle,
    state: &State<'_, AppState>,
    project: Option<Arc<DawnProject>>,
    geometry: &OutputGeometryModel,
    frame: &RenderedOutputFrame,
) {
    let snapshot = match lock_live_output(state) {
        Ok(mut runtime) => runtime.send_frame(project, geometry, frame),
        Err(_) => return,
    };
    let Ok(mut model) = lock_model(state) else {
        return;
    };
    if model.live_output != snapshot {
        model.set_live_output_snapshot(snapshot);
        let dto = model.snapshot_dto();
        let _ = app.emit("app_snapshot_changed", &dto);
    }
}

#[derive(Debug, Clone, PartialEq)]
struct SequenceRenderEventIdentity {
    source_label: String,
    source_key: Option<SequenceKey>,
    render_generation: u64,
    render_dirty_revision: u64,
    render_updating: bool,
    position_seconds: f64,
    geometry_identity: String,
}

impl From<&SequenceTransportSnapshot> for SequenceRenderEventIdentity {
    fn from(snapshot: &SequenceTransportSnapshot) -> Self {
        Self {
            source_label: snapshot.source_label.clone(),
            source_key: snapshot.source_key.clone(),
            render_generation: snapshot.render_generation,
            render_dirty_revision: snapshot.render_dirty_revision,
            render_updating: snapshot.render_updating,
            position_seconds: if snapshot.transport_state.should_publish_continuously() {
                0.0
            } else {
                snapshot.frame.time_seconds
            },
            geometry_identity: snapshot.geometry.geometry_id.clone(),
        }
    }
}
