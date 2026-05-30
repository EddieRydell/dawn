use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use crate::fs::WorkspaceFs;
use crate::lower::{
    lower_project, select_referenced_object, LowerError, ResolvedEffectImport, ResolvedImport,
    SymbolResolver,
};
use crate::model::{DawnFile, ObjectKind, ResolvedProject, SymbolRef};
use crate::parse::{parse_dawn_file_with_source_map, DawnParseDiagnostic};
use crate::path::{canonicalize_path, resolve_import_path, Utf8PathBuf};

#[derive(Debug)]
pub enum LoadProjectError {
    Io {
        path: Utf8PathBuf,
        source: std::io::Error,
    },
    Yaml {
        path: Utf8PathBuf,
        source: DawnParseDiagnostic,
    },
    Lower(LowerError),
}

impl fmt::Display for LoadProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "failed to read `{}`: {source}", path)
            }
            Self::Yaml { path, source } => {
                write!(formatter, "failed to parse `{}`: {source}", path)
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
    fs: &WorkspaceFs,
    project_path: Utf8PathBuf,
    project_key: &str,
) -> Result<ResolvedProject, LoadProjectError> {
    let project_path = canonicalize_path(&fs.resolve(&project_path));
    let file = load_dawn_file(fs, &project_path)?;
    let mut loader = FsImportLoader::new(fs.clone());

    lower_project(&file, project_key, &project_path, &mut loader).map_err(LoadProjectError::Lower)
}
struct FsImportLoader {
    fs: WorkspaceFs,
    files: HashMap<Utf8PathBuf, DawnFile>,
}

impl FsImportLoader {
    fn new(fs: WorkspaceFs) -> Self {
        Self {
            fs,
            files: HashMap::new(),
        }
    }

    fn load_cached(&mut self, path: &Utf8PathBuf) -> Result<&DawnFile, LoadProjectError> {
        match self.files.entry(path.clone()) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => {
                let file = load_dawn_file(&self.fs, path)?;
                Ok(entry.insert(file))
            }
        }
    }

    fn import_paths_for_alias(
        &mut self,
        source_path: &Utf8PathBuf,
        alias: &str,
        reference: &SymbolRef,
    ) -> Result<Vec<Utf8PathBuf>, LowerError> {
        let file = self
            .load_cached(source_path)
            .map_err(|error| LowerError::Import {
                reference: reference.raw().to_string(),
                message: error.to_string(),
            })?;
        let imports = file
            .imports
            .iter()
            .filter(|import| import.alias == alias)
            .map(|import| import.from.clone())
            .collect::<Vec<_>>();
        if imports.is_empty() {
            return Err(LowerError::Import {
                reference: reference.raw().to_string(),
                message: format!("alias `{alias}` was not imported"),
            });
        }
        if imports.len() > 1 {
            return Err(LowerError::Import {
                reference: reference.raw().to_string(),
                message: format!("alias `{alias}` is imported more than once"),
            });
        }
        import_targets(source_path, &imports[0], &self.fs, reference)
    }
}

impl SymbolResolver for FsImportLoader {
    fn resolve_object(
        &mut self,
        source_path: &Utf8PathBuf,
        reference: &SymbolRef,
        _expected: ObjectKind,
    ) -> Result<ResolvedImport, LowerError> {
        if reference.alias().is_none() {
            let file = self
                .load_cached(source_path)
                .map_err(|error| LowerError::Import {
                    reference: reference.raw().to_string(),
                    message: error.to_string(),
                })?;
            let object = select_referenced_object(file, reference)?;
            return Ok(ResolvedImport {
                source_path: source_path.clone(),
                object,
            });
        }

        let mut matches = Vec::new();
        for import_path in
            self.import_paths_for_alias(source_path, reference.alias().unwrap(), reference)?
        {
            let file = self
                .load_cached(&import_path)
                .map_err(|error| LowerError::Import {
                    reference: reference.raw().to_string(),
                    message: error.to_string(),
                })?;
            if let Some(object) = file.get(reference.name().as_str()) {
                matches.push(ResolvedImport {
                    source_path: import_path,
                    object: object.clone(),
                });
            }
        }
        single_match(matches, reference)
    }

