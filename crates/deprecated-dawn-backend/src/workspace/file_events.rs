use std::path::Path;

use dawn_language::path::{utf8_path, Utf8PathBuf};

pub(crate) fn project_path_from_event(root: &Path, path: &Path) -> Option<Utf8PathBuf> {
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
