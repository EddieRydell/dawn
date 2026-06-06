use std::{
    collections::HashMap,
    sync::mpsc::{self, Receiver, Sender},
};

use camino::Utf8PathBuf;
use dawn_backend::{
    EffectPreviewExecutor, EffectPreviewExecutorHandle, EffectPreviewKey, EffectPreviewRequest,
    SequenceEffectPreviewResultBatch,
};

#[derive(Debug)]
pub(crate) struct EffectPreviewRuntime {
    sender: Sender<EffectPreviewRequest>,
    handle: EffectPreviewExecutorHandle,
}

impl Default for EffectPreviewRuntime {
    fn default() -> Self {
        let mut executor = EffectPreviewExecutor::default();
        let handle = executor.handle();
        let (sender, receiver) = mpsc::channel();
        tauri::async_runtime::spawn_blocking(move || run_worker(receiver, &mut executor));
        Self { sender, handle }
    }
}

impl EffectPreviewRuntime {
    pub(crate) fn submit(&self, request: EffectPreviewRequest) -> Result<(), String> {
        self.handle.register_request(&request);
        self.sender.send(request).map_err(|error| error.to_string())
    }

    pub(crate) fn take_results(
        &self,
        path: &Utf8PathBuf,
        object_key: &str,
    ) -> SequenceEffectPreviewResultBatch {
        self.handle.take_results(path, object_key)
    }
}

fn run_worker(receiver: Receiver<EffectPreviewRequest>, executor: &mut EffectPreviewExecutor) {
    while let Ok(request) = receiver.recv() {
        let mut pending = HashMap::new();
        pending.insert(request_key(&request), request);
        while let Ok(request) = receiver.try_recv() {
            pending.insert(request_key(&request), request);
        }
        for request in pending.into_values() {
            executor.execute_request(request);
        }
    }
}

fn request_key(request: &EffectPreviewRequest) -> EffectPreviewKey {
    EffectPreviewKey {
        path: request.path.clone(),
        object_key: request.object_key.clone(),
    }
}
