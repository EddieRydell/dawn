use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use dawn_backend::{PreviewFrameExecutor, PreviewFrameRenderOutput, PreviewFrameRenderTask};

#[derive(Debug)]
pub(crate) struct PreviewFrameRuntime {
    sender: Sender<PreviewFrameRenderTask>,
    receiver: Receiver<PreviewFrameRenderOutput>,
    in_flight: bool,
}

impl Default for PreviewFrameRuntime {
    fn default() -> Self {
        let (task_sender, task_receiver) = mpsc::channel();
        let (output_sender, output_receiver) = mpsc::channel();
        tauri::async_runtime::spawn_blocking(move || run_worker(task_receiver, output_sender));
        Self {
            sender: task_sender,
            receiver: output_receiver,
            in_flight: false,
        }
    }
}

impl PreviewFrameRuntime {
    pub(crate) fn has_in_flight(&self) -> bool {
        self.in_flight
    }

    pub(crate) fn submit(&mut self, task: PreviewFrameRenderTask) -> Result<(), String> {
        self.in_flight = true;
        self.sender.send(task).map_err(|error| {
            self.in_flight = false;
            error.to_string()
        })
    }

    pub(crate) fn try_take_completed(
        &mut self,
    ) -> Result<Option<PreviewFrameRenderOutput>, String> {
        match self.receiver.try_recv() {
            Ok(output) => {
                self.in_flight = false;
                Ok(Some(output))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                self.in_flight = false;
                Err("preview frame worker disconnected".to_string())
            }
        }
    }
}

fn run_worker(
    receiver: Receiver<PreviewFrameRenderTask>,
    sender: Sender<PreviewFrameRenderOutput>,
) {
    let mut executor = PreviewFrameExecutor::default();
    while let Ok(task) = receiver.recv() {
        let output = executor.render(task);
        if sender.send(output).is_err() {
            break;
        }
    }
}
