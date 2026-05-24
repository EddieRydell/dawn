use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use crate::fs::ProjectFs;
use crate::lower::{lower_project, select_imported_object, LowerError, ResolvedImport};
use crate::model::{DawnFile, ImportRef, ObjectKind, ResolvedProject};
use crate::path::{resolve_import_file_path, ProjectPath};

#[derive(Debug)]
pub enum LoadProjectError {
    Io {
        path: ProjectPath,
        source: std::io::Error,
    },
    Yaml {
        path: ProjectPath,
        source: serde_yaml::Error,
    },
    Lower(LowerError),
}

impl fmt::Display for LoadProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "failed to read `{}`: {source}", path.display())
            }
            Self::Yaml { path, source } => {
                write!(formatter, "failed to parse `{}`: {source}", path.display())
            }
            Self::Lower(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for LoadProjectError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Yaml { source, .. } => Some(source),
            Self::Lower(source) => Some(source),
        }
    }
}

impl From<LowerError> for LoadProjectError {
    fn from(error: LowerError) -> Self {
        Self::Lower(error)
    }
}

pub fn load_project(
    fs: &ProjectFs,
    project_path: ProjectPath,
    project_key: &str,
) -> Result<ResolvedProject, LoadProjectError> {
    let file = load_dawn_file(fs, &project_path)?;
    let mut loader = FsImportLoader::new(fs.clone());

    lower_project(
        &file,
        project_key,
        &project_path,
        |source_path, import, expected| loader.resolve(source_path, import, expected),
    )
    .map_err(LoadProjectError::Lower)
}
struct FsImportLoader {
    fs: ProjectFs,
    files: HashMap<ProjectPath, DawnFile>,
}

impl FsImportLoader {
    fn new(fs: ProjectFs) -> Self {
        Self {
            fs,
            files: HashMap::new(),
        }
    }

    fn resolve(
        &mut self,
        source_path: &ProjectPath,
        import: &ImportRef,
        _expected: ObjectKind,
    ) -> Result<ResolvedImport, LowerError> {
        let import_path =
            resolve_import_file_path(source_path, import.path()).map_err(|message| {
                LowerError::Import {
                    import: import.raw().to_string(),
                    message,
                }
            })?;
        let file = self
            .load_cached(&import_path)
            .map_err(|error| LowerError::Import {
                import: import.raw().to_string(),
                message: error.to_string(),
            })?;
        let object = select_imported_object(file, import)?;

        Ok(ResolvedImport {
            source_path: import_path,
            object,
        })
    }

    fn load_cached(&mut self, path: &ProjectPath) -> Result<&DawnFile, LoadProjectError> {
        if !self.files.contains_key(path) {
            let file = load_dawn_file(&self.fs, path)?;
            self.files.insert(path.clone(), file);
        }
        Ok(self
            .files
            .get(path)
            .expect("file was inserted before lookup"))
    }
}
pub fn load_dawn_file(fs: &ProjectFs, path: &ProjectPath) -> Result<DawnFile, LoadProjectError> {
    let text = fs
        .read_to_string(path)
        .map_err(|source| LoadProjectError::Io {
            path: path.clone(),
            source,
        })?;
    serde_yaml::from_str(&text).map_err(|source| LoadProjectError::Yaml {
        path: path.clone(),
        source,
    })
}
