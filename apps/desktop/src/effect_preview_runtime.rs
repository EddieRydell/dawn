use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use dawn_app_runtime::output::runtime::{SequenceEffectThumbnailResult, SequenceRenderCache};
use dawn_language::analysis::ProjectAnalysis;
use dawn_language::document::SequenceDocument;

use crate::effect_previews::{
    preview_max_columns, preview_max_rows, sequence_effect_preview_dto,
    SequenceEffectPreviewErrorResultDto, SequenceEffectPreviewReadyResultDto,
    SequenceEffectPreviewRequestEffectDto, SequenceEffectPreviewResultDto,
    SequenceEffectPreviewUnavailableResultDto,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SequencePreviewDocumentKey {
    path: String,
    object_key: String,
}

impl SequencePreviewDocumentKey {
    pub(crate) fn new(path: String, object_key: String) -> Self {
        Self { path, object_key }
    }
}

#[derive(Clone)]
pub(crate) struct EffectPreviewRuntime {
    sender: Sender<EffectPreviewJob>,
    latest_requests: Arc<Mutex<HashMap<SequencePreviewDocumentKey, u32>>>,
    completed: Arc<Mutex<HashMap<SequencePreviewDocumentKey, Vec<SequenceEffectPreviewResultDto>>>>,
}

impl Default for EffectPreviewRuntime {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        let latest_requests = Arc::new(Mutex::new(HashMap::new()));
        let completed = Arc::new(Mutex::new(HashMap::new()));
        start_worker(receiver, latest_requests.clone(), completed.clone());
        Self {
            sender,
            latest_requests,
            completed,
        }
    }
}

impl EffectPreviewRuntime {
    pub(crate) fn request(
        &self,
        path: String,
        object_key: String,
        request_id: u32,
        effects: Vec<SequenceEffectPreviewRequestEffectDto>,
        analysis: ProjectAnalysis,
        document: SequenceDocument,
    ) -> Result<(), String> {
        let key = SequencePreviewDocumentKey::new(path, object_key);
        self.latest_requests
            .lock()
            .map_err(|_| "effect preview request state lock is poisoned".to_string())?
            .insert(key.clone(), request_id);
        self.sender
            .send(EffectPreviewJob {
                key,
                request_id,
                effects,
                analysis,
                document,
            })
            .map_err(|_| "effect preview worker is not available".to_string())
    }

    pub(crate) fn take_results(
        &self,
        path: String,
        object_key: String,
    ) -> Result<Vec<SequenceEffectPreviewResultDto>, String> {
        let key = SequencePreviewDocumentKey::new(path, object_key);
        Ok(self
            .completed
            .lock()
            .map_err(|_| "effect preview result state lock is poisoned".to_string())?
            .remove(&key)
            .unwrap_or_default())
    }
}

struct EffectPreviewJob {
    key: SequencePreviewDocumentKey,
    request_id: u32,
    effects: Vec<SequenceEffectPreviewRequestEffectDto>,
    analysis: ProjectAnalysis,
    document: SequenceDocument,
}

fn start_worker(
    receiver: Receiver<EffectPreviewJob>,
    latest_requests: Arc<Mutex<HashMap<SequencePreviewDocumentKey, u32>>>,
    completed: Arc<Mutex<HashMap<SequencePreviewDocumentKey, Vec<SequenceEffectPreviewResultDto>>>>,
) {
    thread::spawn(move || {
        let mut cache = SequenceRenderCache::default();
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
            process_job(&mut cache, &latest_requests, &completed, job);
        }
    });
}

fn process_job(
    cache: &mut SequenceRenderCache,
    latest_requests: &Arc<Mutex<HashMap<SequencePreviewDocumentKey, u32>>>,
    completed: &Arc<
        Mutex<HashMap<SequencePreviewDocumentKey, Vec<SequenceEffectPreviewResultDto>>>,
    >,
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
                SequenceEffectPreviewResultDto::Unavailable(
                    SequenceEffectPreviewUnavailableResultDto {
                        request_id: job.request_id,
                        effect_id: requested.effect_id,
                        signature: requested.signature,
                    },
                ),
            );
            continue;
        };
        let result = cache.effect_thumbnail_cancellable(
            &job.analysis,
            &job.document,
            effect,
            preview_max_columns(),
            preview_max_rows(),
            || !is_latest_request(latest_requests, &job.key, job.request_id),
        );
        match result {
            Ok(SequenceEffectThumbnailResult::Ready(thumbnail)) => {
                push_result(
                    completed,
                    &job.key,
                    SequenceEffectPreviewResultDto::Ready(SequenceEffectPreviewReadyResultDto {
                        request_id: job.request_id,
                        signature: requested.signature,
                        preview: sequence_effect_preview_dto(thumbnail),
                    }),
                );
            }
            Ok(SequenceEffectThumbnailResult::Unavailable) => {
                push_result(
                    completed,
                    &job.key,
                    SequenceEffectPreviewResultDto::Unavailable(
                        SequenceEffectPreviewUnavailableResultDto {
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
                    SequenceEffectPreviewResultDto::Error(SequenceEffectPreviewErrorResultDto {
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
    completed: &Arc<
        Mutex<HashMap<SequencePreviewDocumentKey, Vec<SequenceEffectPreviewResultDto>>>,
    >,
    key: &SequencePreviewDocumentKey,
    result: SequenceEffectPreviewResultDto,
) {
    if let Ok(mut completed) = completed.lock() {
        completed.entry(key.clone()).or_default().push(result);
    }
}
