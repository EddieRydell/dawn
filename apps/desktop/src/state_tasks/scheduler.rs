use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use camino::Utf8PathBuf;
use dawn_project_io::{ProjectSession, save_project};

struct Sequenced<T> {
    sequence: u64,
    payload: T,
}

pub(crate) trait SequenceResult {
    fn sequence(&self) -> u64;
}

pub(crate) struct LatestScheduler<P, R> {
    sender: mpsc::Sender<Sequenced<P>>,
    receiver: mpsc::Receiver<R>,
    latest_sequence: u64,
}

impl<P: Send + 'static, R: SequenceResult + Send + 'static> LatestScheduler<P, R> {
    fn new(
        worker: impl FnOnce(mpsc::Receiver<Sequenced<P>>, mpsc::Sender<R>) + Send + 'static,
    ) -> Self {
        let (request_sender, request_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();
        thread::spawn(move || worker(request_receiver, result_sender));
        Self {
            sender: request_sender,
            receiver: result_receiver,
            latest_sequence: 0,
        }
    }

    pub(crate) fn schedule(&mut self, payload: P) -> bool {
        self.latest_sequence = self.latest_sequence.saturating_add(1);
        self.sender
            .send(Sequenced {
                sequence: self.latest_sequence,
                payload,
            })
            .is_ok()
    }

    pub(crate) fn invalidate_pending(&mut self) {
        self.latest_sequence = self.latest_sequence.saturating_add(1);
    }

    pub(crate) fn drain_current_results(&self) -> Vec<R> {
        self.receiver
            .try_iter()
            .filter(|result| result.sequence() == self.latest_sequence)
            .collect()
    }
}

pub(crate) type GuiSaveScheduler = LatestScheduler<GuiSavePayload, GuiSaveResult>;

pub(crate) fn gui_save_scheduler() -> GuiSaveScheduler {
    LatestScheduler::new(gui_save_worker)
}

pub(crate) struct GuiSavePayload {
    pub(crate) session: Arc<ProjectSession>,
    pub(crate) affected_paths: BTreeSet<String>,
    pub(crate) status_path: String,
    pub(crate) filesystem: Arc<Mutex<()>>,
}

pub(crate) enum GuiSaveResult {
    Saved {
        sequence: u64,
        session: Arc<ProjectSession>,
        affected_paths: BTreeSet<String>,
    },
    Failed {
        sequence: u64,
        status_path: String,
        message: String,
    },
}

impl SequenceResult for GuiSaveResult {
    fn sequence(&self) -> u64 {
        match self {
            Self::Saved { sequence, .. } | Self::Failed { sequence, .. } => *sequence,
        }
    }
}

fn gui_save_worker(
    receiver: mpsc::Receiver<Sequenced<GuiSavePayload>>,
    sender: mpsc::Sender<GuiSaveResult>,
) {
    let debounce = Duration::from_millis(250);
    while let Ok(mut pending) = receiver.recv() {
        while let Ok(next) = receiver.recv_timeout(debounce) {
            pending = next;
        }
        let _filesystem = pending
            .payload
            .filesystem
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let result = match save_project(&pending.payload.session) {
            Ok(_) => GuiSaveResult::Saved {
                sequence: pending.sequence,
                session: pending.payload.session,
                affected_paths: pending.payload.affected_paths,
            },
            Err(error) => GuiSaveResult::Failed {
                sequence: pending.sequence,
                status_path: pending.payload.status_path,
                message: error.to_string(),
            },
        };
        if sender.send(result).is_err() {
            break;
        }
    }
}

pub(crate) type RenderRefreshScheduler = LatestScheduler<RenderRefreshPayload, RenderRefreshResult>;

pub(crate) fn render_refresh_scheduler(
    wake: crate::preview::PreviewWake,
) -> RenderRefreshScheduler {
    LatestScheduler::new(move |receiver, sender| render_refresh_worker(receiver, sender, wake))
}

pub(crate) struct RenderRefreshPayload {
    pub(crate) project: Arc<ProjectSession>,
    pub(crate) setup_id: dawn_language::setup::SetupId,
    pub(crate) sequence_id: dawn_language::sequence::SequenceId,
}

pub(crate) enum RenderRefreshResult {
    Refreshed {
        sequence: u64,
        session: Box<crate::rendering::PreparedSequenceOutput>,
    },
    Failed {
        sequence: u64,
        message: String,
    },
}

impl SequenceResult for RenderRefreshResult {
    fn sequence(&self) -> u64 {
        match self {
            Self::Refreshed { sequence, .. } | Self::Failed { sequence, .. } => *sequence,
        }
    }
}

fn render_refresh_worker(
    receiver: mpsc::Receiver<Sequenced<RenderRefreshPayload>>,
    sender: mpsc::Sender<RenderRefreshResult>,
    wake: crate::preview::PreviewWake,
) {
    while let Ok(mut pending) = receiver.recv() {
        while let Ok(newer) = receiver.try_recv() {
            pending = newer;
        }
        let result = match crate::rendering::prepare_sequence_output(
            &pending.payload.project.project,
            &pending.payload.setup_id,
            &pending.payload.sequence_id,
        ) {
            Ok(session) => RenderRefreshResult::Refreshed {
                sequence: pending.sequence,
                session: Box::new(session),
            },
            Err(error) => RenderRefreshResult::Failed {
                sequence: pending.sequence,
                message: format!("{error:?}"),
            },
        };
        if sender.send(result).is_err() {
            break;
        }
        wake.notify();
    }
}

pub(crate) type ProjectAnalysisScheduler =
    LatestScheduler<ProjectAnalysisPayload, ProjectAnalysisResult>;

pub(crate) fn project_analysis_scheduler() -> ProjectAnalysisScheduler {
    LatestScheduler::new(project_analysis_worker)
}

pub(crate) struct ProjectAnalysisPayload {
    pub(crate) root: Utf8PathBuf,
    pub(crate) project_revision: u32,
    pub(crate) filesystem: Arc<Mutex<()>>,
}

pub(crate) struct ProjectAnalysisResult {
    sequence: u64,
    pub(crate) root: Utf8PathBuf,
    pub(crate) project_revision: u32,
    pub(crate) report: dawn_project_io::ProjectCheckReport,
}

impl SequenceResult for ProjectAnalysisResult {
    fn sequence(&self) -> u64 {
        self.sequence
    }
}

fn project_analysis_worker(
    receiver: mpsc::Receiver<Sequenced<ProjectAnalysisPayload>>,
    sender: mpsc::Sender<ProjectAnalysisResult>,
) {
    let debounce = Duration::from_millis(125);
    while let Ok(mut pending) = receiver.recv() {
        while let Ok(next) = receiver.recv_timeout(debounce) {
            pending = next;
        }
        let report = {
            let _filesystem = pending
                .payload
                .filesystem
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            dawn_project_io::check_package(&pending.payload.root)
        };
        let result = ProjectAnalysisResult {
            sequence: pending.sequence,
            root: pending.payload.root,
            project_revision: pending.payload.project_revision,
            report,
        };
        if sender.send(result).is_err() {
            break;
        }
    }
}
