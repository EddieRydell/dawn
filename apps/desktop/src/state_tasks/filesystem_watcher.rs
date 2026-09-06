use camino::Utf8Path;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// Always reconcile the complete inventory: rename pairs and rescan events do
/// not reliably identify all changed sources on every filesystem.
pub(crate) fn watch_project(
    root: &Utf8Path,
    on_change: impl Fn(Result<(), String>) + Send + 'static,
) -> Result<RecommendedWatcher, String> {
    let mut watcher =
        notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
            Ok(event)
                if event.need_rescan() || !matches!(event.kind, notify::EventKind::Access(_)) =>
            {
                on_change(Ok(()))
            }
            Ok(_) => {}
            Err(error) => on_change(Err(error.to_string())),
        })
        .map_err(|error| error.to_string())?;
    watcher
        .watch(root.as_std_path(), RecursiveMode::Recursive)
        .map_err(|error| error.to_string())?;
    Ok(watcher)
}
