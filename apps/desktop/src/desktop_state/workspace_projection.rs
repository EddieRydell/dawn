use std::collections::BTreeMap;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

#[derive(Clone, Copy)]
pub(crate) enum FsEntryKind {
    File,
    Directory,
}

pub(crate) fn canonical_relative_path(path: &Utf8Path) -> Utf8PathBuf {
    Utf8PathBuf::from(path.as_str().replace(std::path::MAIN_SEPARATOR, "/"))
}

pub(crate) fn collect_workspace_paths(
    root: &Utf8Path,
    relative: &Utf8Path,
    paths: &mut BTreeMap<Utf8PathBuf, FsEntryKind>,
) {
    let absolute = root.join(relative);
    let Ok(entries) = fs::read_dir(absolute) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let path = canonical_relative_path(&relative.join(name));
        let kind = if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            FsEntryKind::Directory
        } else {
            FsEntryKind::File
        };
        paths.insert(path.clone(), kind);
        if matches!(kind, FsEntryKind::Directory) {
            collect_workspace_paths(root, &path, paths);
        }
    }
}

pub(crate) fn insert_path_with_parents(
    paths: &mut BTreeMap<Utf8PathBuf, FsEntryKind>,
    path: &Utf8Path,
) {
    let mut current = Utf8PathBuf::new();
    let path = canonical_relative_path(path);
    for component in path.components() {
        let camino::Utf8Component::Normal(part) = component else {
            continue;
        };
        current.push(part);
        current = canonical_relative_path(&current);
        let kind = if current == path {
            FsEntryKind::File
        } else {
            FsEntryKind::Directory
        };
        paths.entry(current.clone()).or_insert(kind);
    }
}