    fn resolve_effect(
        &mut self,
        source_path: &Utf8PathBuf,
        reference: &SymbolRef,
    ) -> Result<ResolvedEffectImport, LowerError> {
        let Some(alias) = reference.alias() else {
            return Err(LowerError::Import {
                reference: reference.raw().to_string(),
                message: "effect references must use an imported alias".to_string(),
            });
        };
        let mut matches = Vec::new();
        for import_path in self.import_paths_for_alias(source_path, alias, reference)? {
            if effect_name_for_path(&import_path)? == reference.name().as_str() {
                matches.push(ResolvedEffectImport {
                    source_path: import_path,
                });
            }
        }
        single_match(matches, reference)
    }
}

pub fn load_dawn_file(fs: &WorkspaceFs, path: &Utf8PathBuf) -> Result<DawnFile, LoadProjectError> {
    let text = fs
        .read_to_string(path)
        .map_err(|source| LoadProjectError::Io {
            path: path.clone(),
            source,
        })?;
    parse_dawn_file_with_source_map(&text)
        .map(|parsed| parsed.file)
        .map_err(|source| LoadProjectError::Yaml {
            path: path.clone(),
            source,
        })
}

fn import_targets(
    source_path: &Utf8PathBuf,
    import_from: &Utf8PathBuf,
    fs: &WorkspaceFs,
    reference: &SymbolRef,
) -> Result<Vec<Utf8PathBuf>, LowerError> {
    let path = resolve_import_path(source_path, import_from);
    if fs.is_file(&path) {
        return Ok(vec![path]);
    }
    if !fs.is_dir(&path) {
        return Err(LowerError::Import {
            reference: reference.raw().to_string(),
            message: format!("import path `{}` was not found", path),
        });
    }
    let mut paths = Vec::new();
    let entries = std::fs::read_dir(path.as_std_path()).map_err(|error| LowerError::Import {
        reference: reference.raw().to_string(),
        message: error.to_string(),
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| LowerError::Import {
            reference: reference.raw().to_string(),
            message: error.to_string(),
        })?;
        let entry_path =
            Utf8PathBuf::from_path_buf(entry.path()).map_err(|path| LowerError::Import {
                reference: reference.raw().to_string(),
                message: format!("path is not valid UTF-8: {}", path.display()),
            })?;
        if entry_path.is_file() && is_dawn_path(&entry_path) {
            paths.push(canonicalize_path(&entry_path));
        }
    }
    paths.sort();
    Ok(paths)
}

fn single_match<T>(mut matches: Vec<T>, reference: &SymbolRef) -> Result<T, LowerError> {
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(LowerError::Import {
            reference: reference.raw().to_string(),
            message: format!("symbol `{}` was not found", reference.name().as_str()),
        }),
        _ => Err(LowerError::Import {
            reference: reference.raw().to_string(),
            message: format!(
                "symbol `{}` is exported more than once",
                reference.name().as_str()
            ),
        }),
    }
}

fn effect_name_for_path(path: &Utf8PathBuf) -> Result<String, LowerError> {
    let text = std::fs::read_to_string(path.as_std_path()).map_err(|error| LowerError::Import {
        reference: path.to_string(),
        message: error.to_string(),
    })?;
    effect_name_from_source(&text).ok_or_else(|| LowerError::Import {
        reference: path.to_string(),
        message: "effect file did not declare an effect".to_string(),
    })
}

fn effect_name_from_source(source: &str) -> Option<String> {
    let rest = source.split_once("effect")?.1.trim_start();
    let name = rest
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .next()?;
    (!name.is_empty()).then(|| name.to_string())
}

fn is_dawn_path(path: &Utf8PathBuf) -> bool {
    path.file_name()
        .is_some_and(|name| name.ends_with(".dawn") && !name.ends_with(".schema.dawn"))
}
