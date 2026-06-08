use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use crate::effect_script::{
    lex as lex_effect_script, parse_module as parse_effect_module, EffectEntrypoint,
};
use crate::fs::WorkspaceFs;
use crate::lower::{
    lower_project, select_referenced_object, LowerError, ResolvedImport, SymbolResolver,
};
use crate::model::{
    AuthoredEffectDefinitionSource, DawnFile, DawnImport, DawnObject, DawnProject,
    EffectDefinition, NoCompiledEffect, ObjectKind, SymbolRef,
};
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
) -> Result<DawnProject, LoadProjectError> {
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
        expected: ObjectKind,
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
        import_targets(source_path, &imports[0], &self.fs, reference, expected)
    }
}

impl SymbolResolver for FsImportLoader {
    fn resolve_object(
        &mut self,
        source_path: &Utf8PathBuf,
        reference: &SymbolRef,
        expected: ObjectKind,
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
                symbol: reference.name().as_str().to_string(),
                object,
            });
        }

        let Some(alias) = reference.alias() else {
            unreachable!("unaliased references are resolved in the local-file branch");
        };
        let mut matches = Vec::new();
        for import_path in self.import_paths_for_alias(source_path, alias, reference, expected)? {
            let file = self
                .load_cached(&import_path)
                .map_err(|error| LowerError::Import {
                    reference: reference.raw().to_string(),
                    message: error.to_string(),
                })?;
            if let Some(object) = file.get(reference.name().as_str()) {
                matches.push(ResolvedImport {
                    source_path: import_path,
                    symbol: reference.name().as_str().to_string(),
                    object: object.clone(),
                });
            }
        }
        single_match(matches, reference)
    }

    fn resolve_effect_alias(
        &mut self,
        source_path: &Utf8PathBuf,
        alias: &str,
        reference: &str,
    ) -> Result<Vec<ResolvedImport>, LowerError> {
        let alias_reference =
            SymbolRef::new(format!("{alias}.__effect_import__")).map_err(|message| {
                LowerError::Import {
                    reference: reference.to_string(),
                    message,
                }
            })?;
        let mut matches = Vec::new();
        for import_path in
            self.import_paths_for_alias(source_path, alias, &alias_reference, ObjectKind::Effect)?
        {
            let file = self
                .load_cached(&import_path)
                .map_err(|error| LowerError::Import {
                    reference: reference.to_string(),
                    message: error.to_string(),
                })?;
            for (symbol, object) in file {
                let DawnObject::Effect(effect) = object else {
                    continue;
                };
                if effect.visibility != crate::effect_script::EffectVisibility::Addable {
                    continue;
                }
                matches.push(ResolvedImport {
                    source_path: import_path.clone(),
                    symbol: symbol.clone(),
                    object: object.clone(),
                });
            }
        }
        let mut names = HashMap::<String, Utf8PathBuf>::new();
        for resolved in &matches {
            if let Some(first_path) =
                names.insert(resolved.symbol.clone(), resolved.source_path.clone())
            {
                return Err(LowerError::Import {
                    reference: reference.to_string(),
                    message: format!(
                        "effect `{}` is exported by both `{}` and `{}`",
                        resolved.symbol, first_path, resolved.source_path
                    ),
                });
            }
        }
        Ok(matches)
    }
}

pub fn load_dawn_file(fs: &WorkspaceFs, path: &Utf8PathBuf) -> Result<DawnFile, LoadProjectError> {
    let text = fs
        .read_to_string(path)
        .map_err(|source| LoadProjectError::Io {
            path: path.clone(),
            source,
        })?;
    if is_effect_dawn_path(path) {
        return load_effect_file(path, text).map_err(LoadProjectError::Lower);
    }
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
    expected: ObjectKind,
) -> Result<Vec<Utf8PathBuf>, LowerError> {
    let path = resolve_import_path(source_path, import_from);
    if fs.is_file(&path) {
        if !is_importable_dawn_path(&path, expected) {
            return Ok(Vec::new());
        }
        return Ok(vec![canonicalize_path(&path)]);
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
        if entry_path.is_file() && is_importable_dawn_path(&entry_path, expected) {
            paths.push(canonicalize_path(&entry_path));
        }
    }
    paths.sort();
    Ok(paths)
}

fn load_effect_file(path: &Utf8PathBuf, text: String) -> Result<DawnFile, LowerError> {
    let tokens = lex_effect_script(&text).map_err(|diagnostics| LowerError::Import {
        reference: path.to_string(),
        message: first_script_diagnostic(diagnostics, "effect file did not produce tokens"),
    })?;
    let module = parse_effect_module(&tokens).map_err(|diagnostics| LowerError::Import {
        reference: path.to_string(),
        message: first_script_diagnostic(diagnostics, "effect file did not declare an effect"),
    })?;
    let imports = module
        .imports
        .iter()
        .map(|import| DawnImport {
            from: Utf8PathBuf::from(import.path.as_str()),
            alias: import.alias.clone(),
        })
        .collect();
    let mut file = DawnFile {
        imports,
        objects: Default::default(),
    };
    for effect in module.effects {
        file.insert(
            effect.name.clone(),
            DawnObject::Effect(EffectDefinition {
                source: AuthoredEffectDefinitionSource { text: text.clone() },
                schema: effect.params,
                kind: match effect.entrypoint {
                    EffectEntrypoint::Sample(_) => crate::effect_script::EffectScriptKind::Sample,
                    EffectEntrypoint::Generator(_) => {
                        crate::effect_script::EffectScriptKind::Generator
                    }
                },
                visibility: effect.visibility,
                compiled: NoCompiledEffect,
            }),
        );
    }
    Ok(file)
}

fn first_script_diagnostic(
    diagnostics: Vec<crate::effect_script::ScriptDiagnostic>,
    empty_message: &str,
) -> String {
    diagnostics
        .first()
        .map(|diagnostic| diagnostic.message.clone())
        .unwrap_or_else(|| empty_message.to_string())
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

fn is_importable_dawn_path(path: &Utf8PathBuf, expected: ObjectKind) -> bool {
    match expected {
        ObjectKind::Effect => is_effect_dawn_path(path),
        _ => is_dawn_path(path) && !is_effect_dawn_path(path),
    }
}

fn is_dawn_path(path: &Utf8PathBuf) -> bool {
    path.file_name()
        .is_some_and(|name| name.ends_with(".dawn") && !name.ends_with(".schema.dawn"))
}

fn is_effect_dawn_path(path: &Utf8PathBuf) -> bool {
    path.file_name()
        .is_some_and(|name| name.ends_with(".effect.dawn"))
}
