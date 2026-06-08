use std::collections::HashMap;

use indexmap::IndexMap;

use crate::diagnostics::{
    DiagnosticSeverity, ProjectDiagnostic, ProjectDiagnosticKind, ProjectLoadResult,
};
use crate::effect_script::{
    lex as lex_effect_script, parse_module as parse_effect_module, EffectEntrypoint,
};
use crate::fs::WorkspaceFs;
use crate::lower::{
    lower_project, select_referenced_object, LowerError, ResolvedImport, SymbolResolver,
};
use crate::model::{
    AuthoredEffectDefinitionSource, DawnFile, DawnImport, DawnObject, DawnProject,
    EffectDefinition, NoCompiledEffect, ObjectKind, ProjectDefinitionKey, ResolvedSourceFile,
    ResolvedSourceObject, ResolvedStores, SymbolRef,
};
use crate::parse::{parse_dawn_file_with_source_map, DawnParseDiagnostic, YamlSourceMap};
use crate::path::{canonicalize_path, resolve_import_path, Utf8PathBuf};

#[derive(Debug)]
struct LoadFailure {
    diagnostics: Vec<ProjectDiagnostic>,
}

pub fn load_project(
    fs: &WorkspaceFs,
    project_path: Utf8PathBuf,
    project_key: &str,
) -> ProjectLoadResult {
    match load_project_fallible(fs, project_path, project_key) {
        Ok(project) => ProjectLoadResult {
            project: Some(project),
            diagnostics: Vec::new(),
        },
        Err(failure) => ProjectLoadResult {
            project: None,
            diagnostics: failure.diagnostics,
        },
    }
}

fn load_project_fallible(
    fs: &WorkspaceFs,
    project_path: Utf8PathBuf,
    project_key: &str,
) -> Result<DawnProject, LoadFailure> {
    let project_path = canonicalize_path(&fs.resolve(&project_path));
    let mut loader = FsImportLoader::new(fs.clone());
    let file = load_dawn_file(fs, &project_path)?;
    loader.files.insert(project_path.clone(), file.clone());
    let root_project = ProjectDefinitionKey::new(project_path.clone(), project_key.to_string());

    let lowered = lower_project(&file.file, project_key, &project_path, &mut loader);
    let mut project = lowered.map_err(|error| {
        if loader.diagnostics.is_empty() {
            LoadFailure {
                diagnostics: lower_error_diagnostics(&project_path, &error),
            }
        } else {
            LoadFailure {
                diagnostics: loader.diagnostics.clone(),
            }
        }
    })?;
    project.stores.root_project = Some(root_project);
    project.stores.source_files =
        loader.source_files(project.stores.root_project.as_ref(), &project.stores);
    Ok(project)
}

#[derive(Debug, Clone)]
struct LoadedDawnFile {
    file: DawnFile,
    source: LoadedSourceFile,
    _source_map: Option<YamlSourceMap>,
}

#[derive(Debug, Clone)]
enum LoadedSourceFile {
    Dawn,
    Effect { text: String },
}

struct FsImportLoader {
    fs: WorkspaceFs,
    files: IndexMap<Utf8PathBuf, LoadedDawnFile>,
    diagnostics: Vec<ProjectDiagnostic>,
}

impl FsImportLoader {
    fn new(fs: WorkspaceFs) -> Self {
        Self {
            fs,
            files: IndexMap::new(),
            diagnostics: Vec::new(),
        }
    }

    fn load_cached(&mut self, path: &Utf8PathBuf) -> Result<&DawnFile, LoadFailure> {
        if !self.files.contains_key(path) {
            match load_dawn_file(&self.fs, path) {
                Ok(file) => {
                    self.files.insert(path.clone(), file);
                }
                Err(failure) => {
                    self.diagnostics.extend(failure.diagnostics.clone());
                    return Err(failure);
                }
            }
        }
        let Some(file) = self.files.get(path) else {
            return Err(LoadFailure {
                diagnostics: vec![project_diagnostic(
                    path.clone(),
                    None,
                    "loaded file was not cached".to_string(),
                    ProjectDiagnosticKind::Io,
                )],
            });
        };
        Ok(&file.file)
    }

