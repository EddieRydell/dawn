use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use dawn_project::path::{utf8_path, Utf8PathBuf};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Manager};

use crate::app_runtime::emit_runtime_read_models;
use crate::state::AppState;

#[derive(Default)]
pub(crate) struct FilesystemWatcherRuntime {
    root: Option<PathBuf>,
    watcher: Option<RecommendedWatcher>,
}

impl FilesystemWatcherRuntime {
    pub(crate) fn sync_project_root(
        &mut self,
        app: &AppHandle,
        root: Option<String>,
    ) -> Result<(), String> {
        let root = root.map(PathBuf::from);
        if self.root == root {
            return Ok(());
        }
        self.watcher = None;
        self.root = root.clone();
        let Some(root) = root else {
            return Ok(());
        };

        let (tx, rx) = mpsc::channel::<Event>();
        let mut watcher = notify::recommended_watcher(move |result| {
            if let Ok(event) = result {
                let _ = tx.send(event);
            }
        })
        .map_err(|error| error.to_string())?;
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|error| error.to_string())?;

        let app = app.clone();
        std::thread::spawn(move || {
            let debounce = Duration::from_millis(140);
            let mut pending = BTreeSet::new();
            loop {
                match rx.recv_timeout(debounce) {
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
                        let state = app.state::<AppState>();
                        {
                            let Ok(mut model) = crate::state::lock_runtime(&state) else {
                                continue;
                            };
                            let model = model.runtime_state_mut();
                            if model.handle_filesystem_changes(paths).is_ok() {
                                let _ = emit_runtime_read_models(&app, model);
                            }
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });

        self.watcher = Some(watcher);
        Ok(())
    }
}

fn project_path_from_event(root: &Path, path: &Path) -> Option<Utf8PathBuf> {
    let relative = path.strip_prefix(root).ok()?;
    if is_ignored(relative) || !is_interesting(relative) {
        return None;
    }
    utf8_path(relative).ok()
}

fn is_ignored(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        matches!(name.as_ref(), ".git" | "target" | "node_modules" | ".cache")
    })
}

fn is_interesting(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    if file_name.ends_with(".dawn") || file_name.ends_with(".effect.dawn") {
        return true;
    }
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp3" | "wav" | "flac" | "m4a" | "aac" | "ogg"
            )
        })
}
