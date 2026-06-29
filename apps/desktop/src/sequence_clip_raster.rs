use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::mpsc;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::thread;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use dawn_language::effect::{
    CurveDefinition, CurveId, CurveSource, EffectDefinition, EffectInst, EffectInstId,
    EffectParamValue,
};
use dawn_language::effect_dsl::EffectKind;
use dawn_language::model::DawnProject;
use dawn_language::sequence::{MarkCollectionKey, Sequence, SequenceId};
use dawn_language::setup::SetupId;
use dawn_language::values::{Color, DawnTime};
use dawn_runtime::{
    EffectRasterPrepareBatch, PreparedEffectRasterRenderer, RenderedTargetPixelAddress,
    resolve_effect_target_pixel_addresses,
};

use crate::dto::{
    GuiDocumentRequest, SequenceClipRaster, SequenceClipRasterError, SequenceClipRasterRequest,
    SequenceClipRasterResponse, SequenceClipRasterResultBatch, SequenceClipRasterUnavailable,
};

pub struct SequenceClipRasterService {
    sender: mpsc::Sender<RasterJob>,
    receiver: mpsc::Receiver<RasterWorkerResult>,
    cache: HashMap<RasterCacheKey, CachedRasterResult>,
    pending_results: HashMap<RasterDocumentKey, VecDeque<RasterResultEntry>>,
    active: Option<ActiveRasterJob>,
    next_job_id: u32,
    latest_job_id: Arc<AtomicU64>,
}

