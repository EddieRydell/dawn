use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use dawn_app_core::dto::{GeometryRenderBoundsDto, GeometryRenderPointDto};
use dawn_app_core::output_runtime::{
    empty_frame, OutputFrame, SequenceChangeImpact, SequenceFrameEvaluator, SequenceRenderCache,
};
use dawn_app_core::preview_session::{
    AudioPlaybackStatus, PreviewRenderKind, PreviewRenderRequest, PreviewRenderResult,
    PreviewRenderTiming, PreviewSnapshot, PreviewTransportState, SequenceKey,
};
use dawn_project::analysis::ProjectAnalysis;
use dawn_project::document::SequenceDocument;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::app_runtime::{apply_audio_clock_to_model, emit_preview_state_snapshot};
use crate::audio_runtime::AudioClock;
use crate::state::{
    lock_audio_runtime, lock_live_output, lock_model, lock_preview_transport, AppState,
    CommandResult,
};
use crate::window_layout::{persist_window_layout, WorkbenchWindow};

const PREVIEW_STATE_EVENT_INTERVAL: Duration = Duration::from_millis(33);

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewStateEventDto {
    pub source_label: String,
    pub transport_state: PreviewTransportState,
    pub preview_updating: bool,
    pub position_seconds: f64,
    pub home_seconds: f64,
    pub duration_seconds: f64,
    pub audio: Option<dawn_app_core::dto::SequenceAudioDto>,
    pub clock_source: String,
    pub audio_playback_status: AudioPlaybackStatus,
    pub frame_topology_identity: String,
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
    pub loop_total_ms: f64,
    pub loop_accounted_ms: f64,
    pub loop_unaccounted_ms: f64,
    pub sleep_actual_ms: f64,
    pub preview_transport_lock_ms: f64,
    pub preview_publish_lock_ms: f64,
    pub live_output_lock_ms: f64,
    pub model_lock_wait_ms: f64,
    pub preview_snapshot_ms: f64,
    pub analysis_handle_ms: f64,
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
    pub frame_fixture_clone_ms: f64,
    pub frame_effect_loop_ms: f64,
    pub frame_output_ms: f64,
    pub publish_ms: f64,
    pub event_emit_ms: f64,
    pub live_output_ms: f64,
    pub rendered_active_effects: u32,
    pub rendered_sampled_pixels: u32,
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
            loop_total_ms: 0.0,
            loop_accounted_ms: 0.0,
            loop_unaccounted_ms: 0.0,
            sleep_actual_ms: 0.0,
            preview_transport_lock_ms: 0.0,
            preview_publish_lock_ms: 0.0,
            live_output_lock_ms: 0.0,
            model_lock_wait_ms: 0.0,
            preview_snapshot_ms: 0.0,
            analysis_handle_ms: 0.0,
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
            frame_fixture_clone_ms: 0.0,
            frame_effect_loop_ms: 0.0,
            frame_output_ms: 0.0,
            publish_ms: 0.0,
            event_emit_ms: 0.0,
            live_output_ms: 0.0,
            rendered_active_effects: 0,
            rendered_sampled_pixels: 0,
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

#[derive(Debug, Default)]
struct DeferredPreviewRenderer {
    sequence_cache: SequenceRenderCache,
    render_cache: Option<SequenceFrameEvaluator>,
    previous_key: Option<SequenceKey>,
    previous_document: Option<Arc<SequenceDocument>>,
}

impl DeferredPreviewRenderer {
    fn render(
        &mut self,
        analysis: Option<&ProjectAnalysis>,
        request: PreviewRenderRequest,
    ) -> Option<PreviewRenderResult> {
        let mut timing = PreviewRenderTiming::default();
        if request.cancellation.is_cancelled() {
            return None;
        }
        let Some(analysis) = analysis else {
            let frame = empty_frame(request.generation, "No project analysis");
            return Some(PreviewRenderResult {
                request,
                frame,
                timing,
            });
        };
        let invalidation_started = Instant::now();
        self.apply_request_cache_invalidation(analysis, &request);
        timing.render_invalidation_ms = elapsed_ms(invalidation_started);
        let cache_started = Instant::now();
        let cached_renderer = self.cached_renderer(analysis, &request.document, &request);
        timing.render_cache_ms = elapsed_ms(cache_started);
        let frame = match cached_renderer {
            Ok(Some((renderer, renderer_build_ms))) => {
                let evaluated = match &request.kind {
                    PreviewRenderKind::SequenceFrame {
                        position_seconds, ..
                    } => renderer.evaluate_timed_cancellable(
                        *position_seconds,
                        request.generation,
                        || request.cancellation.is_cancelled(),
                    ),
                    PreviewRenderKind::EffectPreview {
                        preview_seconds,
                        ids,
                    } => renderer.evaluate_effect_preview_filtered_timed_cancellable(
                        *preview_seconds,
                        request.generation,
                        Some(ids),
                        || request.cancellation.is_cancelled(),
                    ),
                };
                let (frame, evaluation_timing) = evaluated?;
                timing.apply_evaluation(renderer_build_ms, evaluation_timing);
                frame
            }
            Ok(None) => return None,
            Err(message) => empty_frame(request.generation, message),
        };
        if request.cancellation.is_cancelled() {
            return None;
        }
        let result_started = Instant::now();
        self.previous_key = Some(request.key.clone());
        self.previous_document = Some(request.document.clone());
        timing.render_result_ms = elapsed_ms(result_started);
        Some(PreviewRenderResult {
            request,
            frame,
            timing,
        })
    }

    fn apply_request_cache_invalidation(
        &mut self,
        analysis: &ProjectAnalysis,
        request: &PreviewRenderRequest,
    ) {
        if self.previous_key.as_ref() != Some(&request.key) {
            self.sequence_cache.clear();
            self.render_cache = None;
            return;
        }
        let Some(previous_document) = self.previous_document.as_ref() else {
            return;
        };
        if Arc::ptr_eq(previous_document, &request.document) {
            return;
        }
        let impact = SequenceChangeImpact::between(previous_document, &request.document, analysis);
        if impact.requires_full_clear()
            || !impact.invalidated_prepared_effect_ids().is_empty()
            || !impact.invalidated_topology_effect_ids().is_empty()
        {
            self.sequence_cache.apply_change_impact(&impact);
            self.render_cache = None;
        }
    }

    fn cached_renderer(
        &mut self,
        analysis: &ProjectAnalysis,
        document: &SequenceDocument,
        request: &PreviewRenderRequest,
    ) -> Result<Option<(&mut SequenceFrameEvaluator, f64)>, String> {
        let mut renderer_build_ms = 0.0;
        if self.render_cache.is_none() {
            let build_started = Instant::now();
            let Some((renderer, _)) =
                self.sequence_cache
                    .build_evaluator_cancellable(analysis, document, || {
                        request.cancellation.is_cancelled()
                    })?
            else {
                return Ok(None);
            };
            self.render_cache = Some(renderer);
            renderer_build_ms = elapsed_ms(build_started);
        }
        Ok(self
            .render_cache
            .as_mut()
            .map(|renderer| (renderer, renderer_build_ms)))
    }
}

pub(crate) fn start_preview_worker(app: AppHandle) {
    thread::spawn(move || {
        let worker_started = Instant::now();
        let sleeper = spin_sleep::SpinSleeper::default();
        let mut deferred_renderer = DeferredPreviewRenderer::default();
        let mut last_published_generation: Option<u64> = None;
        let mut had_sink = false;
        let mut last_event_at = Instant::now() - PREVIEW_STATE_EVENT_INTERVAL;
        let mut last_event_identity: Option<PreviewEventIdentity> = None;
        let mut last_event_emit_ms = 0.0;
        let mut previous_loop_started = worker_started;
        loop {
            let state = app.state::<AppState>();
            let started = Instant::now();
            let loop_interval_ms =
                started.duration_since(previous_loop_started).as_secs_f64() * 1000.0;
            previous_loop_started = started;
            let mut timing = PreviewTimingDto::empty(worker_started.elapsed().as_secs_f64());
            timing.loop_interval_ms = loop_interval_ms;

            let preview_transport_lock_started = Instant::now();
            let has_sink = lock_preview_transport(&state)
                .map(|runtime| runtime.has_sinks())
                .unwrap_or(false);
            timing.preview_transport_lock_ms = elapsed_ms(preview_transport_lock_started);
            let live_output_lock_started = Instant::now();
            let live_output_enabled = lock_live_output(&state)
                .map(|runtime| runtime.enabled())
                .unwrap_or(false);
            timing.live_output_lock_ms = elapsed_ms(live_output_lock_started);
            if has_sink && !had_sink {
                last_published_generation = None;
            }
            had_sink = has_sink;

            timing.has_sink = has_sink;
            timing.event_emit_ms = last_event_emit_ms;
            let audio_poll_started = Instant::now();
            let audio_clock = lock_audio_runtime(&state)
                .ok()
                .and_then(|runtime| runtime.clock().ok());
            timing.audio_poll_ms = elapsed_ms(audio_poll_started);
            timing.audio_position_seconds =
                audio_clock.as_ref().map(|clock| clock.position_seconds);
            let model_lock_started = Instant::now();
            let (mut snapshot, mut target_fps, mut analysis, deferred_request) =
                match lock_model(&state) {
                    Ok(mut model) => {
                        timing.model_lock_wait_ms = elapsed_ms(model_lock_started);
                        let model_started = Instant::now();
                        let preview_snapshot_started = Instant::now();
                        let preview_snapshot = model.preview.snapshot();
                        timing.preview_snapshot_ms += elapsed_ms(preview_snapshot_started);
                        if let Some(clock) = audio_clock.as_ref() {
                            if !should_apply_audio_clock_to_model(&preview_snapshot, clock) {
                                model.tick_preview();
                            } else {
                                let analysis_handle_started = Instant::now();
                                let analysis = model.analysis.clone();
                                timing.analysis_handle_ms += elapsed_ms(analysis_handle_started);
                                let apply_started = Instant::now();
                                apply_audio_clock_to_model(&mut model, clock, analysis.as_deref());
                                timing.audio_apply_ms = elapsed_ms(apply_started);
                            }
                        } else {
                            model.tick_preview();
                        };
                        let preview_snapshot_started = Instant::now();
                        let snapshot = model.preview.snapshot();
                        timing.preview_snapshot_ms += elapsed_ms(preview_snapshot_started);
                        let deferred_request = if snapshot.preview_updating {
                            model.begin_deferred_preview_render()
                        } else {
                            None
                        };
                        timing.rendered_frame = deferred_request.is_some();
                        timing.model_update_ms = model_started.elapsed().as_secs_f64() * 1000.0;
                        let analysis_handle_started = Instant::now();
                        let analysis = model.analysis.clone();
                        timing.analysis_handle_ms += elapsed_ms(analysis_handle_started);
                        (
                            snapshot,
                            model.preview_target_fps(),
                            analysis,
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
                let result = deferred_renderer.render(analysis.as_deref(), request);
                timing.render_wall_ms = elapsed_ms(render_started);
                if let Some(result) = result {
                    record_render_timing(&mut timing, result.timing);
                    timing.render_overhead_ms = (timing.render_wall_ms - timing.render_ms).max(0.0);
                    timing.rendered_frame = true;
                    let model_lock_started = Instant::now();
                    if let Ok(mut model) = lock_model(&state) {
                        timing.model_lock_wait_ms += elapsed_ms(model_lock_started);
                        let model_started = Instant::now();
                        let _completed = model.complete_deferred_preview_render(result);
                        let preview_snapshot_started = Instant::now();
                        snapshot = model.preview.snapshot();
                        timing.preview_snapshot_ms += elapsed_ms(preview_snapshot_started);
                        let analysis_handle_started = Instant::now();
                        analysis = model.analysis.clone();
                        timing.analysis_handle_ms += elapsed_ms(analysis_handle_started);
                        target_fps = model.preview_target_fps();
                        timing.model_update_ms += elapsed_ms(model_started);
                    }
                }
            }
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
                && (snapshot.transport_state.should_publish_continuously()
                    || last_published_generation != Some(frame_generation));
            if should_publish_frame {
                let preview_publish_lock_started = Instant::now();
                if let Ok(mut runtime) = lock_preview_transport(&state) {
                    timing.preview_publish_lock_ms = elapsed_ms(preview_publish_lock_started);
                    let publish_started = Instant::now();
                    runtime.publish_frame(
                        &snapshot.frame,
                        snapshot.transport_state.should_publish_continuously(),
                        backend_seconds,
                    );
                    timing.publish_ms = publish_started.elapsed().as_secs_f64() * 1000.0;
                    timing.published_frame = true;
                    last_published_generation = Some(frame_generation);
                } else {
                    timing.preview_publish_lock_ms = elapsed_ms(preview_publish_lock_started);
                }
            }
            if live_output_enabled {
                let live_output_started = Instant::now();
                publish_live_output_frame(&app, &state, analysis.clone(), &snapshot.frame);
                timing.live_output_ms = elapsed_ms(live_output_started);
            }

            let event_identity = PreviewEventIdentity::from(&snapshot);
            let should_emit_event = if snapshot.transport_state.should_publish_continuously() {
                last_event_identity.as_ref() != Some(&event_identity)
                    || last_event_at.elapsed() >= PREVIEW_STATE_EVENT_INTERVAL
            } else {
                last_event_identity.as_ref() != Some(&event_identity)
            };
            let fps = if has_sink || live_output_enabled {
                target_fps.max(1)
            } else if snapshot.transport_state.should_publish_continuously() {
                target_fps.clamp(1, 30)
            } else {
                10
            };
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
                emit_preview_state_snapshot(&app, &snapshot, timing);
                last_event_emit_ms = elapsed_ms(event_emit_started);
                last_event_at = Instant::now();
                last_event_identity = Some(event_identity);
            }
        }
    });
}

fn record_render_timing(timing: &mut PreviewTimingDto, render_timing: PreviewRenderTiming) {
    timing.render_ms = render_timing.total_ms;
    timing.renderer_build_ms = render_timing.renderer_build_ms;
    timing.frame_evaluate_ms = render_timing.frame_evaluate_ms;
    timing.frame_fixture_clone_ms = render_timing.fixture_clone_ms;
    timing.frame_effect_loop_ms = render_timing.effect_loop_ms;
    timing.frame_output_ms = render_timing.output_frame_ms;
    timing.rendered_active_effects = render_timing.active_effects;
    timing.rendered_sampled_pixels = render_timing.sampled_pixels;
    timing.render_invalidation_ms = render_timing.render_invalidation_ms;
    timing.render_cache_ms = render_timing.render_cache_ms;
    timing.render_result_ms = render_timing.render_result_ms;
}

fn accounted_loop_ms(timing: &PreviewTimingDto) -> f64 {
    timing.preview_transport_lock_ms
        + timing.live_output_lock_ms
        + timing.audio_poll_ms
        + timing.model_lock_wait_ms
        + timing.model_update_ms
        + timing.render_wall_ms
        + timing.preview_publish_lock_ms
        + timing.publish_ms
        + timing.live_output_ms
        + timing.event_emit_ms
        + timing.sleep_actual_ms
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn should_apply_audio_clock_to_model(
    preview_snapshot: &PreviewSnapshot,
    clock: &AudioClock,
) -> bool {
    preview_snapshot.transport_state.is_active_playback()
        || matches!(
            clock.status,
            AudioPlaybackStatus::LoadingToPlay
                | AudioPlaybackStatus::Playing
                | AudioPlaybackStatus::Ended
        )
        || clock.ended
        || preview_snapshot.audio_playback_status == AudioPlaybackStatus::LoadingToPlay
}

fn publish_live_output_frame(
    app: &AppHandle,
    state: &State<'_, AppState>,
    analysis: Option<Arc<ProjectAnalysis>>,
    frame: &OutputFrame,
) {
    let snapshot = match lock_live_output(state) {
        Ok(mut runtime) => runtime.send_frame(analysis, frame),
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
        topology_identity: frame.topology_identity.stable_key(),
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
    transport_state: PreviewTransportState,
    preview_updating: bool,
    position_seconds: f64,
    home_seconds: f64,
    duration_seconds: f64,
    audio_path: Option<String>,
    audio_exists: bool,
    clock_source: String,
    audio_playback_status: AudioPlaybackStatus,
    frame_topology_identity: String,
    status: String,
}

impl From<&PreviewSnapshot> for PreviewEventIdentity {
    fn from(snapshot: &PreviewSnapshot) -> Self {
        Self {
            source_label: snapshot.source_label.clone(),
            transport_state: snapshot.transport_state,
            preview_updating: snapshot.preview_updating,
            position_seconds: if snapshot.transport_state.should_publish_continuously() {
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
            audio_playback_status: snapshot.audio_playback_status,
            frame_topology_identity: snapshot.frame.topology_identity.stable_key(),
            status: snapshot.status.clone(),
        }
    }
}
