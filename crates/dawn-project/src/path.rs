use std::path::{Component, Path, PathBuf};

pub use camino::{Utf8Path, Utf8PathBuf};

pub fn canonicalize_path(path: &Utf8Path) -> Utf8PathBuf {
    std::fs::canonicalize(path.as_std_path())
        .ok()
        .and_then(|path| Utf8PathBuf::from_path_buf(path).ok())
        .unwrap_or_else(|| lexical_absolute(path))
}

pub fn resolve_import_path(source_path: &Utf8Path, import_path: &Utf8Path) -> Utf8PathBuf {
    let resolved = if import_path.is_absolute() {
        import_path.to_path_buf()
    } else {
        source_path
            .parent()
            .map(|parent| parent.join(import_path))
            .unwrap_or_else(|| import_path.to_path_buf())
    };
    canonicalize_path(&resolved)
}

pub fn serialized_import_path(source_path: &Utf8Path, target_path: &Utf8Path) -> String {
    let source_parent = source_path.parent().unwrap_or_else(|| Utf8Path::new(""));
    let relative = pathdiff::diff_paths(target_path.as_std_path(), source_parent.as_std_path())
        .and_then(|path| Utf8PathBuf::from_path_buf(path).ok())
        .filter(|path| path.is_relative())
        .unwrap_or_else(|| target_path.to_path_buf());
    slash_path(&relative)
}

pub fn slash_path(path: &Utf8Path) -> String {
    path.as_str().replace('\\', "/")
}

pub trait PathStringExt {
    fn to_slash_string(&self) -> String;
}

impl PathStringExt for Utf8Path {
    fn to_slash_string(&self) -> String {
        slash_path(self)
    }
}

impl PathStringExt for Utf8PathBuf {
    fn to_slash_string(&self) -> String {
        slash_path(self)
    }
}

pub fn utf8_path(path: impl AsRef<Path>) -> Result<Utf8PathBuf, String> {
    Utf8PathBuf::from_path_buf(path.as_ref().to_path_buf())
        .map_err(|path| format!("path is not valid UTF-8: {}", path.display()))
}

pub fn lexical_normalize(path: &Utf8Path) -> Utf8PathBuf {
    let normalized = lexical_normalize_std(path.as_std_path());
    Utf8PathBuf::from_path_buf(normalized).unwrap_or_else(|path| {
        panic!(
            "normalizing a UTF-8 path produced non-UTF-8 path `{}`",
            path.display()
        )
    })
}

fn lexical_absolute(path: &Utf8Path) -> Utf8PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let current_dir = std::env::current_dir()
            .ok()
            .and_then(|path| Utf8PathBuf::from_path_buf(path).ok())
            .unwrap_or_default();
        current_dir.join(path)
    };
    lexical_normalize(&absolute)
}

fn lexical_normalize_std(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}
