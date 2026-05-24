use std::path::{Component, Path, PathBuf};

use serde::de;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectPath(PathBuf);

impl ProjectPath {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self::parse(path).expect("project path must be relative and contained")
    }

    pub fn parse(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let normalized = normalize_project_path(path)?;
        Ok(Self(normalized))
    }

    pub fn root() -> Self {
        Self(PathBuf::new())
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn parent(&self) -> Option<Self> {
        self.0.parent().map(Self::new)
    }

    pub fn join(&self, path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        if path.is_absolute() {
            return Err("project path must be relative".to_string());
        }
        Self::parse(self.0.join(path))
    }

    pub fn file_name(&self) -> Option<&std::ffi::OsStr> {
        self.0.file_name()
    }

    pub fn starts_with(&self, base: &ProjectPath) -> bool {
        self.0.starts_with(&base.0)
    }

    pub fn is_root(&self) -> bool {
        self.0.as_os_str().is_empty()
    }

    pub fn to_slash_string(&self) -> String {
        path_to_slash_string(&self.0)
    }

    pub fn display(&self) -> std::path::Display<'_> {
        self.0.display()
    }
}

impl AsRef<Path> for ProjectPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl Serialize for ProjectPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_slash_string())
    }
}

impl<'de> Deserialize<'de> for ProjectPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::parse(raw).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPath(PathBuf);

impl ImportPath {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self(path.as_ref().to_path_buf())
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn to_slash_string(&self) -> String {
        path_to_slash_string(&self.0)
    }
}

pub(crate) fn resolve_import_file_path(
    source_path: &ProjectPath,
    import_path: &ImportPath,
) -> Result<ProjectPath, String> {
    if import_path.as_path().is_absolute() {
        return Err("absolute imports are not allowed".to_string());
    }

    source_path
        .parent()
        .map(|parent| parent.join(import_path.as_path()))
        .unwrap_or_else(|| ProjectPath::parse(import_path.as_path()))
}

pub(crate) fn relative_import_path(source_path: &ProjectPath, target_path: &ProjectPath) -> String {
    let Some(source_parent) = source_path.as_path().parent() else {
        return target_path.to_slash_string();
    };
    path_to_slash_string(&relative_path_between(source_parent, target_path.as_path()))
}

fn relative_path_between(from_dir: &Path, target_path: &Path) -> PathBuf {
    let from = lexically_normalize_path(from_dir);
    let target = lexically_normalize_path(target_path);
    let from_components = from.components().collect::<Vec<_>>();
    let target_components = target.components().collect::<Vec<_>>();

    let mut common = 0;
    while common < from_components.len()
        && common < target_components.len()
        && from_components[common] == target_components[common]
    {
        common += 1;
    }

    if common == 0 {
        return target;
    }

    let mut relative = PathBuf::new();
    for component in &from_components[common..] {
        if matches!(component, Component::Normal(_)) {
            relative.push("..");
        }
    }
    for component in &target_components[common..] {
        relative.push(component.as_os_str());
    }

    if relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        relative
    }
}

fn normalize_project_path(path: &Path) -> Result<PathBuf, String> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                return Err("project path must be relative".to_string());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err("project path escapes the project root".to_string());
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    Ok(normalized)
}

fn lexically_normalize_path(path: &Path) -> PathBuf {
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

pub(crate) fn path_to_slash_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