impl SequenceClipRasterService {
    pub fn new() -> Self {
        let (request_sender, request_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();
        let latest_job_id = Arc::new(AtomicU64::new(0));
        thread::spawn({
            let latest_job_id = Arc::clone(&latest_job_id);
            move || raster_worker(request_receiver, result_sender, latest_job_id)
        });
        Self {
            sender: request_sender,
            receiver: result_receiver,
            cache: HashMap::new(),
            pending_results: HashMap::new(),
            active: None,
            next_job_id: 1,
            latest_job_id,
        }
    }

    pub fn request(
        &mut self,
        project_revision: u32,
        project: Option<DawnProject>,
        setup_id: Option<SetupId>,
        sequence_id: Option<SequenceId>,
        request: SequenceClipRasterRequest,
    ) -> SequenceClipRasterResponse {
        let document_key = RasterDocumentKey::new(&request.document);
        let base_key = RasterRequestKey {
            path: request.document.path.clone(),
            object_key: request.document.object_key.clone(),
            column_stride_frames: request.column_stride_frames,
            display_row_count: request.display_row_count,
        };
        self.drain_results();
        self.prepare_response_and_schedule(RasterScheduleInput {
            project_revision,
            project,
            setup_id,
            sequence_id,
            document_key,
            base_key,
            ordered_items: request.items,
        })
    }

    fn drain_results(&mut self) {
        while let Ok(result) = self.receiver.try_recv() {
            match result {
                RasterWorkerResult::Raster {
                    job_id,
                    request_id,
                    document_key,
                    cache_key,
                    signature,
                    signature_key,
                    raster,
                } => {
                    let result_raster = raster.clone();
                    self.cache.insert(
                        cache_key,
                        CachedRasterResult {
                            signature: signature.clone(),
                            result: CachedRasterValue::Raster(raster),
                        },
                    );
                    if self.is_current_request(&document_key, request_id) {
                        self.push_result(
                            document_key,
                            RasterResultEntry::Ready {
                                request_id,
                                signature: signature_key,
                                raster: result_raster,
                            },
                        );
                    }
                    self.finish_active_work_item(job_id);
                }
                RasterWorkerResult::Error {
                    job_id,
                    request_id,
                    document_key,
                    cache_key,
                    signature,
                    signature_key,
                    error,
                } => {
                    let result_error = error.clone();
                    self.cache.insert(
                        cache_key,
                        CachedRasterResult {
                            signature: signature.clone(),
                            result: CachedRasterValue::Error(error),
                        },
                    );
                    if self.is_current_request(&document_key, request_id) {
                        self.push_result(
                            document_key,
                            RasterResultEntry::Error {
                                request_id,
                                signature: signature_key,
                                error: result_error,
                            },
                        );
                    }
                    self.finish_active_work_item(job_id);
                }
                RasterWorkerResult::Complete {
                    job_id,
                    request_id,
                    document_key,
                } => {
                    let current_request = self.is_current_request(&document_key, request_id);
                    if self
                        .active
                        .as_ref()
                        .is_some_and(|active| active.job_id == job_id)
                    {
                        self.active = None;
                    }
                    if current_request {
                        self.push_result(document_key, RasterResultEntry::Complete { request_id });
                    }
                }
            }
        }
    }

    pub fn take_results(
        &mut self,
        project_revision: u32,
        request: GuiDocumentRequest,
        request_id: u32,
    ) -> SequenceClipRasterResultBatch {
        self.drain_results();
        let document_key = RasterDocumentKey::new(&request);
        let mut ready = Vec::new();
        let mut unavailable = Vec::new();
        let mut errors = Vec::new();
        let mut complete = self
            .active
            .as_ref()
            .is_none_or(|active| active.request_id != request_id);

        if let Some(entries) = self.pending_results.get_mut(&document_key) {
            let mut retained = VecDeque::new();
            while let Some(entry) = entries.pop_front() {
                match entry {
                    RasterResultEntry::Ready {
                        request_id: entry_request_id,
                        signature,
                        mut raster,
                    } if entry_request_id == request_id => {
                        raster.request_id = entry_request_id;
                        raster.signature = signature;
                        ready.push(raster);
                    }
                    RasterResultEntry::Unavailable {
                        request_id: entry_request_id,
                        effect_id,
                        signature,
                    } if entry_request_id == request_id => {
                        unavailable.push(SequenceClipRasterUnavailable {
                            request_id: entry_request_id,
                            effect_id,
                            signature,
                        });
                    }
                    RasterResultEntry::Error {
                        request_id: entry_request_id,
                        signature,
                        mut error,
                    } if entry_request_id == request_id => {
                        error.request_id = entry_request_id;
                        error.signature = signature;
                        errors.push(error);
                    }
                    RasterResultEntry::Complete {
                        request_id: entry_request_id,
                    } if entry_request_id == request_id => {
                        complete = true;
                    }
                    entry => retained.push_back(entry),
                }
            }
            *entries = retained;
        }

        SequenceClipRasterResultBatch {
            project_revision,
            request_id,
            ready,
            unavailable,
            errors,
            complete,
        }
    }

    fn prepare_response_and_schedule(
        &mut self,
        input: RasterScheduleInput,
    ) -> SequenceClipRasterResponse {
        let RasterScheduleInput {
            project_revision,
            project,
            setup_id,
            sequence_id,
            document_key,
            base_key,
            ordered_items,
        } = input;
        let request_id = self.next_job_id;
        self.next_job_id += 1;
        let Some(project) = project else {
            self.active = None;
            return empty_response(project_revision, request_id, true);
        };
        let (Some(setup_id), Some(sequence_id)) = (setup_id, sequence_id) else {
            self.active = None;
            return empty_response(project_revision, request_id, true);
        };

        let Some(sequence) = project.sequences.get(&sequence_id) else {
            self.active = None;
            return empty_response(project_revision, request_id, true);
        };

        let mut work_items = Vec::new();
        let requested_ids = ordered_items
            .iter()
            .map(|item| item.effect_id)
            .collect::<Vec<_>>();
        let ordered_ids = ordered_existing_effect_ids(&sequence.effects, &requested_ids);
        let client_signatures = ordered_items
            .iter()
            .map(|item| (item.effect_id, item.signature.clone()))
            .collect::<HashMap<_, _>>();
        let current_ids = sequence
            .effects
            .iter()
            .map(|effect| effect.id.0)
            .collect::<HashSet<_>>();
        self.cache.retain(|key, _| {
            !key.matches_request(&base_key) || current_ids.contains(&key.effect_id)
        });
        self.pending_results
            .insert(document_key.clone(), VecDeque::new());
        for effect_id in requested_ids {
            if !current_ids.contains(&effect_id) {
                self.push_result(
                    document_key.clone(),
                    RasterResultEntry::Unavailable {
                        request_id,
                        effect_id,
                        signature: signature_key(&RenderInputSignature::Invalid {
                            message: "effect unavailable".to_string(),
                        }),
                    },
                );
            }
        }

        for effect_id in ordered_ids {
            let Some(effect) = sequence
                .effects
                .iter()
                .find(|effect| effect.id == EffectInstId(effect_id))
            else {
                continue;
            };
            let cache_key = RasterCacheKey::new(&base_key, effect_id);
            let signature = match render_signature(&project, &setup_id, sequence, effect) {
                Ok(signature) => signature,
                Err(message) => RenderInputSignature::Invalid { message },
            };
            let signature_key = signature_key(&signature);
            if client_signatures
                .get(&effect_id)
                .and_then(|signature| signature.as_ref())
                .is_some_and(|client_signature| client_signature == &signature_key)
            {
                continue;
            }
            match self.cache.get(&cache_key) {
                Some(entry) if entry.signature == signature => match &entry.result {
                    CachedRasterValue::Raster(raster) => self.push_result(
                        document_key.clone(),
                        RasterResultEntry::Ready {
                            request_id,
                            signature: signature_key,
                            raster: raster.clone(),
                        },
                    ),
                    CachedRasterValue::Error(error) => self.push_result(
                        document_key.clone(),
                        RasterResultEntry::Error {
                            request_id,
                            signature: signature_key,
                            error: error.clone(),
                        },
                    ),
                },
                Some(_) => {
                    self.cache.remove(&cache_key);
                    work_items.push(RasterWorkItem {
                        cache_key,
                        signature,
                        signature_key,
                        effect_id,
                    });
                }
                None => work_items.push(RasterWorkItem {
                    cache_key,
                    signature,
                    signature_key,
                    effect_id,
                }),
            }
        }

        self.schedule_missing_work(
            request_id,
            document_key,
            project,
            setup_id,
            sequence_id,
            work_items,
        );
        SequenceClipRasterResponse {
            project_revision,
            request_id,
            complete: self.active.is_none(),
        }
    }

    fn schedule_missing_work(
        &mut self,
        request_id: u32,
        document_key: RasterDocumentKey,
        project: DawnProject,
        setup_id: SetupId,
        sequence_id: SequenceId,
        work_items: Vec<RasterWorkItem>,
    ) {
        if work_items.is_empty() {
            self.active = None;
            self.push_result(document_key, RasterResultEntry::Complete { request_id });
            return;
        }
        let job_id = request_id;
        let job = RasterJob {
            job_id,
            request_id,
            document_key: document_key.clone(),
            project,
            setup_id,
            sequence_id,
            work_items: work_items.clone(),
        };
        if self.sender.send(job).is_ok() {
            self.latest_job_id
                .store(u64::from(job_id), Ordering::Relaxed);
            self.active = Some(ActiveRasterJob {
                job_id,
                request_id,
                document_key,
                work_items,
            });
        } else {
            self.active = None;
            self.push_result(document_key, RasterResultEntry::Complete { request_id });
        }
    }

    fn finish_active_work_item(&mut self, job_id: u32) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if active.job_id != job_id || active.work_items.is_empty() {
            return;
        }
        active.work_items.remove(0);
    }

