use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::SystemTime;

use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};

use crate::contracts::{
    CommandAck, Event, EventEnvelope, RequestId, Revision, RuntimeError, RuntimeErrorKind,
    RuntimeResult, ServiceName,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackpressurePolicy {
    Reject,
    LatestOnly,
    Coalesce,
}

#[derive(Debug)]
pub enum RunnerMessage<C> {
    Command {
        request_id: RequestId,
        target_revision: Option<Revision>,
        command: C,
    },
    Shutdown,
}

pub trait ServiceCore: Send + 'static {
    type Command: Send + 'static;

    fn service_name(&self) -> ServiceName;
    fn revision(&self) -> Revision;
    fn handle(&mut self, command: Self::Command) -> RuntimeResult<Vec<Event>>;
}

pub struct ServiceHandle<C> {
    service: ServiceName,
    tx: Sender<RunnerMessage<C>>,
    policy: BackpressurePolicy,
    stopped: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl<C: Send + 'static> ServiceHandle<C> {
    pub fn submit(
        &self,
        request_id: RequestId,
        target_revision: Option<Revision>,
        command: C,
    ) -> RuntimeResult<CommandAck> {
        if self.stopped.load(Ordering::SeqCst) {
            return Err(RuntimeError::new(
                self.service.clone(),
                RuntimeErrorKind::Fatal,
                "service runner is stopped",
            ));
        }
        let message = RunnerMessage::Command {
            request_id,
            target_revision,
            command,
        };
        match self.tx.try_send(message) {
            Ok(()) => Ok(CommandAck {
                request_id,
                service: self.service.clone(),
                target_revision,
            }),
            Err(TrySendError::Full(_)) => Err(RuntimeError::new(
                self.service.clone(),
                RuntimeErrorKind::Backpressure,
                format!("{:?} queue is full", self.policy),
            )),
            Err(TrySendError::Disconnected(_)) => Err(RuntimeError::new(
                self.service.clone(),
                RuntimeErrorKind::Fatal,
                "service runner disconnected",
            )),
        }
    }

    pub fn shutdown(mut self) -> RuntimeResult<()> {
        let _ = self.tx.send(RunnerMessage::Shutdown);
        if let Some(join) = self.join.take() {
            join.join().map_err(|_| {
                RuntimeError::new(
                    self.service,
                    RuntimeErrorKind::Fatal,
                    "service runner thread panicked",
                )
            })?;
        }
        Ok(())
    }
}

pub fn spawn_service<C>(
    mut core: C,
    capacity: usize,
    policy: BackpressurePolicy,
    events: Sender<EventEnvelope>,
) -> ServiceHandle<C::Command>
where
    C: ServiceCore,
{
    let service = core.service_name();
    let (tx, rx) = bounded(capacity);
    let stopped = Arc::new(AtomicBool::new(false));
    let stopped_thread = stopped.clone();
    let sequence = Arc::new(AtomicU64::new(0));
    let sequence_thread = sequence.clone();
    let join =
        thread::spawn(move || run_loop(&mut core, rx, events, stopped_thread, sequence_thread));
    ServiceHandle {
        service,
        tx,
        policy,
        stopped,
        join: Some(join),
    }
}

fn run_loop<C>(
    core: &mut C,
    rx: Receiver<RunnerMessage<C::Command>>,
    events: Sender<EventEnvelope>,
    stopped: Arc<AtomicBool>,
    sequence: Arc<AtomicU64>,
) where
    C: ServiceCore,
{
    while let Ok(message) = rx.recv() {
        match message {
            RunnerMessage::Command {
                request_id,
                target_revision: _,
                command,
            } => match core.handle(command) {
                Ok(core_events) => {
                    for event in core_events {
                        let _ = events.send(envelope(
                            Some(request_id),
                            core.service_name(),
                            &sequence,
                            event,
                        ));
                    }
                }
                Err(error) => {
                    let _ = events.send(envelope(
                        Some(request_id),
                        core.service_name(),
                        &sequence,
                        Event::Fatal {
                            service: error.service,
                            message: error.message,
                        },
                    ));
                }
            },
            RunnerMessage::Shutdown => {
                stopped.store(true, Ordering::SeqCst);
                return;
            }
        }
    }
    stopped.store(true, Ordering::SeqCst);
}

fn envelope(
    request_id: Option<RequestId>,
    service: ServiceName,
    sequence: &AtomicU64,
    event: Event,
) -> EventEnvelope {
    EventEnvelope {
        request_id,
        service,
        sequence: sequence.fetch_add(1, Ordering::SeqCst),
        created_at: SystemTime::now(),
        event,
    }
}
