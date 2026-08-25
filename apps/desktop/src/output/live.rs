use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use dawn_language::controller::{Controller, ControllerId};
use dawn_output::OutputTransports;
use indexmap::IndexMap;

use crate::audio::AudioEngine;
use crate::dto::{
    AudioTransportState, LiveOutputControllerSnapshot, LiveOutputControllerState,
    LiveOutputSnapshot, LiveOutputState,
};
use crate::rendering::SequenceRenderService;

enum Command {
    Enable {
        generation: u32,
        controllers: IndexMap<ControllerId, Controller>,
        active: Vec<ControllerId>,
    },
    Disable {
        generation: u32,
    },
    Shutdown,
}

struct Update {
    generation: u32,
    snapshot: LiveOutputSnapshot,
}

pub(crate) struct LiveOutputService {
    sender: mpsc::Sender<Command>,
    receiver: mpsc::Receiver<Update>,
    generation: u32,
    snapshot: LiveOutputSnapshot,
    resume_after_prepare: bool,
    worker: Option<JoinHandle<()>>,
}

impl LiveOutputService {
    pub(crate) fn new(
        audio: Arc<Mutex<AudioEngine>>,
        render: Arc<Mutex<SequenceRenderService>>,
    ) -> Self {
        let (sender, command_receiver) = mpsc::channel();
        let (update_sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || worker(command_receiver, update_sender, audio, render));
        Self {
            sender,
            receiver,
            generation: 0,
            snapshot: disabled_snapshot(0),
            resume_after_prepare: false,
            worker: Some(worker),
        }
    }

    pub(crate) fn enable(
        &mut self,
        controllers: IndexMap<ControllerId, Controller>,
        active: Vec<ControllerId>,
    ) -> LiveOutputSnapshot {
        self.resume_after_prepare = false;
        self.generation = self.generation.saturating_add(1);
        self.snapshot = preparing_snapshot(self.generation, &controllers, &active);
        if self
            .sender
            .send(Command::Enable {
                generation: self.generation,
                controllers,
                active,
            })
            .is_err()
        {
            self.snapshot.state = LiveOutputState::Error;
            self.snapshot.last_error = Some("Live output worker is unavailable.".to_string());
        }
        self.snapshot.clone()
    }

    pub(crate) fn disable(&mut self) -> LiveOutputSnapshot {
        self.resume_after_prepare = false;
        self.disable_preserving_resume()
    }

    fn disable_preserving_resume(&mut self) -> LiveOutputSnapshot {
        self.generation = self.generation.saturating_add(1);
        let _ = self.sender.send(Command::Disable {
            generation: self.generation,
        });
        self.snapshot = disabled_snapshot(self.generation);
        self.snapshot.clone()
    }

    pub(crate) fn suspend(&mut self) -> LiveOutputSnapshot {
        self.resume_after_prepare = matches!(
            self.snapshot.state,
            LiveOutputState::Preparing | LiveOutputState::Holding | LiveOutputState::Streaming
        );
        self.disable_preserving_resume()
    }

    pub(crate) fn take_resume_after_prepare(&mut self) -> bool {
        std::mem::take(&mut self.resume_after_prepare)
    }

    pub(crate) fn snapshot(&mut self) -> LiveOutputSnapshot {
        for update in self.receiver.try_iter() {
            if update.generation == self.generation {
                self.snapshot = update.snapshot;
            }
        }
        self.snapshot.clone()
    }

    pub(crate) fn shutdown(&mut self) {
        self.generation = self.generation.saturating_add(1);
        let _ = self.sender.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.snapshot = disabled_snapshot(self.generation);
    }
}