    fn push_result(&mut self, document_key: RasterDocumentKey, result: RasterResultEntry) {
        self.pending_results
            .entry(document_key)
            .or_default()
            .push_back(result);
    }

    fn is_current_request(&self, document_key: &RasterDocumentKey, request_id: u32) -> bool {
        self.active.as_ref().is_some_and(|active| {
            &active.document_key == document_key && active.request_id == request_id
        })
    }
}

impl Default for SequenceClipRasterService {
    fn default() -> Self {
        Self::new()
    }
}

struct ActiveRasterJob {
    job_id: u32,
    request_id: u32,
    document_key: RasterDocumentKey,
    work_items: Vec<RasterWorkItem>,
}

struct RasterScheduleInput {
    project_revision: u32,
    project: Option<DawnProject>,
    setup_id: Option<SetupId>,
    sequence_id: Option<SequenceId>,
    document_key: RasterDocumentKey,
    base_key: RasterRequestKey,
    ordered_items: Vec<crate::dto::SequenceClipRasterRequestItem>,
}

#[derive(Clone, Debug, PartialEq)]
struct RasterDocumentKey {
    path: String,
    object_key: Option<String>,
}

impl RasterDocumentKey {
    fn new(request: &GuiDocumentRequest) -> Self {
        Self {
            path: request.path.clone(),
            object_key: request.object_key.clone(),
        }
    }
}

