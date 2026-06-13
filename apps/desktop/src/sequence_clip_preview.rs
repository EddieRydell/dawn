use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::mpsc;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::thread;

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
    resolve_effect_target_pixel_addresses, PreparedEffectPreviewRenderer,
    RenderedTargetPixelAddress,
};

use crate::dto::{
    GuiDocumentRequest, SequenceClipPreview, SequenceClipPreviewError, SequenceClipPreviewRequest,
    SequenceClipPreviewResponse, SequenceClipPreviewResultBatch, SequenceClipPreviewUnavailable,
};

pub struct SequenceClipPreviewService {
    sender: mpsc::Sender<PreviewJob>,
    receiver: mpsc::Receiver<PreviewWorkerResult>,
    cache: HashMap<PreviewCacheKey, CachedPreviewResult>,
    pending_results: HashMap<PreviewDocumentKey, VecDeque<PreviewResultEntry>>,
    active: Option<ActivePreviewJob>,
    next_job_id: u32,
    latest_job_id: Arc<AtomicU64>,
}

impl SequenceClipPreviewService {
    pub fn new() -> Self {
        let (request_sender, request_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();
        let latest_job_id = Arc::new(AtomicU64::new(0));
        thread::spawn({
            let latest_job_id = Arc::clone(&latest_job_id);
            move || preview_worker(request_receiver, result_sender, latest_job_id)
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
        request: SequenceClipPreviewRequest,
    ) -> SequenceClipPreviewResponse {
        let document_key = PreviewDocumentKey::new(&request.document);
        let base_key = PreviewRequestKey {
            path: request.document.path.clone(),
            object_key: request.document.object_key.clone(),
            sample_step_seconds: request.sample_step_seconds,
            max_rows: request.max_rows,
            max_columns: request.max_columns,
        };
        self.drain_results();
        self.prepare_response_and_schedule(PreviewScheduleInput {
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
                PreviewWorkerResult::Preview {
                    job_id,
                    request_id,
                    document_key,
                    cache_key,
                    signature,
                    signature_key,
                    preview,
                } => {
                    let result_preview = preview.clone();
                    self.cache.insert(
                        cache_key,
                        CachedPreviewResult {
                            signature: signature.clone(),
                            result: CachedPreviewValue::Preview(preview),
                        },
                    );
                    if self.is_current_request(&document_key, request_id) {
                        self.push_result(
                            document_key,
                            PreviewResultEntry::Ready {
                                request_id,
                                signature: signature_key,
                                preview: result_preview,
                            },
                        );
                    }
                    self.finish_active_work_item(job_id);
                }
                PreviewWorkerResult::Error {
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
                        CachedPreviewResult {
                            signature: signature.clone(),
                            result: CachedPreviewValue::Error(error),
                        },
                    );
                    if self.is_current_request(&document_key, request_id) {
                        self.push_result(
                            document_key,
                            PreviewResultEntry::Error {
                                request_id,
                                signature: signature_key,
                                error: result_error,
                            },
                        );
                    }
                    self.finish_active_work_item(job_id);
                }
                PreviewWorkerResult::Complete {
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
                        self.push_result(document_key, PreviewResultEntry::Complete { request_id });
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
    ) -> SequenceClipPreviewResultBatch {
        self.drain_results();
        let document_key = PreviewDocumentKey::new(&request);
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
                    PreviewResultEntry::Ready {
                        request_id: entry_request_id,
                        signature,
                        mut preview,
                    } if entry_request_id == request_id => {
                        preview.request_id = entry_request_id;
                        preview.signature = signature;
                        ready.push(preview);
                    }
                    PreviewResultEntry::Unavailable {
                        request_id: entry_request_id,
                        effect_id,
                        signature,
                    } if entry_request_id == request_id => {
                        unavailable.push(SequenceClipPreviewUnavailable {
                            request_id: entry_request_id,
                            effect_id,
                            signature,
                        });
                    }
                    PreviewResultEntry::Error {
                        request_id: entry_request_id,
                        signature,
                        mut error,
                    } if entry_request_id == request_id => {
                        error.request_id = entry_request_id;
                        error.signature = signature;
                        errors.push(error);
                    }
                    PreviewResultEntry::Complete {
                        request_id: entry_request_id,
                    } if entry_request_id == request_id => {
                        complete = true;
                    }
                    entry => retained.push_back(entry),
                }
            }
            *entries = retained;
        }

        SequenceClipPreviewResultBatch {
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
        input: PreviewScheduleInput,
    ) -> SequenceClipPreviewResponse {
        let PreviewScheduleInput {
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
        let ordered_ids = ordered_all_effect_ids(&sequence.effects, &requested_ids);
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
                    PreviewResultEntry::Unavailable {
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
            let cache_key = PreviewCacheKey::new(&base_key, effect_id);
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
                    CachedPreviewValue::Preview(preview) => self.push_result(
                        document_key.clone(),
                        PreviewResultEntry::Ready {
                            request_id,
                            signature: signature_key,
                            preview: preview.clone(),
                        },
                    ),
                    CachedPreviewValue::Error(error) => self.push_result(
                        document_key.clone(),
                        PreviewResultEntry::Error {
                            request_id,
                            signature: signature_key,
                            error: error.clone(),
                        },
                    ),
                },
                Some(_) => {
                    self.cache.remove(&cache_key);
                    work_items.push(PreviewWorkItem {
                        cache_key,
                        signature,
                        signature_key,
                        effect_id,
                    });
                }
                None => work_items.push(PreviewWorkItem {
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
        SequenceClipPreviewResponse {
            project_revision,
            request_id,
            complete: self.active.is_none(),
        }
    }

    fn schedule_missing_work(
        &mut self,
        request_id: u32,
        document_key: PreviewDocumentKey,
        project: DawnProject,
        setup_id: SetupId,
        sequence_id: SequenceId,
        work_items: Vec<PreviewWorkItem>,
    ) {
        if work_items.is_empty() {
            self.active = None;
            self.push_result(document_key, PreviewResultEntry::Complete { request_id });
            return;
        }
        let job_id = request_id;
        let job = PreviewJob {
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
            self.active = Some(ActivePreviewJob {
                job_id,
                request_id,
                document_key,
                work_items,
            });
        } else {
            self.active = None;
            self.push_result(document_key, PreviewResultEntry::Complete { request_id });
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

    fn push_result(&mut self, document_key: PreviewDocumentKey, result: PreviewResultEntry) {
        self.pending_results
            .entry(document_key)
            .or_default()
            .push_back(result);
    }

    fn is_current_request(&self, document_key: &PreviewDocumentKey, request_id: u32) -> bool {
        self.active.as_ref().is_some_and(|active| {
            &active.document_key == document_key && active.request_id == request_id
        })
    }
}

impl Default for SequenceClipPreviewService {
    fn default() -> Self {
        Self::new()
    }
}

struct ActivePreviewJob {
    job_id: u32,
    request_id: u32,
    document_key: PreviewDocumentKey,
    work_items: Vec<PreviewWorkItem>,
}

struct PreviewScheduleInput {
    project_revision: u32,
    project: Option<DawnProject>,
    setup_id: Option<SetupId>,
    sequence_id: Option<SequenceId>,
    document_key: PreviewDocumentKey,
    base_key: PreviewRequestKey,
    ordered_items: Vec<crate::dto::SequenceClipPreviewRequestItem>,
}

#[derive(Clone, Debug, PartialEq)]
struct PreviewDocumentKey {
    path: String,
    object_key: Option<String>,
}

impl PreviewDocumentKey {
    fn new(request: &GuiDocumentRequest) -> Self {
        Self {
            path: request.path.clone(),
            object_key: request.object_key.clone(),
        }
    }
}

impl Eq for PreviewDocumentKey {}

impl Hash for PreviewDocumentKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.object_key.hash(state);
    }
}

#[derive(Clone, Debug, PartialEq)]
struct PreviewRequestKey {
    path: String,
    object_key: Option<String>,
    sample_step_seconds: f64,
    max_rows: u32,
    max_columns: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreviewCacheKey {
    path: String,
    object_key: Option<String>,
    effect_id: u32,
    sample_step_seconds: u64,
    max_rows: u32,
    max_columns: u32,
}

impl PreviewCacheKey {
    fn new(request: &PreviewRequestKey, effect_id: u32) -> Self {
        Self {
            path: request.path.clone(),
            object_key: request.object_key.clone(),
            effect_id,
            sample_step_seconds: request.sample_step_seconds.to_bits(),
            max_rows: request.max_rows,
            max_columns: request.max_columns,
        }
    }

    fn matches_request(&self, request: &PreviewRequestKey) -> bool {
        self.path == request.path
            && self.object_key == request.object_key
            && self.sample_step_seconds == request.sample_step_seconds.to_bits()
            && self.max_rows == request.max_rows
            && self.max_columns == request.max_columns
    }
}

impl Hash for PreviewCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.object_key.hash(state);
        self.effect_id.hash(state);
        self.sample_step_seconds.hash(state);
        self.max_rows.hash(state);
        self.max_columns.hash(state);
    }
}

#[derive(Clone)]
struct CachedPreviewResult {
    signature: RenderInputSignature,
    result: CachedPreviewValue,
}

#[derive(Clone)]
enum CachedPreviewValue {
    Preview(SequenceClipPreview),
    Error(SequenceClipPreviewError),
}

struct PreviewJob {
    job_id: u32,
    request_id: u32,
    document_key: PreviewDocumentKey,
    project: DawnProject,
    setup_id: SetupId,
    sequence_id: SequenceId,
    work_items: Vec<PreviewWorkItem>,
}

#[derive(Clone, Debug, PartialEq)]
struct PreviewWorkItem {
    cache_key: PreviewCacheKey,
    signature: RenderInputSignature,
    signature_key: String,
    effect_id: u32,
}

enum PreviewResultEntry {
    Ready {
        request_id: u32,
        signature: String,
        preview: SequenceClipPreview,
    },
    Unavailable {
        request_id: u32,
        effect_id: u32,
        signature: String,
    },
    Error {
        request_id: u32,
        signature: String,
        error: SequenceClipPreviewError,
    },
    Complete {
        request_id: u32,
    },
}

enum PreviewWorkerResult {
    Preview {
        job_id: u32,
        request_id: u32,
        document_key: PreviewDocumentKey,
        cache_key: PreviewCacheKey,
        signature: RenderInputSignature,
        signature_key: String,
        preview: SequenceClipPreview,
    },
    Error {
        job_id: u32,
        request_id: u32,
        document_key: PreviewDocumentKey,
        cache_key: PreviewCacheKey,
        signature: RenderInputSignature,
        signature_key: String,
        error: SequenceClipPreviewError,
    },
    Complete {
        job_id: u32,
        request_id: u32,
        document_key: PreviewDocumentKey,
    },
}

fn preview_worker(
    receiver: mpsc::Receiver<PreviewJob>,
    sender: mpsc::Sender<PreviewWorkerResult>,
    latest_job_id: Arc<AtomicU64>,
) {
    let mut cache = HashMap::<PreviewRenderCacheKey, CachedPreviewValue>::new();
    let mut job = match receiver.recv() {
        Ok(job) => job,
        Err(_) => return,
    };
    loop {
        prune_worker_cache(&mut cache, &job.work_items);
        let mut completed = true;
        let work_items = std::mem::take(&mut job.work_items);
        for item in work_items {
            if latest_job_id.load(Ordering::Relaxed) != u64::from(job.job_id) {
                completed = false;
                break;
            }
            let render_cache_key = PreviewRenderCacheKey::new(&item);
            let result = match cache.get(&render_cache_key).cloned() {
                Some(CachedPreviewValue::Preview(preview)) => PreviewWorkerResult::Preview {
                    job_id: job.job_id,
                    request_id: job.request_id,
                    document_key: job.document_key.clone(),
                    cache_key: item.cache_key,
                    signature: item.signature,
                    signature_key: item.signature_key,
                    preview,
                },
                Some(CachedPreviewValue::Error(error)) => PreviewWorkerResult::Error {
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
                    match render_effect_preview(PreviewRenderRequest {
                        project: &job.project,
                        setup_id: &job.setup_id,
                        sequence_id: &job.sequence_id,
                        effect_id: item.effect_id,
                        sample_step_seconds: item.cache_key.sample_step_seconds,
                        max_rows: item.cache_key.max_rows,
                        max_columns: item.cache_key.max_columns,
                        should_continue: &should_continue,
                    }) {
                        Ok(preview) => {
                            cache.insert(
                                render_cache_key,
                                CachedPreviewValue::Preview(preview.clone()),
                            );
                            PreviewWorkerResult::Preview {
                                job_id: job.job_id,
                                request_id: job.request_id,
                                document_key: job.document_key.clone(),
                                cache_key: item.cache_key,
                                signature: item.signature,
                                signature_key: item.signature_key,
                                preview,
                            }
                        }
                        Err(PreviewRenderFailure::Cancelled) => {
                            completed = false;
                            break;
                        }
                        Err(PreviewRenderFailure::Error(message)) => {
                            let error = SequenceClipPreviewError {
                                request_id: job.request_id,
                                effect_id: item.effect_id,
                                signature: item.signature_key.clone(),
                                message,
                            };
                            cache
                                .insert(render_cache_key, CachedPreviewValue::Error(error.clone()));
                            PreviewWorkerResult::Error {
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
                .send(PreviewWorkerResult::Complete {
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
struct PreviewRenderCacheKey {
    signature: String,
    sample_step_seconds: u64,
    max_rows: u32,
    max_columns: u32,
}

impl PreviewRenderCacheKey {
    fn new(item: &PreviewWorkItem) -> Self {
        Self {
            signature: item.signature_key.clone(),
            sample_step_seconds: item.cache_key.sample_step_seconds,
            max_rows: item.cache_key.max_rows,
            max_columns: item.cache_key.max_columns,
        }
    }
}

fn prune_worker_cache(
    cache: &mut HashMap<PreviewRenderCacheKey, CachedPreviewValue>,
    active_items: &[PreviewWorkItem],
) {
    let active = active_items
        .iter()
        .map(PreviewRenderCacheKey::new)
        .collect::<HashSet<_>>();
    cache.retain(|key, _| active.contains(key));
}

fn newest_queued_job(receiver: &mpsc::Receiver<PreviewJob>) -> Option<PreviewJob> {
    let mut newest = None;
    while let Ok(job) = receiver.try_recv() {
        newest = Some(job);
    }
    newest
}

fn ordered_all_effect_ids(
    effects: &[dawn_language::effect::EffectInst],
    ordered_effect_ids: &[u32],
) -> Vec<u32> {
    let mut ids = ordered_existing_effect_ids(effects, ordered_effect_ids);
    for effect in effects {
        if !ids.contains(&effect.id.0) {
            ids.push(effect.id.0);
        }
    }
    ids
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

struct PreviewRenderRequest<'a> {
    project: &'a DawnProject,
    setup_id: &'a SetupId,
    sequence_id: &'a SequenceId,
    effect_id: u32,
    sample_step_seconds: u64,
    max_rows: u32,
    max_columns: u32,
    should_continue: &'a dyn Fn() -> bool,
}

fn render_effect_preview(
    request: PreviewRenderRequest<'_>,
) -> Result<SequenceClipPreview, PreviewRenderFailure> {
    let PreviewRenderRequest {
        project,
        setup_id,
        sequence_id,
        effect_id,
        sample_step_seconds,
        max_rows,
        max_columns,
        should_continue,
    } = request;
    let sample_step_seconds = f64::from_bits(sample_step_seconds);
    if !sample_step_seconds.is_finite() || sample_step_seconds <= 0.0 {
        return Err(PreviewRenderFailure::Error(
            "preview sample step must be positive and finite".to_string(),
        ));
    }
    if max_rows == 0 {
        return Err(PreviewRenderFailure::Error(
            "preview max rows must be greater than zero".to_string(),
        ));
    }
    if max_columns == 0 {
        return Err(PreviewRenderFailure::Error(
            "preview max columns must be greater than zero".to_string(),
        ));
    }
    let effect_id = EffectInstId(effect_id);
    let renderer =
        PreparedEffectPreviewRenderer::prepare(project, setup_id, sequence_id, &effect_id)
            .map_err(|error| PreviewRenderFailure::Error(format!("{error:?}")))?;

    let start_seconds = renderer.start_seconds();
    let duration_seconds = renderer.duration_seconds();
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return Err(PreviewRenderFailure::Error(
            "effect duration must be positive and finite".to_string(),
        ));
    }
    let target_pixel_count = renderer.target_pixel_count();
    if target_pixel_count == 0 {
        return Err(PreviewRenderFailure::Error(
            "effect target has no pixels".to_string(),
        ));
    }
    let requested_columns = (duration_seconds / sample_step_seconds).ceil().max(1.0) as usize;
    let columns = requested_columns.min(max_columns as usize);
    let rows = target_pixel_count.min(max_rows as usize);
    let mut pixels_rgb = vec![0u8; rows * columns * 3];
    for column in 0..columns {
        if !should_continue() {
            return Err(PreviewRenderFailure::Cancelled);
        }
        let sample_seconds = if columns <= 1 {
            start_seconds
        } else {
            let progress = column as f64 / (columns - 1) as f64;
            start_seconds + duration_seconds * progress
        };
        let colors = renderer
            .render_target_colors(sample_seconds)
            .map_err(|error| PreviewRenderFailure::Error(format!("{error:?}")))?;
        for row in 0..rows {
            let color = average_row(&colors, row, rows);
            let offset = (row * columns + column) * 3;
            pixels_rgb[offset] = color.red;
            pixels_rgb[offset + 1] = color.green;
            pixels_rgb[offset + 2] = color.blue;
        }
    }
    Ok(SequenceClipPreview {
        request_id: 0,
        effect_id: effect_id.0,
        signature: String::new(),
        columns: columns as u32,
        rows: rows as u32,
        sample_step_seconds,
        start_seconds,
        duration_seconds,
        pixels_rgb,
    })
}

enum PreviewRenderFailure {
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
) -> SequenceClipPreviewResponse {
    SequenceClipPreviewResponse {
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
