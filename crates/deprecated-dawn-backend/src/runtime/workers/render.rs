use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use dawn_language::analysis::ProjectAnalysis;
use dawn_language::document::SequenceDocument;
use dawn_language::model::Color;

use crate::output::sequence::{
    empty_frame, SequenceChangeImpact, SequenceEffectThumbnailResult, SequenceFrameEvaluator,
    SequenceRenderCache, SequenceRenderCache as RenderCache,
};
use crate::preview::session::{
    PreviewRenderRequest, PreviewRenderResult, PreviewRenderTiming, SequenceKey,
};

const EFFECT_PREVIEW_MAX_COLUMNS: usize = 360;
const EFFECT_PREVIEW_MAX_ROWS: usize = 50;

#[derive(Debug, Clone)]
pub struct SequenceEffectPreviewRequestEffect {
    pub effect_id: u32,
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct SequenceEffectPreview {
    pub effect_id: u32,
    pub duration_seconds: f64,
    pub source_pixel_count: u32,
    pub sampled_pixel_indices: Vec<u32>,
    pub columns: u32,
    pub rows: u32,
    pub colors: Vec<Color>,
}

#[derive(Debug, Clone)]
pub enum SequenceEffectPreviewResult {
    Ready(SequenceEffectPreviewReadyResult),
    Unavailable(SequenceEffectPreviewUnavailableResult),
    Error(SequenceEffectPreviewErrorResult),
}

#[derive(Debug, Clone)]
pub struct SequenceEffectPreviewReadyResult {
    pub request_id: u32,
    pub signature: String,
    pub preview: SequenceEffectPreview,
}

#[derive(Debug, Clone)]
pub struct SequenceEffectPreviewUnavailableResult {
    pub request_id: u32,
    pub effect_id: u32,
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct SequenceEffectPreviewErrorResult {
    pub request_id: u32,
    pub effect_id: u32,
    pub signature: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SequencePreviewDocumentKey {
    path: String,
    object_key: String,
}

impl SequencePreviewDocumentKey {
    fn new(path: String, object_key: String) -> Self {
        Self { path, object_key }
    }
}

#[derive(Debug)]
pub(super) struct RenderWorker {
    effect_preview_sender: Sender<EffectPreviewJob>,
    preview_frame_sender: Sender<PreviewFrameJob>,
    latest_effect_preview_requests: Arc<Mutex<HashMap<SequencePreviewDocumentKey, u32>>>,
    completed_effect_previews:
        Arc<Mutex<HashMap<SequencePreviewDocumentKey, Vec<SequenceEffectPreviewResult>>>>,
    completed_preview_frames: Arc<Mutex<Vec<PreviewRenderResult>>>,
}

impl Default for RenderWorker {
    fn default() -> Self {
        let (effect_preview_sender, effect_preview_receiver) = mpsc::channel();
        let (preview_frame_sender, preview_frame_receiver) = mpsc::channel();
        let latest_effect_preview_requests = Arc::new(Mutex::new(HashMap::new()));
        let completed_effect_previews = Arc::new(Mutex::new(HashMap::new()));
        let completed_preview_frames = Arc::new(Mutex::new(Vec::new()));
        start_effect_preview_worker(
            effect_preview_receiver,
            latest_effect_preview_requests.clone(),
            completed_effect_previews.clone(),
        );
        start_preview_frame_worker(preview_frame_receiver, completed_preview_frames.clone());
        Self {
            effect_preview_sender,
            preview_frame_sender,
            latest_effect_preview_requests,
            completed_effect_previews,
            completed_preview_frames,
        }
    }
}

impl RenderWorker {
    pub(super) fn request_preview_frame(
        &self,
        analysis: Option<ProjectAnalysis>,
        request: PreviewRenderRequest,
    ) -> Result<(), String> {
        self.preview_frame_sender
            .send(PreviewFrameJob { analysis, request })
            .map_err(|_| "render worker is not available".to_string())
    }

    pub(super) fn drain(&self) -> Vec<RenderWorkerResult> {
        let completed_preview_frames = self
            .completed_preview_frames
            .lock()
            .map(|mut completed| completed.drain(..).collect::<Vec<_>>())
            .unwrap_or_default();
        completed_preview_frames
            .into_iter()
            .map(RenderWorkerResult::PreviewFrame)
            .collect()
    }

    pub(super) fn request_effect_previews(
        &self,
        path: String,
        object_key: String,
        request_id: u32,
        effects: Vec<SequenceEffectPreviewRequestEffect>,
        analysis: ProjectAnalysis,
        document: SequenceDocument,
    ) -> Result<(), String> {
        let key = SequencePreviewDocumentKey::new(path, object_key);
        self.latest_effect_preview_requests
            .lock()
            .map_err(|_| "effect preview request state lock is poisoned".to_string())?
            .insert(key.clone(), request_id);
        self.effect_preview_sender
            .send(EffectPreviewJob {
                key,
                request_id,
                effects,
                analysis,
                document,
            })
            .map_err(|_| "render worker is not available".to_string())
    }

    pub(super) fn take_effect_preview_results(
        &self,
        path: String,
        object_key: String,
    ) -> Result<Vec<SequenceEffectPreviewResult>, String> {
        let key = SequencePreviewDocumentKey::new(path, object_key);
        Ok(self
            .completed_effect_previews
            .lock()
            .map_err(|_| "effect preview result state lock is poisoned".to_string())?
            .remove(&key)
            .unwrap_or_default())
    }
}

#[derive(Debug)]
pub(crate) enum RenderWorkerResult {
    PreviewFrame(PreviewRenderResult),
}

struct PreviewFrameJob {
    analysis: Option<ProjectAnalysis>,
    request: PreviewRenderRequest,
}

struct EffectPreviewJob {
    key: SequencePreviewDocumentKey,
    request_id: u32,
    effects: Vec<SequenceEffectPreviewRequestEffect>,
    analysis: ProjectAnalysis,
    document: SequenceDocument,
}

#[derive(Debug, Default)]
struct DeferredPreviewRenderer {
    sequence_cache: SequenceRenderCache,
    render_cache: Option<SequenceFrameEvaluator>,
    previous_key: Option<SequenceKey>,
    previous_document: Option<SequenceDocument>,
}

impl DeferredPreviewRenderer {
    fn render(
        &mut self,
        analysis: Option<&ProjectAnalysis>,
        request: PreviewRenderRequest,
    ) -> PreviewRenderResult {
        let Some(analysis) = analysis else {
            let frame = empty_frame(request.generation, "No project analysis");
            return PreviewRenderResult {
                request,
                frame,
                timing: PreviewRenderTiming::default(),
            };
        };
        self.apply_request_cache_invalidation(analysis, &request);
        let mut timing = PreviewRenderTiming::default();
        let frame = match self.cached_renderer(analysis, &request.document) {
            Ok((renderer, renderer_build_ms)) => {
                let (frame, evaluation_timing) =
                    renderer.evaluate_timed(request.position_seconds, request.generation);
                timing = PreviewRenderTiming::from_evaluation(renderer_build_ms, evaluation_timing);
                frame
            }
            Err(message) => empty_frame(request.generation, message),
        };
        self.previous_key = Some(request.key.clone());
        self.previous_document = Some(request.document.clone());
        PreviewRenderResult {
            request,
            frame,
            timing,
        }
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
    ) -> Result<(&mut SequenceFrameEvaluator, f64), String> {
        let mut renderer_build_ms = 0.0;
        if self.render_cache.is_none() {
            let build_started = Instant::now();
            let (renderer, _) = self.sequence_cache.build_evaluator(analysis, document)?;
            self.render_cache = Some(renderer);
            renderer_build_ms = elapsed_ms(build_started);
        }
        self.render_cache
            .as_mut()
            .map(|renderer| (renderer, renderer_build_ms))
            .ok_or_else(|| "Sequence preview renderer was not prepared".to_string())
    }
}

fn start_preview_frame_worker(
    receiver: Receiver<PreviewFrameJob>,
    completed: Arc<Mutex<Vec<PreviewRenderResult>>>,
) {
    thread::spawn(move || {
        let mut renderer = DeferredPreviewRenderer::default();
        while let Ok(job) = receiver.recv() {
            let result = renderer.render(job.analysis.as_ref(), job.request);
            if let Ok(mut completed) = completed.lock() {
                completed.push(result);
            }
        }
    });
}

fn start_effect_preview_worker(
    receiver: Receiver<EffectPreviewJob>,
    latest_requests: Arc<Mutex<HashMap<SequencePreviewDocumentKey, u32>>>,
    completed: Arc<Mutex<HashMap<SequencePreviewDocumentKey, Vec<SequenceEffectPreviewResult>>>>,
) {
    thread::spawn(move || {
        let mut cache = RenderCache::default();
        let mut pending = HashMap::<SequencePreviewDocumentKey, EffectPreviewJob>::new();
        loop {
            if pending.is_empty() {
                let Ok(job) = receiver.recv() else {
                    break;
                };
                pending.insert(job.key.clone(), job);
            }
            while let Ok(job) = receiver.try_recv() {
                pending.insert(job.key.clone(), job);
            }
            let Some(key) = pending.keys().next().cloned() else {
                continue;
            };
            let Some(job) = pending.remove(&key) else {
                continue;
            };
            if !is_latest_request(&latest_requests, &job.key, job.request_id) {
                continue;
            }
            process_effect_preview_job(&mut cache, &latest_requests, &completed, job);
        }
    });
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn process_effect_preview_job(
    cache: &mut SequenceRenderCache,
    latest_requests: &Arc<Mutex<HashMap<SequencePreviewDocumentKey, u32>>>,
    completed: &Arc<Mutex<HashMap<SequencePreviewDocumentKey, Vec<SequenceEffectPreviewResult>>>>,
    job: EffectPreviewJob,
) {
    for requested in job.effects {
        if !is_latest_request(latest_requests, &job.key, job.request_id) {
            return;
        }
        let Some(effect) = job
            .document
            .effects
            .iter()
            .find(|effect| effect.id == requested.effect_id)
        else {
            push_result(
                completed,
                &job.key,
                SequenceEffectPreviewResult::Unavailable(SequenceEffectPreviewUnavailableResult {
                    request_id: job.request_id,
                    effect_id: requested.effect_id,
                    signature: requested.signature,
                }),
            );
            continue;
        };
        let result = cache.effect_thumbnail_cancellable(
            &job.analysis,
            &job.document,
            effect,
            EFFECT_PREVIEW_MAX_COLUMNS,
            EFFECT_PREVIEW_MAX_ROWS,
            || !is_latest_request(latest_requests, &job.key, job.request_id),
        );
        match result {
            Ok(SequenceEffectThumbnailResult::Ready(thumbnail)) => {
                push_result(
                    completed,
                    &job.key,
                    SequenceEffectPreviewResult::Ready(SequenceEffectPreviewReadyResult {
                        request_id: job.request_id,
                        signature: requested.signature,
                        preview: SequenceEffectPreview {
                            effect_id: thumbnail.effect_id,
                            duration_seconds: thumbnail.duration_seconds,
                            source_pixel_count: thumbnail.source_pixel_count,
                            sampled_pixel_indices: thumbnail.sampled_pixel_indices,
                            columns: thumbnail.columns,
                            rows: thumbnail.rows,
                            colors: thumbnail.colors,
                        },
                    }),
                );
            }
            Ok(SequenceEffectThumbnailResult::Unavailable) => {
                push_result(
                    completed,
                    &job.key,
                    SequenceEffectPreviewResult::Unavailable(
                        SequenceEffectPreviewUnavailableResult {
                            request_id: job.request_id,
                            effect_id: requested.effect_id,
                            signature: requested.signature,
                        },
                    ),
                );
            }
            Ok(SequenceEffectThumbnailResult::Cancelled) => return,
            Err(message) => {
                push_result(
                    completed,
                    &job.key,
                    SequenceEffectPreviewResult::Error(SequenceEffectPreviewErrorResult {
                        request_id: job.request_id,
                        effect_id: requested.effect_id,
                        signature: requested.signature,
                        message,
                    }),
                );
            }
        }
    }
}

fn is_latest_request(
    latest_requests: &Arc<Mutex<HashMap<SequencePreviewDocumentKey, u32>>>,
    key: &SequencePreviewDocumentKey,
    request_id: u32,
) -> bool {
    latest_requests
        .lock()
        .map(|latest| latest.get(key).copied() == Some(request_id))
        .unwrap_or(false)
}

fn push_result(
    completed: &Arc<Mutex<HashMap<SequencePreviewDocumentKey, Vec<SequenceEffectPreviewResult>>>>,
    key: &SequencePreviewDocumentKey,
    result: SequenceEffectPreviewResult,
) {
    if let Ok(mut completed) = completed.lock() {
        completed.entry(key.clone()).or_default().push(result);
    }
}