impl Eq for RasterDocumentKey {}

impl Hash for RasterDocumentKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.object_key.hash(state);
    }
}

#[derive(Clone, Debug, PartialEq)]
struct RasterRequestKey {
    path: String,
    object_key: Option<String>,
    column_stride_frames: u32,
    display_row_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RasterCacheKey {
    path: String,
    object_key: Option<String>,
    effect_id: u32,
    column_stride_frames: u32,
    display_row_count: u32,
}

impl RasterCacheKey {
    fn new(request: &RasterRequestKey, effect_id: u32) -> Self {
        Self {
            path: request.path.clone(),
            object_key: request.object_key.clone(),
            effect_id,
            column_stride_frames: request.column_stride_frames,
            display_row_count: request.display_row_count,
        }
    }

    fn matches_request(&self, request: &RasterRequestKey) -> bool {
        self.path == request.path
            && self.object_key == request.object_key
            && self.column_stride_frames == request.column_stride_frames
            && self.display_row_count == request.display_row_count
    }
}

impl Hash for RasterCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.object_key.hash(state);
        self.effect_id.hash(state);
        self.column_stride_frames.hash(state);
        self.display_row_count.hash(state);
    }
}

#[derive(Clone)]
struct CachedRasterResult {
    signature: RenderInputSignature,
    result: CachedRasterValue,
}

#[derive(Clone)]
enum CachedRasterValue {
    Raster(SequenceClipRaster),
    Error(SequenceClipRasterError),
}

struct RasterJob {
    job_id: u32,
    request_id: u32,
    document_key: RasterDocumentKey,
    project: DawnProject,
    setup_id: SetupId,
    sequence_id: SequenceId,
    work_items: Vec<RasterWorkItem>,
}

#[derive(Clone, Debug, PartialEq)]
struct RasterWorkItem {
    cache_key: RasterCacheKey,
    signature: RenderInputSignature,
    signature_key: String,
    effect_id: u32,
}

enum RasterResultEntry {
    Ready {
        request_id: u32,
        signature: String,
        raster: SequenceClipRaster,
    },
    Unavailable {
        request_id: u32,
        effect_id: u32,
        signature: String,
    },
    Error {
        request_id: u32,
        signature: String,
        error: SequenceClipRasterError,
    },
    Complete {
        request_id: u32,
    },
}

enum RasterWorkerResult {
    Raster {
        job_id: u32,
        request_id: u32,
        document_key: RasterDocumentKey,
        cache_key: RasterCacheKey,
        signature: RenderInputSignature,
        signature_key: String,
        raster: SequenceClipRaster,
    },
    Error {
        job_id: u32,
        request_id: u32,
        document_key: RasterDocumentKey,
        cache_key: RasterCacheKey,
        signature: RenderInputSignature,
        signature_key: String,
        error: SequenceClipRasterError,
    },
    Complete {
        job_id: u32,
        request_id: u32,
        document_key: RasterDocumentKey,
    },
}

