use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use camino::Utf8PathBuf;
use dawn_language::sequence_render::{SequenceEffectThumbnailResult, SequenceRenderCache};

use crate::types::{
    EffectPreviewRequest, SequenceEffectPreview, SequenceEffectPreviewErrorResult,
    SequenceEffectPreviewReadyResult, SequenceEffectPreviewResult,
    SequenceEffectPreviewResultBatch, SequenceEffectPreviewUnavailableResult,
};

const EFFECT_PREVIEW_MAX_COLUMNS: usize = 360;
const EFFECT_PREVIEW_MAX_ROWS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EffectPreviewKey {
    pub path: Utf8PathBuf,
    pub object_key: String,
}

#[derive(Debug, Clone, Default)]
pub struct EffectPreviewExecutorHandle {
    latest_requests: Arc<Mutex<HashMap<EffectPreviewKey, u32>>>,
    completed: Arc<Mutex<HashMap<EffectPreviewKey, Vec<SequenceEffectPreviewResult>>>>,
}

impl EffectPreviewExecutorHandle {
    pub fn register_request(&self, request: &EffectPreviewRequest) {
        let key = EffectPreviewKey {
            path: request.path.clone(),
            object_key: request.object_key.clone(),
        };
        if let Ok(mut latest) = self.latest_requests.lock() {
            latest.insert(key.clone(), request.request_id);
        }
        if let Ok(mut completed) = self.completed.lock() {
            completed.remove(&key);
        }
    }

    pub fn take_results(
        &self,
        path: &Utf8PathBuf,
        object_key: &str,
    ) -> SequenceEffectPreviewResultBatch {
        let key = EffectPreviewKey {
            path: path.clone(),
            object_key: object_key.to_string(),
        };
        let request_id = self
            .latest_requests
            .lock()
            .ok()
            .and_then(|latest| latest.get(&key).copied())
            .unwrap_or_default();
        let results = self
            .completed
            .lock()
            .ok()
            .and_then(|mut completed| completed.remove(&key))
            .unwrap_or_default();
        SequenceEffectPreviewResultBatch {
            request_id,
            results,
        }
    }
}

#[derive(Debug, Default)]
pub struct EffectPreviewExecutor {
    cache: SequenceRenderCache,
    handle: EffectPreviewExecutorHandle,
}

impl EffectPreviewExecutor {
    pub fn handle(&self) -> EffectPreviewExecutorHandle {
        self.handle.clone()
    }

    pub fn execute_request(&mut self, request: EffectPreviewRequest) {
        let key = EffectPreviewKey {
            path: request.path.clone(),
            object_key: request.object_key.clone(),
        };
        for requested in request.effects {
            if !self.is_latest_request(&key, request.request_id) {
                return;
            }
            let Some(effect) = request
                .document
                .effects
                .iter()
                .find(|effect| effect.id == requested.effect_id)
            else {
                self.push_result(
                    &key,
                    request.request_id,
                    SequenceEffectPreviewResult::Unavailable(
                        SequenceEffectPreviewUnavailableResult {
                            effect_id: requested.effect_id,
                            signature: requested.signature,
                        },
                    ),
                );
                continue;
            };
            let result = self.cache.effect_thumbnail_cancellable(
                &request.analysis,
                &request.document,
                effect,
                EFFECT_PREVIEW_MAX_COLUMNS,
                EFFECT_PREVIEW_MAX_ROWS,
                || !is_latest_request(&self.handle, &key, request.request_id),
            );
            match result {
                Ok(SequenceEffectThumbnailResult::Ready(thumbnail)) => {
                    self.push_result(
                        &key,
                        request.request_id,
                        SequenceEffectPreviewResult::Ready(SequenceEffectPreviewReadyResult {
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
                    self.push_result(
                        &key,
                        request.request_id,
                        SequenceEffectPreviewResult::Unavailable(
                            SequenceEffectPreviewUnavailableResult {
                                effect_id: requested.effect_id,
                                signature: requested.signature,
                            },
                        ),
                    );
                }
                Ok(SequenceEffectThumbnailResult::Cancelled) => return,
                Err(message) => {
                    self.push_result(
                        &key,
                        request.request_id,
                        SequenceEffectPreviewResult::Error(SequenceEffectPreviewErrorResult {
                            effect_id: requested.effect_id,
                            signature: requested.signature,
                            message,
                        }),
                    );
                }
            }
        }
    }

    fn is_latest_request(&self, key: &EffectPreviewKey, request_id: u32) -> bool {
        self.handle
            .latest_requests
            .lock()
            .map(|latest| latest.get(key).copied() == Some(request_id))
            .unwrap_or(false)
    }

    fn push_result(
        &self,
        key: &EffectPreviewKey,
        request_id: u32,
        result: SequenceEffectPreviewResult,
    ) {
        if !self.is_latest_request(key, request_id) {
            return;
        }
        if let Ok(mut completed) = self.handle.completed.lock() {
            completed.entry(key.clone()).or_default().push(result);
        }
    }
}

fn is_latest_request(
    handle: &EffectPreviewExecutorHandle,
    key: &EffectPreviewKey,
    request_id: u32,
) -> bool {
    handle
        .latest_requests
        .lock()
        .map(|latest| latest.get(key).copied() == Some(request_id))
        .unwrap_or(false)
}
