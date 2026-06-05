use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use dawn_language::path::Utf8PathBuf;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::workspace::file_events::project_path_from_event;

#[derive(Debug, Default)]
pub(super) struct FilesystemWorker {
    root: Option<PathBuf>,
    watcher: Option<RecommendedWatcher>,
    changes: Option<Receiver<Vec<Utf8PathBuf>>>,
}

impl FilesystemWorker {
    pub(super) fn sync_project_root(&mut self, root: Option<PathBuf>) -> Result<(), String> {
        if self.root == root {
            return Ok(());
        }

        self.watcher = None;
        self.changes = None;
        self.root = root.clone();

        let Some(root) = root else {
            return Ok(());
        };

        let (event_tx, event_rx) = mpsc::channel::<Event>();
        let (change_tx, change_rx) = mpsc::channel::<Vec<Utf8PathBuf>>();
        let mut watcher = notify::recommended_watcher(move |result| {
            if let Ok(event) = result {
                let _ = event_tx.send(event);
            }
        })
        .map_err(|error| error.to_string())?;
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|error| error.to_string())?;

        std::thread::spawn(move || {
            let debounce = Duration::from_millis(140);
            let mut pending = BTreeSet::new();
            loop {
                match event_rx.recv_timeout(debounce) {
                    Ok(event) => {
                        for path in event.paths {
                            if let Some(path) = project_path_from_event(&root, &path) {
                                pending.insert(path);
                            }
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        if pending.is_empty() {
                            continue;
                        }
                        let paths = pending.iter().cloned().collect::<Vec<_>>();
                        pending.clear();
                        if change_tx.send(paths).is_err() {
                            break;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });

        self.watcher = Some(watcher);
        self.changes = Some(change_rx);
        Ok(())
    }

    pub(super) fn drain(&mut self) -> Vec<Vec<Utf8PathBuf>> {
        let Some(changes) = &self.changes else {
            return Vec::new();
        };
        changes.try_iter().collect()
    }
}