fn raster_worker(
    receiver: mpsc::Receiver<RasterJob>,
    sender: mpsc::Sender<RasterWorkerResult>,
    latest_job_id: Arc<AtomicU64>,
) {
    let mut raster_cache = HashMap::<RasterRenderCacheKey, CachedRasterValue>::new();
    let mut renderer_cache = HashMap::<String, PreparedEffectRasterRenderer>::new();
    let mut job = match receiver.recv() {
        Ok(job) => job,
        Err(_) => return,
    };
    loop {
        prune_worker_raster_cache(&mut raster_cache, &job.work_items);
        prune_prepared_renderer_cache(&mut renderer_cache, &job.work_items);
        let mut completed = true;
        let work_items = std::mem::take(&mut job.work_items);
        let (mut prepare_batch, prepare_batch_error) = match EffectRasterPrepareBatch::prepare(
            &job.project,
            &job.setup_id,
            &job.sequence_id,
        ) {
            Ok(batch) => (Some(batch), None),
            Err(error) => (None, Some(format!("{error:?}"))),
        };
        for item in work_items {
            if latest_job_id.load(Ordering::Relaxed) != u64::from(job.job_id) {
                completed = false;
                break;
            }
            let render_cache_key = RasterRenderCacheKey::new(&item);
            let result = match raster_cache.get(&render_cache_key).cloned() {
                Some(CachedRasterValue::Raster(raster)) => RasterWorkerResult::Raster {
                    job_id: job.job_id,
                    request_id: job.request_id,
                    document_key: job.document_key.clone(),
                    cache_key: item.cache_key,
                    signature: item.signature,
                    signature_key: item.signature_key,
                    raster,
                },
                Some(CachedRasterValue::Error(error)) => RasterWorkerResult::Error {
                    job_id: job.job_id,
                    request_id: job.request_id,
                    document_key: job.document_key.clone(),
                    cache_key: item.cache_key,
                    signature: item.signature,
                    signature_key: item.signature_key,
                    error,
                },
                None => {
                    let should_continue =
                        || latest_job_id.load(Ordering::Relaxed) == u64::from(job.job_id);
                    let renderer = match renderer_cache.get(&item.signature_key).cloned() {
                        Some(renderer) => Ok(renderer),
                        None => match prepare_batch.as_mut() {
                            Some(batch) => {
                                match batch.prepare_effect(&EffectInstId(item.effect_id)) {
                                    Ok(renderer) => {
                                        renderer_cache
                                            .insert(item.signature_key.clone(), renderer.clone());
                                        Ok(renderer)
                                    }
                                    Err(error) => Err(format!("{error:?}")),
                                }
                            }
                            None => match &prepare_batch_error {
                                Some(message) => Err(message.clone()),
                                None => Err("raster prepare batch unavailable".to_string()),
                            },
                        },
                    };
                    match match renderer {
                        Ok(renderer) => render_effect_raster(RasterRenderRequest {
                            renderer,
                            effect_id: item.effect_id,
                            column_stride_frames: item.cache_key.column_stride_frames,
                            display_row_count: item.cache_key.display_row_count,
                            should_continue: &should_continue,
                        }),
                        Err(message) => Err(RasterRenderFailure::Error(message)),
                    } {
                        Ok(raster) => {
                            raster_cache.insert(
                                render_cache_key,
                                CachedRasterValue::Raster(raster.clone()),
                            );
                            RasterWorkerResult::Raster {
                                job_id: job.job_id,
                                request_id: job.request_id,
                                document_key: job.document_key.clone(),
                                cache_key: item.cache_key,
                                signature: item.signature,
                                signature_key: item.signature_key,
                                raster,
                            }
                        }
                        Err(RasterRenderFailure::Cancelled) => {
                            completed = false;
                            break;
                        }
                        Err(RasterRenderFailure::Error(message)) => {
                            let error = SequenceClipRasterError {
                                request_id: job.request_id,
                                effect_id: item.effect_id,
                                signature: item.signature_key.clone(),
                                message,
                            };
                            raster_cache
                                .insert(render_cache_key, CachedRasterValue::Error(error.clone()));
                            RasterWorkerResult::Error {
                                job_id: job.job_id,
                                request_id: job.request_id,
                                document_key: job.document_key.clone(),
                                cache_key: item.cache_key,
                                signature: item.signature,
                                signature_key: item.signature_key,
                                error,
                            }
                        }
                    }
                }
            };
            if sender.send(result).is_err() {
                return;
            }
            if let Some(next) = newest_queued_job(&receiver) {
                job = next;
                completed = false;
                break;
            }
        }
        if completed
            && sender
                .send(RasterWorkerResult::Complete {
                    job_id: job.job_id,
                    request_id: job.request_id,
                    document_key: job.document_key.clone(),
                })
                .is_err()
        {
            return;
        }
        if completed {
            job = match receiver.recv() {
                Ok(job) => job,
                Err(_) => return,
            };
        } else if let Some(next) = newest_queued_job(&receiver) {
            job = next;
        } else {
            job = match receiver.recv() {
                Ok(job) => job,
                Err(_) => return,
            };
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RasterRenderCacheKey {
    signature: String,
    column_stride_frames: u32,
    display_row_count: u32,
}

impl RasterRenderCacheKey {
    fn new(item: &RasterWorkItem) -> Self {
        Self {
            signature: item.signature_key.clone(),
            column_stride_frames: item.cache_key.column_stride_frames,
            display_row_count: item.cache_key.display_row_count,
        }
    }
}

fn prune_worker_raster_cache(
    cache: &mut HashMap<RasterRenderCacheKey, CachedRasterValue>,
    active_items: &[RasterWorkItem],
) {
    let active = active_items
        .iter()
        .map(RasterRenderCacheKey::new)
        .collect::<HashSet<_>>();
    cache.retain(|key, _| active.contains(key));
}

fn prune_prepared_renderer_cache(
    cache: &mut HashMap<String, PreparedEffectRasterRenderer>,
    active_items: &[RasterWorkItem],
) {
    let active = active_items
        .iter()
        .map(|item| item.signature_key.clone())
        .collect::<HashSet<_>>();
    cache.retain(|key, _| active.contains(key));
}

fn newest_queued_job(receiver: &mpsc::Receiver<RasterJob>) -> Option<RasterJob> {
    let mut newest = None;
    while let Ok(job) = receiver.try_recv() {
        newest = Some(job);
    }
    newest
}

fn ordered_existing_effect_ids(
    effects: &[dawn_language::effect::EffectInst],
    ordered_effect_ids: &[u32],
) -> Vec<u32> {
    let existing = effects
        .iter()
        .map(|effect| effect.id.0)
        .collect::<HashSet<_>>();
    let mut ids = Vec::new();
    for id in ordered_effect_ids {
        if existing.contains(id) && !ids.contains(id) {
            ids.push(*id);
        }
    }
    ids
}

struct RasterRenderRequest<'a> {
    renderer: PreparedEffectRasterRenderer,
    effect_id: u32,
    column_stride_frames: u32,
    display_row_count: u32,
    should_continue: &'a dyn Fn() -> bool,
}

fn render_effect_raster(
    request: RasterRenderRequest<'_>,
) -> Result<SequenceClipRaster, RasterRenderFailure> {
    let RasterRenderRequest {
        renderer,
        effect_id,
        column_stride_frames,
        display_row_count,
        should_continue,
    } = request;
    if column_stride_frames == 0 {
        return Err(RasterRenderFailure::Error(
            "raster column stride frames must be greater than zero".to_string(),
        ));
    }
    if display_row_count == 0 {
        return Err(RasterRenderFailure::Error(
            "raster display row count must be greater than zero".to_string(),
        ));
    }
    let start_seconds = renderer.start_seconds();
    let duration_seconds = renderer.duration_seconds();
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return Err(RasterRenderFailure::Error(
            "effect duration must be positive and finite".to_string(),
        ));
    }
    let target_pixel_count = renderer.target_pixel_count();
    if target_pixel_count == 0 {
        return Err(RasterRenderFailure::Error(
            "effect target has no pixels".to_string(),
        ));
    }
    let duration_frames = duration_seconds * f64::from(renderer.frame_rate());
    if !duration_frames.is_finite() || duration_frames <= 0.0 {
        return Err(RasterRenderFailure::Error(
            "effect duration frames must be positive and finite".to_string(),
        ));
    }
    let columns = (duration_frames / f64::from(column_stride_frames))
        .ceil()
        .max(1.0) as usize;
    let rows = target_pixel_count.min(display_row_count as usize);
    let mut pixels_rgba = vec![0u8; rows * columns * 4];
    for column in 0..columns {
        if !should_continue() {
            return Err(RasterRenderFailure::Cancelled);
        }
        let sample_seconds = start_seconds
            + (column * column_stride_frames as usize) as f64 / f64::from(renderer.frame_rate());
        let colors = renderer
            .render_target_colors(sample_seconds)
            .map_err(|error| RasterRenderFailure::Error(format!("{error:?}")))?;
        for row in 0..rows {
            let color = average_row(&colors, row, rows);
            let offset = (row * columns + column) * 4;
            pixels_rgba[offset] = color.red;
            pixels_rgba[offset + 1] = color.green;
            pixels_rgba[offset + 2] = color.blue;
            pixels_rgba[offset + 3] = 255;
        }
    }
    Ok(SequenceClipRaster {
        request_id: 0,
        effect_id,
        signature: String::new(),
        columns: columns as u32,
        rows: rows as u32,
        column_stride_frames,
        start_seconds,
        duration_seconds,
        pixels_rgba_base64: STANDARD.encode(pixels_rgba),
    })
}

enum RasterRenderFailure {
    Cancelled,
    Error(String),
}

#[derive(Clone, Debug, PartialEq)]
enum RenderInputSignature {
    Valid(Box<RenderInputSignatureData>),
    Invalid { message: String },
}

#[derive(Clone, Debug, PartialEq)]
struct RenderInputSignatureData {
    effect: EffectInst,
    definition: Option<EffectDefinition>,
    generator_definitions: Vec<(dawn_language::effect::EffectDefinitionId, EffectDefinition)>,
    curve_references: Vec<(CurveId, Option<CurveDefinition>)>,
    mark_references: Vec<(MarkCollectionKey, Option<Vec<DawnTime>>)>,
    target_pixels: Vec<RenderedTargetPixelAddress>,
}

fn render_signature(
    project: &DawnProject,
    setup_id: &SetupId,
    sequence: &Sequence,
    effect: &EffectInst,
) -> Result<RenderInputSignature, String> {
    let target_pixels =
        resolve_effect_target_pixel_addresses(project, setup_id, &effect.target, &effect.scope)
            .map_err(|error| format!("{error:?}"))?;
    let definition = project.definitions.effects.get(&effect.definition).cloned();
    let generator_definitions = if definition
        .as_ref()
        .is_some_and(|definition| definition.compiled.kind() == EffectKind::Generator)
    {
        project
            .definitions
            .effects
            .definitions
            .iter()
            .map(|(id, definition)| (id.clone(), definition.clone()))
            .collect()
    } else {
        Vec::new()
    };
    let mut curve_references = Vec::new();
    let mut mark_references = Vec::new();
    for value in effect.param_overrides.values() {
        collect_param_references(
            project,
            sequence,
            value,
            &mut curve_references,
            &mut mark_references,
        );
    }
    Ok(RenderInputSignature::Valid(Box::new(
        RenderInputSignatureData {
            effect: effect.clone(),
            definition,
            generator_definitions,
            curve_references,
            mark_references,
            target_pixels,
        },
    )))
}

fn collect_param_references(
    project: &DawnProject,
    sequence: &Sequence,
    value: &EffectParamValue,
    curve_references: &mut Vec<(CurveId, Option<CurveDefinition>)>,
    mark_references: &mut Vec<(MarkCollectionKey, Option<Vec<DawnTime>>)>,
) {
    match value {
        EffectParamValue::Curve(CurveSource::Reference(id)) => {
            curve_references.push((id.clone(), project.definitions.curves.get(id).cloned()));
        }
        EffectParamValue::Marks(key) => {
            let marks = sequence
                .mark_collections
                .iter()
                .find(|collection| collection.key == *key)
                .map(|collection| collection.marks.clone());
            mark_references.push((key.clone(), marks));
        }
        EffectParamValue::Array(values) => {
            for value in values {
                collect_param_references(
                    project,
                    sequence,
                    value,
                    curve_references,
                    mark_references,
                );
            }
        }
        EffectParamValue::Int(_)
        | EffectParamValue::Float(_)
        | EffectParamValue::Bool(_)
        | EffectParamValue::Color(_)
        | EffectParamValue::Enum(_)
        | EffectParamValue::Curve(CurveSource::Inline(_)) => {}
    }
}

fn signature_key(signature: &RenderInputSignature) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{signature:?}").hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn empty_response(
    project_revision: u32,
    request_id: u32,
    complete: bool,
) -> SequenceClipRasterResponse {
    SequenceClipRasterResponse {
        project_revision,
        request_id,
        complete,
    }
}

fn average_row(colors: &[Color], row: usize, rows: usize) -> Color {
    let start = row * colors.len() / rows;
    let end = ((row + 1) * colors.len() / rows).max(start + 1);
    let mut red = 0u32;
    let mut green = 0u32;
    let mut blue = 0u32;
    for color in &colors[start..end] {
        red += u32::from(color.red);
        green += u32::from(color.green);
        blue += u32::from(color.blue);
    }
    let count = (end - start) as u32;
    Color {
        red: (red / count) as u8,
        green: (green / count) as u8,
        blue: (blue / count) as u8,
    }
}