    fn source_files(
        &self,
        root_project: Option<&ProjectDefinitionKey>,
        stores: &ResolvedStores,
    ) -> IndexMap<Utf8PathBuf, ResolvedSourceFile> {
        self.files
            .iter()
            .map(|(path, loaded)| {
                let source = match &loaded.source {
                    LoadedSourceFile::Dawn => ResolvedSourceFile::Dawn {
                        imports: loaded.file.imports.clone(),
                        objects: loaded
                            .file
                            .iter()
                            .map(|(name, object)| {
                                (
                                    name.clone(),
                                    source_object_slot(path, name, object, root_project, stores),
                                )
                            })
                            .collect(),
                    },
                    LoadedSourceFile::Effect { text } => {
                        ResolvedSourceFile::Effect { text: text.clone() }
                    }
                };
                (path.clone(), source)
            })
            .collect()
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
                message: first_project_diagnostic_message(&error.diagnostics),
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
                    message: first_project_diagnostic_message(&error.diagnostics),
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
                    message: first_project_diagnostic_message(&error.diagnostics),
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
                    message: first_project_diagnostic_message(&error.diagnostics),
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

fn load_dawn_file(fs: &WorkspaceFs, path: &Utf8PathBuf) -> Result<LoadedDawnFile, LoadFailure> {
    let text = fs.read_to_string(path).map_err(|source| LoadFailure {
        diagnostics: vec![project_diagnostic(
            path.clone(),
            None,
            format!("failed to read `{path}`: {source}"),
            ProjectDiagnosticKind::Io,
        )],
    })?;
    if is_effect_dawn_path(path) {
        return load_effect_file(path, text);
    }
    parse_dawn_file_with_source_map(&text)
        .map(|parsed| LoadedDawnFile {
            file: parsed.file,
            source: LoadedSourceFile::Dawn,
            _source_map: Some(parsed.source_map),
        })
        .map_err(|source| LoadFailure {
            diagnostics: vec![parse_diagnostic(path.clone(), source)],
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

fn load_effect_file(path: &Utf8PathBuf, text: String) -> Result<LoadedDawnFile, LoadFailure> {
    let tokens = lex_effect_script(&text).map_err(|diagnostics| LoadFailure {
        diagnostics: script_diagnostics(path, diagnostics),
    })?;
    let module = parse_effect_module(&tokens).map_err(|diagnostics| LoadFailure {
        diagnostics: script_diagnostics(path, diagnostics),
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
    Ok(LoadedDawnFile {
        file,
        source: LoadedSourceFile::Effect { text },
        _source_map: None,
    })
}

fn source_object_slot(
    path: &Utf8PathBuf,
    name: &str,
    object: &DawnObject<crate::model::Authored>,
    root_project: Option<&ProjectDefinitionKey>,
    stores: &ResolvedStores,
) -> ResolvedSourceObject {
    match object {
        DawnObject::Project(_) => {
            let key = ProjectDefinitionKey::new(path.clone(), name.to_string());
            if root_project.is_some_and(|root| root == &key) {
                ResolvedSourceObject::Project(key)
            } else {
                ResolvedSourceObject::Unused(object.clone())
            }
        }
        DawnObject::Display(_) => source_store_slot(
            path,
            name,
            object,
            &stores.displays,
            ResolvedSourceObject::Display,
        ),
        DawnObject::Controller(_) => source_store_slot(
            path,
            name,
            object,
            &stores.controllers,
            ResolvedSourceObject::Controller,
        ),
        DawnObject::Layout(_) => source_store_slot(
            path,
            name,
            object,
            &stores.layouts,
            ResolvedSourceObject::Layout,
        ),
        DawnObject::Fixture(_) => source_store_slot(
            path,
            name,
            object,
            &stores.fixture_definitions,
            ResolvedSourceObject::Fixture,
        ),
        DawnObject::Patch(_) => source_store_slot(
            path,
            name,
            object,
            &stores.patches,
            ResolvedSourceObject::Patch,
        ),
        DawnObject::Sequence(_) => source_store_slot(
            path,
            name,
            object,
            &stores.sequences,
            ResolvedSourceObject::Sequence,
        ),
        DawnObject::Curve(_) => source_store_slot(
            path,
            name,
            object,
            &stores.curves,
            ResolvedSourceObject::Curve,
        ),
        DawnObject::Effect(_) => ResolvedSourceObject::Unused(object.clone()),
    }
}

fn source_store_slot<K, V>(
    path: &Utf8PathBuf,
    name: &str,
    object: &DawnObject<crate::model::Authored>,
    store: &IndexMap<K, V>,
    make_slot: impl FnOnce(K) -> ResolvedSourceObject,
) -> ResolvedSourceObject
where
    K: Clone + Eq + std::hash::Hash,
    K: FromDefinitionKey,
{
    let key = K::from_definition_parts(path.clone(), name.to_string());
    if store.contains_key(&key) {
        make_slot(key)
    } else {
        ResolvedSourceObject::Unused(object.clone())
    }
}

trait FromDefinitionKey {
    fn from_definition_parts(path: Utf8PathBuf, name: String) -> Self;
}

macro_rules! impl_from_definition_key {
    ($type:ty) => {
        impl FromDefinitionKey for $type {
            fn from_definition_parts(path: Utf8PathBuf, name: String) -> Self {
                Self::new(path, name)
            }
        }
    };
}

impl_from_definition_key!(crate::model::DisplayDefinitionKey);
impl_from_definition_key!(crate::model::ControllerDefinitionKey);
impl_from_definition_key!(crate::model::LayoutDefinitionKey);
impl_from_definition_key!(crate::model::FixtureDefinitionKey);
impl_from_definition_key!(crate::model::PatchDefinitionKey);
impl_from_definition_key!(crate::model::SequenceDefinitionKey);
impl_from_definition_key!(crate::model::CurveDefinitionKey);

fn first_project_diagnostic_message(diagnostics: &[ProjectDiagnostic]) -> String {
    diagnostics
        .first()
        .map(|diagnostic| diagnostic.message.clone())
        .unwrap_or_else(|| "project did not load".to_string())
}

fn parse_diagnostic(path: Utf8PathBuf, diagnostic: DawnParseDiagnostic) -> ProjectDiagnostic {
    project_diagnostic(path, diagnostic.range, diagnostic.message, diagnostic.kind)
}

fn script_diagnostics(
    path: &Utf8PathBuf,
    diagnostics: Vec<crate::effect_script::ScriptDiagnostic>,
) -> Vec<ProjectDiagnostic> {
    if diagnostics.is_empty() {
        return vec![project_diagnostic(
            path.clone(),
            None,
            "effect script did not compile".to_string(),
            ProjectDiagnosticKind::EffectScript,
        )];
    }
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            project_diagnostic(
                path.clone(),
                diagnostic.range,
                diagnostic.message,
                ProjectDiagnosticKind::EffectScript,
            )
        })
        .collect()
}

fn lower_error_diagnostics(
    source_path: &Utf8PathBuf,
    error: &LowerError,
) -> Vec<ProjectDiagnostic> {
    match error {
        LowerError::Import { .. } => vec![project_diagnostic(
            source_path.clone(),
            None,
            error.to_string(),
            ProjectDiagnosticKind::Import,
        )],
        LowerError::EffectCompile {
            source_path,
            diagnostics,
            ..
        } => script_diagnostics(source_path, diagnostics.clone()),
        _ => vec![project_diagnostic(
            source_path.clone(),
            None,
            error.to_string(),
            ProjectDiagnosticKind::Lower,
        )],
    }
}

fn project_diagnostic(
    file: Utf8PathBuf,
    range: Option<crate::diagnostics::TextRange>,
    message: String,
    kind: ProjectDiagnosticKind,
) -> ProjectDiagnostic {
    ProjectDiagnostic {
        severity: DiagnosticSeverity::Error,
        file,
        range,
        message,
        kind,
    }
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
