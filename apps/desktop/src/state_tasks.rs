use std::collections::BTreeSet;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

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

    pub(crate) fn schedule(&mut self, payload: P) -> Result<(), ScheduleError> {
        self.latest_sequence = self.latest_sequence.saturating_add(1);
        self.sender
            .send(Sequenced {
                sequence: self.latest_sequence,
                payload,
            })
            .map_err(|_| ScheduleError)
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

pub(crate) struct ScheduleError;

pub(crate) type GuiSaveScheduler = LatestScheduler<GuiSavePayload, GuiSaveResult>;

pub(crate) fn gui_save_scheduler() -> GuiSaveScheduler {
    LatestScheduler::new(gui_save_worker)
}

pub(crate) struct GuiSavePayload {
    pub(crate) session: Arc<ProjectSession>,
    pub(crate) affected_paths: BTreeSet<String>,
    pub(crate) status_path: String,
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

pub(crate) fn render_refresh_scheduler() -> RenderRefreshScheduler {
    LatestScheduler::new(render_refresh_worker)
}

pub(crate) struct RenderRefreshPayload {
    pub(crate) project: Arc<ProjectSession>,
    pub(crate) setup_id: dawn_language::setup::SetupId,
    pub(crate) sequence_id: dawn_language::sequence::SequenceId,
}

pub(crate) enum RenderRefreshResult {
    Refreshed {
        sequence: u64,
        session: Box<crate::show_render::PreparedRenderSession>,
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
) {
    while let Ok(mut pending) = receiver.recv() {
        while let Ok(newer) = receiver.try_recv() {
            pending = newer;
        }
        let result = match crate::show_render::prepare_render_session(
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
    }
}

#[derive(Clone)]
pub(crate) struct GuiHistoryEntry {
    pub(crate) before: Arc<ProjectSession>,
    pub(crate) after: Arc<ProjectSession>,
    pub(crate) affected_paths: BTreeSet<String>,
    pub(crate) status_path: String,
}

pub(crate) struct GuiHistory {
    undo: Vec<GuiHistoryEntry>,
    redo: Vec<GuiHistoryEntry>,
    limit: usize,
}

impl GuiHistory {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit,
        }
    }

    pub(crate) fn push_undo(&mut self, entry: GuiHistoryEntry) {
        self.undo.push(entry);
        self.trim_undo();
        self.redo.clear();
    }

    pub(crate) fn push_undo_from_redo(&mut self, entry: GuiHistoryEntry) {
        self.undo.push(entry);
        self.trim_undo();
    }

    fn trim_undo(&mut self) {
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }

    pub(crate) fn peek_undo(&self) -> Option<GuiHistoryEntry> {
        self.undo.last().cloned()
    }

    pub(crate) fn pop_undo(&mut self) -> Option<GuiHistoryEntry> {
        self.undo.pop()
    }

    pub(crate) fn push_redo(&mut self, entry: GuiHistoryEntry) {
        self.redo.push(entry);
    }

    pub(crate) fn peek_redo(&self) -> Option<GuiHistoryEntry> {
        self.redo.last().cloned()
    }

    pub(crate) fn pop_redo(&mut self) -> Option<GuiHistoryEntry> {
        self.redo.pop()
    }

    pub(crate) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8Path;
    use dawn_project_io::load_project;

    fn entry(session: &ProjectSession, status_path: &str) -> GuiHistoryEntry {
        GuiHistoryEntry {
            before: Arc::new(session.clone()),
            after: Arc::new(session.clone()),
            affected_paths: BTreeSet::new(),
            status_path: status_path.to_string(),
        }
    }

    #[test]
    fn new_undo_entries_clear_redo_and_respect_the_limit() {
        let workspace = Utf8Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Utf8Path::parent)
            .unwrap();
        let session = load_project(&workspace.join("examples/starter/project.dawn")).unwrap();
        let mut history = GuiHistory::new(2);
        history.push_redo(entry(&session, "redo"));
        history.push_undo(entry(&session, "one"));
        history.push_undo(entry(&session, "two"));
        history.push_undo(entry(&session, "three"));

        assert!(history.pop_redo().is_none());
        assert_eq!(history.pop_undo().unwrap().status_path, "three");
        assert_eq!(history.pop_undo().unwrap().status_path, "two");
        assert!(history.pop_undo().is_none());
    }
}