impl Drop for LiveOutputService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn worker(
    receiver: mpsc::Receiver<Command>,
    updates: mpsc::Sender<Update>,
    audio: Arc<Mutex<AudioEngine>>,
    render: Arc<Mutex<SequenceRenderService>>,
) {
    let mut active: Option<(u32, OutputTransports, LiveOutputSnapshot)> = None;
    let mut tick_interval = Duration::from_millis(20);
    loop {
        let wait = active
            .as_ref()
            .map_or(Duration::from_secs(60), |_| tick_interval);
        match receiver.recv_timeout(wait) {
            Ok(Command::Enable {
                generation,
                controllers,
                active: ids,
            }) => {
                terminate_active(&mut active);
                tick_interval = Duration::from_millis(20);
                let mut snapshot = preparing_snapshot(generation, &controllers, &ids);
                match OutputTransports::open(&controllers, &ids) {
                    Ok(transports) => {
                        snapshot.state = LiveOutputState::Holding;
                        for controller in &mut snapshot.controllers {
                            controller.state = LiveOutputControllerState::Active;
                        }
                        let _ = updates.send(Update {
                            generation,
                            snapshot: snapshot.clone(),
                        });
                        active = Some((generation, transports, snapshot));
                    }
                    Err(error) => {
                        fail_snapshot(&mut snapshot, format!("{error:?}"));
                        let _ = updates.send(Update {
                            generation,
                            snapshot,
                        });
                    }
                }
            }
            Ok(Command::Disable { generation }) => {
                terminate_active(&mut active);
                let _ = updates.send(Update {
                    generation,
                    snapshot: disabled_snapshot(generation),
                });
            }
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                terminate_active(&mut active);
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        let Some((generation, transports, snapshot)) = active.as_mut() else {
            continue;
        };
        let audio = audio
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .snapshot();
        if matches!(
            audio.state,
            AudioTransportState::Stopped | AudioTransportState::Ended
        ) {
            let generation = *generation;
            terminate_active(&mut active);
            let _ = updates.send(Update {
                generation,
                snapshot: disabled_snapshot(generation),
            });
            continue;
        }
        let rendered = render
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .render_current_sequence_frame(&audio);
        match rendered {
            Ok(rendered) => {
                tick_interval =
                    Duration::from_secs_f64(1.0 / f64::from(rendered.frame.frame_rate.max(1)));
                if let Err(error) = transports.send(&rendered.frame.controller_frames) {
                    let generation = *generation;
                    let mut failed = snapshot.clone();
                    fail_snapshot(&mut failed, format!("{error:?}"));
                    terminate_active(&mut active);
                    let _ = updates.send(Update {
                        generation,
                        snapshot: failed,
                    });
                } else {
                    let state = if matches!(audio.state, AudioTransportState::Playing) {
                        LiveOutputState::Streaming
                    } else {
                        LiveOutputState::Holding
                    };
                    if snapshot.state != state {
                        snapshot.state = state;
                        let _ = updates.send(Update {
                            generation: *generation,
                            snapshot: snapshot.clone(),
                        });
                    }
                }
            }
            Err(error) => {
                let generation = *generation;
                let mut failed = snapshot.clone();
                fail_snapshot(&mut failed, format!("{error:?}"));
                terminate_active(&mut active);
                let _ = updates.send(Update {
                    generation,
                    snapshot: failed,
                });
            }
        }
    }
}

fn terminate_active(active: &mut Option<(u32, OutputTransports, LiveOutputSnapshot)>) {
    if let Some((_, transports, _)) = active.as_mut() {
        let _ = transports.blackout_and_terminate();
    }
    *active = None;
}

fn fail_snapshot(snapshot: &mut LiveOutputSnapshot, message: String) {
    snapshot.state = LiveOutputState::Error;
    snapshot.last_error = Some(message.clone());
    for controller in &mut snapshot.controllers {
        controller.state = LiveOutputControllerState::Error;
        controller.last_error = Some(message.clone());
    }
}

fn preparing_snapshot(
    generation: u32,
    controllers: &IndexMap<ControllerId, Controller>,
    active: &[ControllerId],
) -> LiveOutputSnapshot {
    let active_universe_count = active
        .iter()
        .filter_map(|id| controllers.get(id))
        .map(|controller| controller.ports.len() as u32)
        .sum();
    LiveOutputSnapshot {
        state: LiveOutputState::Preparing,
        generation,
        active_controller_count: active.len() as u32,
        active_universe_count,
        controllers: active
            .iter()
            .map(|id| LiveOutputControllerSnapshot {
                id: format!("{}:{}", id.0.document(), id.0.object()),
                state: LiveOutputControllerState::Opening,
                last_error: None,
            })
            .collect(),
        last_error: None,
    }
}

pub(crate) fn disabled_snapshot(generation: u32) -> LiveOutputSnapshot {
    LiveOutputSnapshot {
        state: LiveOutputState::Disabled,
        generation,
        active_controller_count: 0,
        active_universe_count: 0,
        controllers: Vec::new(),
        last_error: None,
    }
}
