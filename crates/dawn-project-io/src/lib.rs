#![deny(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::effect::{
    CurveDefinition, CurveId, CurveSource, EffectDefinition, EffectDefinitionId, EffectInst,
    EffectInstId, EffectParamValue, EffectScope, EffectTarget,
};
use dawn_language::effect_dsl::{compile_effects, Identifier};
use dawn_language::model::{DawnProject, ProjectDefinitionStores, ProjectId, ProjectRoot};
use dawn_language::sequence::{
    AssetId, AutomationClip, AutomationClipId, MarkCollection, MarkCollectionKey, Sequence,
    SequenceAudio, SequenceId,
};
use dawn_language::setup::{
    ControllerAddress, ControllerDefinition, ControllerDefinitionId, ControllerId,
    ControllerOutput, ControllerOutputIndex, Display, DisplayId, FixtureDefinition,
    FixtureDefinitionId, FixtureGroup, FixtureGroupId, FixtureInst, FixtureInstanceId, Geometry,
    Layout, LayoutId, LayoutTarget, Patch, PatchId, PatchRoute, PixelRange, Protocol,
    RgbChannelOrder,
};
use dawn_language::values::{
    Color, Curve, CurvePoint, CurveValue, DawnDuration, DawnTime, Distance, DistanceSpan, Point3,
    Rotation3, Scale3,
};
use indexmap::{IndexMap, IndexSet};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::time::Duration;
use yaml_serde::{Mapping, Value};

pub fn load_project(path: &Utf8Path) -> Result<ProjectSession, LoadProjectError> {
    Loader::new(path)?.load()
}

pub fn export_project(
    session: &ProjectSession,
    output_root: &Utf8Path,
) -> Result<ExportReport, ExportProjectError> {
    if output_root.exists() && !output_root.is_dir() {
        return Err(ExportProjectError::OutputRootIsFile {
            path: output_root.to_path_buf(),
        });
    }
    fs::create_dir_all(output_root).map_err(|source| ExportProjectError::Io {
        path: output_root.to_path_buf(),
        source,
    })?;

    let written_files = write_source_documents(session, output_root)?;

    let mut copied_assets = Vec::new();
    for asset in &session.source.referenced_assets {
        let output_path = output_root.join(&asset.relative_path);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|source| ExportProjectError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::copy(&asset.absolute_path, &output_path).map_err(|source| ExportProjectError::Io {
            path: output_path.clone(),
            source,
        })?;
        copied_assets.push(asset.relative_path.clone());
    }

    Ok(ExportReport {
        written_files,
        copied_assets,
    })
}

pub fn save_project(session: &ProjectSession) -> Result<SaveReport, SaveProjectError> {
    let written_files = write_source_documents(session, &session.source.source_root)?;
    Ok(SaveReport { written_files })
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectSession {
    pub project: DawnProject,
    pub source: SourceProject,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceProject {
    pub source_root: Utf8PathBuf,
    pub entrypoint: Utf8PathBuf,
    pub documents: IndexMap<Utf8PathBuf, SourceDocument>,
    pub import_graph: IndexMap<Utf8PathBuf, Vec<ImportEdge>>,
    pub source_map: SourceMap,
    pub effect_source_text: IndexMap<EffectDefinitionId, String>,
    pub referenced_assets: Vec<ReferencedAsset>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceMap {
    pub objects: IndexMap<SourceObjectId, SourceObjectLocation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SourceObjectId {
    pub kind: SourceObjectKind,
    pub id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum SourceObjectKind {
    Project,
    Display,
    Controller,
    Layout,
    Patch,
    FixtureDefinition,
    Curve,
    Sequence,
    EffectDefinition,
    EffectInstance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceObjectLocation {
    pub document: Utf8PathBuf,
    pub object_key: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceDocument {
    pub relative_path: Utf8PathBuf,
    pub imports: Vec<ImportDecl>,
    pub exported_objects: Vec<String>,
    pub kind: SourceDocumentKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SourceDocumentKind {
    Dawn {
        value: Value,
        object_types: Vec<SourceObjectKind>,
    },
    Effect {
        source: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDecl {
    pub from: Utf8PathBuf,
    pub alias: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportEdge {
    pub alias: String,
    pub from: Utf8PathBuf,
    pub targets: Vec<Utf8PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferencedAsset {
    pub id: AssetId,
    pub relative_path: Utf8PathBuf,
    pub absolute_path: Utf8PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportReport {
    pub written_files: Vec<Utf8PathBuf>,
    pub copied_assets: Vec<Utf8PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveReport {
    pub written_files: Vec<Utf8PathBuf>,
}

#[derive(Debug)]
pub enum LoadProjectError {
    InvalidEntrypoint {
        path: Utf8PathBuf,
    },
    Io {
        path: Utf8PathBuf,
        source: io::Error,
    },
    ParseYaml {
        path: Utf8PathBuf,
        source: yaml_serde::Error,
    },
    InvalidDocument {
        path: Utf8PathBuf,
        message: String,
    },
    InvalidReference {
        path: Utf8PathBuf,
        reference: String,
    },
    InvalidEffect {
        path: Utf8PathBuf,
        diagnostics: Vec<String>,
    },
}

#[derive(Debug)]
pub enum ExportProjectError {
    OutputRootIsFile {
        path: Utf8PathBuf,
    },
    Io {
        path: Utf8PathBuf,
        source: io::Error,
    },
    Serialize {
        path: Utf8PathBuf,
        source: yaml_serde::Error,
    },
}

pub type SaveProjectError = ExportProjectError;

impl fmt::Display for LoadProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEntrypoint { path } => write!(formatter, "invalid entrypoint {path}"),
            Self::Io { path, source } => write!(formatter, "{path}: {source}"),
            Self::ParseYaml { path, source } => write!(formatter, "{path}: {source}"),
            Self::InvalidDocument { path, message } => write!(formatter, "{path}: {message}"),
            Self::InvalidReference { path, reference } => {
                write!(formatter, "{path}: invalid reference {reference}")
            }
            Self::InvalidEffect { path, diagnostics } => {
                write!(
                    formatter,
                    "{path}: invalid effect: {}",
                    diagnostics.join(", ")
                )
            }
        }
    }
}

impl std::error::Error for LoadProjectError {}

impl fmt::Display for ExportProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputRootIsFile { path } => write!(formatter, "output root is a file: {path}"),
            Self::Io { path, source } => write!(formatter, "{path}: {source}"),
            Self::Serialize { path, source } => write!(formatter, "{path}: {source}"),
        }
    }
}

impl std::error::Error for ExportProjectError {}

fn write_source_documents(
    session: &ProjectSession,
    output_root: &Utf8Path,
) -> Result<Vec<Utf8PathBuf>, ExportProjectError> {
    let mut written_files = Vec::new();
    for document in session.source.documents.values() {
        let output_path = output_root.join(&document.relative_path);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|source| ExportProjectError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        match &document.kind {
            SourceDocumentKind::Dawn { value, .. } => {
                let yaml = yaml_serde::to_string(value).map_err(|source| {
                    ExportProjectError::Serialize {
                        path: output_path.clone(),
                        source,
                    }
                })?;
                fs::write(&output_path, yaml).map_err(|source| ExportProjectError::Io {
                    path: output_path.clone(),
                    source,
                })?;
            }
            SourceDocumentKind::Effect { source } => {
                fs::write(&output_path, source).map_err(|source| ExportProjectError::Io {
                    path: output_path.clone(),
                    source,
                })?;
            }
        }
        written_files.push(document.relative_path.clone());
    }
    Ok(written_files)
}

struct Loader {
    source_root: Utf8PathBuf,
    entrypoint: Utf8PathBuf,
    documents: IndexMap<Utf8PathBuf, SourceDocument>,
    import_graph: IndexMap<Utf8PathBuf, Vec<ImportEdge>>,
    visible_objects: IndexMap<Utf8PathBuf, IndexMap<AliasObjectKey, ResolvedObject>>,
    loading_documents: IndexSet<Utf8PathBuf>,
    source_map: SourceMap,
    effect_source_text: IndexMap<EffectDefinitionId, String>,
    referenced_assets: Vec<ReferencedAsset>,
    next_asset_id: u32,
}

impl Loader {
    fn new(path: &Utf8Path) -> Result<Self, LoadProjectError> {
        let absolute = path
            .canonicalize_utf8()
            .map_err(|source| LoadProjectError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        if !absolute.is_file() {
            return Err(LoadProjectError::InvalidEntrypoint { path: absolute });
        }
        let source_root = absolute
            .parent()
            .ok_or_else(|| LoadProjectError::InvalidEntrypoint {
                path: absolute.clone(),
            })?
            .to_path_buf();
        let entrypoint = relative_path(&source_root, &absolute)?;
        Ok(Self {
            source_root,
            entrypoint,
            documents: IndexMap::new(),
            import_graph: IndexMap::new(),
            visible_objects: IndexMap::new(),
            loading_documents: IndexSet::new(),
            source_map: SourceMap::default(),
            effect_source_text: IndexMap::new(),
            referenced_assets: Vec::new(),
            next_asset_id: 1,
        })
    }

    fn load(mut self) -> Result<ProjectSession, LoadProjectError> {
        let entrypoint = self.entrypoint.clone();
        self.load_document(&entrypoint)?;
        let project = self.resolve_project(&entrypoint)?;
        self.normalize_source_root()?;
        let entrypoint = self.entrypoint.clone();
        Ok(ProjectSession {
            project,
            source: SourceProject {
                source_root: self.source_root,
                entrypoint,
                documents: self.documents,
                import_graph: self.import_graph,
                source_map: self.source_map,
                effect_source_text: self.effect_source_text,
                referenced_assets: self.referenced_assets,
            },
        })
    }

    fn load_document(&mut self, relative: &Utf8Path) -> Result<(), LoadProjectError> {
        if self.documents.contains_key(relative) {
            return Ok(());
        }
        if self.loading_documents.contains(relative) {
            return Ok(());
        }
        self.loading_documents.insert(relative.to_path_buf());
        let absolute = self.source_root.join(relative);
        let file_name = relative.file_name().unwrap_or_default();
        let result = if file_name.ends_with(".effect.dawn") {
            self.load_effect_document(relative, &absolute)
        } else {
            self.load_dawn_document(relative, &absolute)
        };
        self.loading_documents.shift_remove(relative);
        result
    }

    fn load_effect_document(
        &mut self,
        relative: &Utf8Path,
        absolute: &Utf8Path,
    ) -> Result<(), LoadProjectError> {
        let source = fs::read_to_string(absolute).map_err(|source| LoadProjectError::Io {
            path: absolute.to_path_buf(),
            source,
        })?;
        let compiled =
            compile_effects(&source).map_err(|diagnostics| LoadProjectError::InvalidEffect {
                path: relative.to_path_buf(),
                diagnostics: diagnostics
                    .into_iter()
                    .map(|diagnostic| {
                        format!(
                            "{}..{} {}",
                            diagnostic.span.start, diagnostic.span.end, diagnostic.message
                        )
                    })
                    .collect(),
            })?;
        let mut visible = IndexMap::new();
        let mut exported_objects = Vec::new();
        let mut object_types = Vec::new();
        for effect in compiled {
            let name = effect.name().as_str().to_string();
            let id = EffectDefinitionId(name.clone());
            self.effect_source_text.insert(id.clone(), source.clone());
            self.source_map.objects.insert(
                SourceObjectId {
                    kind: SourceObjectKind::EffectDefinition,
                    id: id.0.clone(),
                },
                SourceObjectLocation {
                    document: relative.to_path_buf(),
                    object_key: name.clone(),
                },
            );
            visible.insert(
                AliasObjectKey {
                    alias: None,
                    object: name.clone(),
                },
                ResolvedObject::EffectDefinition(id),
            );
            exported_objects.push(name);
            object_types.push(SourceObjectKind::EffectDefinition);
        }
        self.visible_objects.insert(relative.to_path_buf(), visible);
        self.import_graph.insert(relative.to_path_buf(), Vec::new());
        self.documents.insert(
            relative.to_path_buf(),
            SourceDocument {
                relative_path: relative.to_path_buf(),
                imports: Vec::new(),
                exported_objects,
                kind: SourceDocumentKind::Effect { source },
            },
        );
        Ok(())
    }

    fn load_dawn_document(
        &mut self,
        relative: &Utf8Path,
        absolute: &Utf8Path,
    ) -> Result<(), LoadProjectError> {
        let text = fs::read_to_string(absolute).map_err(|source| LoadProjectError::Io {
            path: absolute.to_path_buf(),
            source,
        })?;
        let value: Value =
            yaml_serde::from_str(&text).map_err(|source| LoadProjectError::ParseYaml {
                path: relative.to_path_buf(),
                source,
            })?;
        let map = mapping(&value).ok_or_else(|| LoadProjectError::InvalidDocument {
            path: relative.to_path_buf(),
            message: "document root must be a mapping".to_string(),
        })?;
        let imports = parse_imports(relative, map)?;
        let mut visible = IndexMap::new();

        let mut exported_objects = Vec::new();
        let mut object_types = Vec::new();
        for (key, object_value) in map {
            let Some(key) = key.as_str() else {
                return Err(LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    message: "object keys must be strings".to_string(),
                });
            };
            if key == "imports" {
                continue;
            }
            let object_type = string_field(relative, object_value, "type")?;
            let object = match object_type {
                "project" => ResolvedObject::Project(ProjectId(key.to_string())),
                "display" => ResolvedObject::Display(DisplayId(key.to_string())),
                "controller" => ResolvedObject::Controller(ControllerId(key.to_string())),
                "layout" => ResolvedObject::Layout(LayoutId(key.to_string())),
                "patch" => ResolvedObject::Patch(PatchId(key.to_string())),
                "fixture" => {
                    ResolvedObject::FixtureDefinition(FixtureDefinitionId(key.to_string()))
                }
                "curve" => ResolvedObject::Curve(CurveId(key.to_string())),
                "sequence" => ResolvedObject::Sequence(SequenceId(key.to_string())),
                other => {
                    return Err(LoadProjectError::InvalidDocument {
                        path: relative.to_path_buf(),
                        message: format!("unsupported object type `{other}`"),
                    })
                }
            };
            object_types.push(object.source_kind());
            self.source_map.objects.insert(
                SourceObjectId {
                    kind: object.source_kind(),
                    id: object.id_string(),
                },
                SourceObjectLocation {
                    document: relative.to_path_buf(),
                    object_key: key.to_string(),
                },
            );
            visible.insert(
                AliasObjectKey {
                    alias: None,
                    object: key.to_string(),
                },
                object,
            );
            exported_objects.push(key.to_string());
        }

        self.visible_objects.insert(relative.to_path_buf(), visible);

        let mut import_edges = Vec::new();
        let mut imported_visible = Vec::new();
        for import in &imports {
            let targets = self.resolve_import(relative, &import.from)?;
            let mut names = IndexSet::new();
            for target in &targets {
                self.load_document(target)?;
                let target_visible = self.visible_objects.get(target).ok_or_else(|| {
                    LoadProjectError::InvalidDocument {
                        path: target.clone(),
                        message: "loaded document had no visible object index".to_string(),
                    }
                })?;
                for (key, object) in target_visible {
                    if key.alias.is_some() {
                        continue;
                    }
                    if !names.insert(key.object.clone()) {
                        return Err(LoadProjectError::InvalidDocument {
                            path: relative.to_path_buf(),
                            message: format!(
                                "duplicate exported object `{}` in import alias `{}`",
                                key.object, import.alias
                            ),
                        });
                    }
                    imported_visible.push((
                        AliasObjectKey {
                            alias: Some(import.alias.clone()),
                            object: key.object.clone(),
                        },
                        object.clone(),
                    ));
                }
            }
            import_edges.push(ImportEdge {
                alias: import.alias.clone(),
                from: import.from.clone(),
                targets,
            });
        }
        if let Some(visible) = self.visible_objects.get_mut(relative) {
            visible.extend(imported_visible);
        }

        self.import_graph
            .insert(relative.to_path_buf(), import_edges);
        self.documents.insert(
            relative.to_path_buf(),
            SourceDocument {
                relative_path: relative.to_path_buf(),
                imports,
                exported_objects,
                kind: SourceDocumentKind::Dawn {
                    value,
                    object_types,
                },
            },
        );
        Ok(())
    }

    fn resolve_import(
        &self,
        importer: &Utf8Path,
        import_from: &Utf8Path,
    ) -> Result<Vec<Utf8PathBuf>, LoadProjectError> {
        let importer_dir = importer.parent().unwrap_or_else(|| Utf8Path::new(""));
        let target_relative = normalize_relative(importer_dir.join(import_from));
        let absolute = self.source_root.join(&target_relative);
        if absolute.is_dir() {
            let mut children = Vec::new();
            for entry in fs::read_dir(&absolute).map_err(|source| LoadProjectError::Io {
                path: absolute.clone(),
                source,
            })? {
                let entry = entry.map_err(|source| LoadProjectError::Io {
                    path: absolute.clone(),
                    source,
                })?;
                let path = Utf8PathBuf::from_path_buf(entry.path()).map_err(|path| {
                    LoadProjectError::InvalidDocument {
                        path: absolute.clone(),
                        message: format!("non-utf8 import child path `{}`", path.display()),
                    }
                })?;
                if path.is_file() && path.file_name().is_some_and(|name| name.ends_with(".dawn")) {
                    children.push(relative_path(&self.source_root, &path)?);
                }
            }
            children.sort();
            return Ok(children);
        }
        if absolute.is_file() {
            return Ok(vec![target_relative]);
        }
        Err(LoadProjectError::InvalidDocument {
            path: importer.to_path_buf(),
            message: format!("import target does not exist: {import_from}"),
        })
    }

    fn resolve_project(&mut self, entrypoint: &Utf8Path) -> Result<DawnProject, LoadProjectError> {
        let root_object = self.single_project_object(entrypoint)?;
        let root_id = ProjectId(root_object.key.clone());
        let display = self.reference_as_display(
            entrypoint,
            string_field(entrypoint, root_object.value, "display")?,
        )?;
        let sequences = sequence_field(entrypoint, root_object.value, "sequences")?
            .iter()
            .map(|reference| self.reference_as_sequence(entrypoint, reference))
            .collect::<Result<Vec<_>, _>>()?;

        let mut project = DawnProject {
            root: ProjectRoot {
                id: root_id,
                display: display.clone(),
                sequences: sequences.clone(),
            },
            displays: IndexMap::new(),
            layouts: IndexMap::new(),
            patches: IndexMap::new(),
            controllers: IndexMap::new(),
            sequences: IndexMap::new(),
            definitions: ProjectDefinitionStores::default(),
        };

        let mut resolver = DomainResolver {
            loader: self,
            project: &mut project,
        };
        resolver.resolve_display(entrypoint, &display)?;
        for sequence in sequences {
            resolver.resolve_sequence(entrypoint, &sequence)?;
        }
        Ok(project)
    }

    fn normalize_source_root(&mut self) -> Result<(), LoadProjectError> {
        let mut paths = self
            .documents
            .keys()
            .map(|path| self.source_root.join(path))
            .collect::<Vec<_>>();
        paths.extend(
            self.referenced_assets
                .iter()
                .map(|asset| asset.absolute_path.clone()),
        );
        let Some(common_root) = common_parent(&paths) else {
            return Ok(());
        };
        if common_root == self.source_root {
            return Ok(());
        }

        let old_root = self.source_root.clone();
        self.source_root = common_root;
        self.entrypoint = relative_path(&self.source_root, &old_root.join(&self.entrypoint))?;

        self.documents = self
            .documents
            .drain(..)
            .map(|(path, mut document)| {
                let next_path = relative_path(&self.source_root, &old_root.join(&path))?;
                document.relative_path = next_path.clone();
                Ok((next_path, document))
            })
            .collect::<Result<IndexMap<_, _>, LoadProjectError>>()?;

        self.import_graph = self
            .import_graph
            .drain(..)
            .map(|(path, edges)| {
                let next_path = relative_path(&self.source_root, &old_root.join(&path))?;
                let edges = edges
                    .into_iter()
                    .map(|mut edge| {
                        edge.targets = edge
                            .targets
                            .into_iter()
                            .map(|target| relative_path(&self.source_root, &old_root.join(target)))
                            .collect::<Result<Vec<_>, _>>()?;
                        Ok(edge)
                    })
                    .collect::<Result<Vec<_>, LoadProjectError>>()?;
                Ok((next_path, edges))
            })
            .collect::<Result<IndexMap<_, _>, LoadProjectError>>()?;

        for location in self.source_map.objects.values_mut() {
            location.document =
                relative_path(&self.source_root, &old_root.join(&location.document))?;
        }
        for asset in &mut self.referenced_assets {
            asset.relative_path = relative_path(&self.source_root, &asset.absolute_path)?;
        }
        Ok(())
    }

    fn single_project_object<'a>(
        &'a self,
        path: &Utf8Path,
    ) -> Result<SourceObjectValue<'a>, LoadProjectError> {
        let document = self.dawn_document(path)?;
        let mut found = None;
        let document_map = mapping(document).ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            message: "document root must be a mapping".to_string(),
        })?;
        for (key, value) in document_map.iter() {
            let Some(key) = key.as_str() else {
                continue;
            };
            if key == "imports" {
                continue;
            }
            if string_field(path, value, "type")? == "project" {
                found = Some(SourceObjectValue {
                    key: key.to_string(),
                    value,
                });
                break;
            }
        }
        found.ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            message: "entrypoint must contain a project object".to_string(),
        })
    }

    fn object_value(
        &self,
        id: &ResolvedObject,
    ) -> Result<(Utf8PathBuf, String, Value), LoadProjectError> {
        let location = self
            .source_map
            .objects
            .get(&SourceObjectId {
                kind: id.source_kind(),
                id: id.id_string(),
            })
            .ok_or_else(|| LoadProjectError::InvalidReference {
                path: self.entrypoint.clone(),
                reference: id.id_string(),
            })?;
        let document = self.dawn_document(&location.document)?;
        let document_map = mapping(document).ok_or_else(|| LoadProjectError::InvalidDocument {
            path: location.document.clone(),
            message: "document root must be a mapping".to_string(),
        })?;
        let value = document_map
            .get(Value::String(location.object_key.clone()))
            .ok_or_else(|| LoadProjectError::InvalidReference {
                path: location.document.clone(),
                reference: location.object_key.clone(),
            })?
            .clone();
        Ok((
            location.document.clone(),
            location.object_key.clone(),
            value,
        ))
    }

    fn dawn_document<'a>(&'a self, path: &Utf8Path) -> Result<&'a Value, LoadProjectError> {
        let document =
            self.documents
                .get(path)
                .ok_or_else(|| LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    message: "document not loaded".to_string(),
                })?;
        match &document.kind {
            SourceDocumentKind::Dawn { value, .. } => Ok(value),
            SourceDocumentKind::Effect { .. } => Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                message: "expected YAML Dawn document".to_string(),
            }),
        }
    }

    fn resolve_reference(
        &self,
        path: &Utf8Path,
        reference: &str,
    ) -> Result<ResolvedObject, LoadProjectError> {
        let (alias, object) =
            reference
                .split_once('.')
                .ok_or_else(|| LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    reference: reference.to_string(),
                })?;
        self.visible_objects
            .get(path)
            .and_then(|visible| {
                visible.get(&AliasObjectKey {
                    alias: Some(alias.to_string()),
                    object: object.to_string(),
                })
            })
            .cloned()
            .ok_or_else(|| LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                reference: reference.to_string(),
            })
    }

    fn reference_as_display(
        &self,
        path: &Utf8Path,
        reference: &str,
    ) -> Result<DisplayId, LoadProjectError> {
        match self.resolve_reference(path, reference)? {
            ResolvedObject::Display(id) => Ok(id),
            _ => Err(LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                reference: reference.to_string(),
            }),
        }
    }

    fn reference_as_sequence(
        &self,
        path: &Utf8Path,
        reference: &str,
    ) -> Result<SequenceId, LoadProjectError> {
        match self.resolve_reference(path, reference)? {
            ResolvedObject::Sequence(id) => Ok(id),
            _ => Err(LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                reference: reference.to_string(),
            }),
        }
    }
}

struct DomainResolver<'a> {
    loader: &'a mut Loader,
    project: &'a mut DawnProject,
}

impl DomainResolver<'_> {
    fn resolve_display(&mut self, path: &Utf8Path, id: &DisplayId) -> Result<(), LoadProjectError> {
        if self.project.displays.contains_key(id) {
            return Ok(());
        }
        let (document_path, _, value) = self
            .loader
            .object_value(&ResolvedObject::Display(id.clone()))?;
        let layout_ref = string_field(&document_path, &value, "layout")?;
        let patch_ref = string_field(&document_path, &value, "patch")?;
        let layout = match self.loader.resolve_reference(&document_path, layout_ref)? {
            ResolvedObject::Layout(layout) => layout,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: document_path,
                    reference: layout_ref.to_string(),
                })
            }
        };
        let patch = match self.loader.resolve_reference(&document_path, patch_ref)? {
            ResolvedObject::Patch(patch) => patch,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    reference: patch_ref.to_string(),
                })
            }
        };
        let controllers = sequence_field(&document_path, &value, "controllers")?
            .iter()
            .map(|name| ControllerId(name.to_string()))
            .collect::<Vec<_>>();
        self.project.displays.insert(
            id.clone(),
            Display {
                id: id.clone(),
                layout: layout.clone(),
                patch: patch.clone(),
                controllers: controllers.clone(),
            },
        );
        self.resolve_layout(&document_path, &layout)?;
        self.resolve_patch(&document_path, &patch)?;
        for controller in controllers {
            self.resolve_controller(&document_path, &controller)?;
        }
        Ok(())
    }

    fn resolve_controller(
        &mut self,
        path: &Utf8Path,
        id: &ControllerId,
    ) -> Result<(), LoadProjectError> {
        if self.project.controllers.contains_key(id) {
            return Ok(());
        }
        let (_, _, value) = self
            .loader
            .object_value(&ResolvedObject::Controller(id.clone()))?;
        let protocol = match string_field(path, &value, "protocol")? {
            "sacn" => Protocol::E131,
            "artnet" => Protocol::Artnet,
            other => {
                return Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    message: format!("unsupported controller protocol `{other}`"),
                })
            }
        };
        let address = optional_string_field(&value, "destination")
            .map(parse_controller_address)
            .transpose()
            .map_err(|message| LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                message,
            })?;
        let output = required_mapping(path, &value, "output")?;
        let output_value = Value::Mapping(output);
        let output_type = string_field(path, &output_value, "type")?;
        let channel_order =
            parse_channel_order(string_field(path, &output_value, "channel_order")?).ok_or_else(
                || LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    message: "invalid channel order".to_string(),
                },
            )?;
        let outputs = match output_type {
            "linear_rgb" => {
                let output_count = u32_field(path, &output_value, "output_count")?;
                let pixels = usize_field(path, &output_value, "pixels_per_output")?;
                let first_universe = u32_field(path, &output_value, "first_universe")?;
                (0..output_count)
                    .map(|index| ControllerOutput {
                        channel_order: channel_order.clone(),
                        pixels,
                        first_universe: first_universe + index,
                    })
                    .collect()
            }
            "patched_dmx" => sequence_values(path, &output_value, "universes")?
                .iter()
                .map(|universe| {
                    let range = string_field(path, universe, "range")?;
                    let slots = parse_slot_range(range).ok_or_else(|| {
                        LoadProjectError::InvalidDocument {
                            path: path.to_path_buf(),
                            message: format!("invalid universe range `{range}`"),
                        }
                    })?;
                    Ok(ControllerOutput {
                        channel_order: channel_order.clone(),
                        pixels: slots / 3,
                        first_universe: u32_field(path, universe, "id")?,
                    })
                })
                .collect::<Result<Vec<_>, LoadProjectError>>()?,
            other => {
                return Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    message: format!("unsupported controller output type `{other}`"),
                })
            }
        };
        let definition = ControllerDefinition {
            protocol,
            address,
            outputs,
        };
        self.project
            .definitions
            .controllers
            .insert(ControllerDefinitionId(id.0.clone()), definition.clone());
        self.project.controllers.insert(id.clone(), definition);
        Ok(())
    }

    fn resolve_layout(&mut self, path: &Utf8Path, id: &LayoutId) -> Result<(), LoadProjectError> {
        if self.project.layouts.contains_key(id) {
            return Ok(());
        }
        let (document_path, _, value) = self
            .loader
            .object_value(&ResolvedObject::Layout(id.clone()))?;
        let target_order = optional_sequence(&value, "target_order")
            .unwrap_or_default()
            .iter()
            .map(|target| parse_layout_target(&document_path, target))
            .collect::<Result<Vec<_>, _>>()?;
        let fixtures = optional_sequence(&value, "fixtures")
            .unwrap_or_default()
            .iter()
            .map(|fixture| self.parse_fixture_inst(&document_path, fixture))
            .collect::<Result<Vec<_>, _>>()?;
        let groups = optional_sequence(&value, "groups")
            .unwrap_or_default()
            .iter()
            .map(|group| parse_fixture_group(&document_path, group))
            .collect::<Result<Vec<_>, _>>()?;
        for fixture in &fixtures {
            self.resolve_fixture_definition(&document_path, &fixture.definition)?;
        }
        self.project.layouts.insert(
            id.clone(),
            Layout {
                id: id.clone(),
                target_order,
                fixtures,
                groups,
            },
        );
        let _ = path;
        Ok(())
    }

    fn parse_fixture_inst(
        &self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<FixtureInst, LoadProjectError> {
        let id = FixtureInstanceId(u32_field(path, value, "id")?);
        let name = string_field(path, value, "name")?.to_string();
        let fixture_ref = string_field(path, value, "fixture")?;
        let definition = match self.loader.resolve_reference(path, fixture_ref)? {
            ResolvedObject::FixtureDefinition(definition) => definition,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    reference: fixture_ref.to_string(),
                })
            }
        };
        let transform = optional_mapping_ref(value, "transform");
        let position = transform
            .and_then(|mapping| mapping.get(Value::String("position".to_string())))
            .map(parse_point3)
            .transpose()?
            .unwrap_or_default();
        let rotation = transform
            .and_then(|mapping| mapping.get(Value::String("rotation".to_string())))
            .map(parse_rotation3)
            .transpose()?
            .unwrap_or_default();
        let scale = transform
            .and_then(|mapping| mapping.get(Value::String("scale".to_string())))
            .map(parse_scale3)
            .transpose()?
            .unwrap_or_default();
        Ok(FixtureInst {
            id,
            name,
            definition,
            position,
            rotation,
            scale,
        })
    }

    fn resolve_fixture_definition(
        &mut self,
        path: &Utf8Path,
        id: &FixtureDefinitionId,
    ) -> Result<(), LoadProjectError> {
        if self
            .project
            .definitions
            .fixtures
            .definitions
            .contains_key(id)
        {
            return Ok(());
        }
        let (_, _, value) = self
            .loader
            .object_value(&ResolvedObject::FixtureDefinition(id.clone()))?;
        let bulb_diameter = f64_field(path, &value, "bulb_diameter")?;
        let geometry_value = required_field(path, &value, "geometry")?;
        let geometry_type = string_field(path, geometry_value, "type")?;
        let geometry = match geometry_type {
            "points" => Geometry::Points {
                points: sequence_values(path, geometry_value, "points")?
                    .iter()
                    .map(parse_point3)
                    .collect::<Result<Vec<_>, _>>()?,
            },
            "lines" => Geometry::Lines {
                points: sequence_values(path, geometry_value, "points")?
                    .iter()
                    .map(parse_point3)
                    .collect::<Result<Vec<_>, _>>()?,
                pixels: u32_field(path, geometry_value, "pixels")?,
            },
            "arc" => Geometry::Arc {
                center: parse_point3(required_field(path, geometry_value, "center")?)?,
                radius: distance_span(f64_field(path, geometry_value, "radius")?),
                start_degrees: f64_field(path, geometry_value, "startDegrees")?,
                end_degrees: f64_field(path, geometry_value, "endDegrees")?,
                pixels: u32_field(path, geometry_value, "pixels")?,
            },
            other => {
                return Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    message: format!("unsupported fixture geometry `{other}`"),
                })
            }
        };
        self.project.definitions.fixtures.insert(
            id.clone(),
            FixtureDefinition {
                bulb_radius: distance_span(bulb_diameter / 2.0),
                geometry,
            },
        );
        Ok(())
    }

    fn resolve_patch(&mut self, _path: &Utf8Path, id: &PatchId) -> Result<(), LoadProjectError> {
        if self.project.patches.contains_key(id) {
            return Ok(());
        }
        let (document_path, _, value) = self
            .loader
            .object_value(&ResolvedObject::Patch(id.clone()))?;
        let routes = optional_sequence(&value, "routes")
            .unwrap_or_default()
            .iter()
            .map(|route| self.parse_patch_route(&document_path, route))
            .collect::<Result<Vec<_>, _>>()?;
        self.project.patches.insert(
            id.clone(),
            Patch {
                id: id.clone(),
                routes,
            },
        );
        Ok(())
    }

    fn parse_patch_route(
        &self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<PatchRoute, LoadProjectError> {
        let controller_ref = string_field(path, value, "controller")?;
        let controller = match self.loader.resolve_reference(path, controller_ref) {
            Ok(ResolvedObject::Controller(controller)) => controller,
            Ok(_) => {
                return Err(LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    reference: controller_ref.to_string(),
                })
            }
            Err(_) => ControllerId(controller_ref.to_string()),
        };
        let output = optional_field(value, "output")
            .and_then(Value::as_u64)
            .map(|value| value as u32)
            .or_else(|| {
                optional_field(value, "universe")
                    .and_then(Value::as_u64)
                    .map(|value| value.saturating_sub(1) as u32)
            })
            .ok_or_else(|| LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                message: "patch route must contain output or universe".to_string(),
            })?;
        let start_channel_offset = optional_field(value, "start_channel_offset")
            .and_then(Value::as_u64)
            .map(|value| value as u32)
            .or_else(|| {
                optional_field(value, "start")
                    .and_then(Value::as_u64)
                    .map(|value| value.saturating_sub(1) as u32)
            })
            .ok_or_else(|| LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                message: "patch route must contain start_channel_offset or start".to_string(),
            })?;
        Ok(PatchRoute {
            fixture: FixtureInstanceId(u32_field(path, value, "fixture")?),
            fixture_pixels: PixelRange {
                start: optional_field(value, "fixture_pixel_start")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
                count: optional_field(value, "fixture_pixel_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
            },
            controller,
            output: ControllerOutputIndex(output),
            start_channel_offset,
        })
    }

    fn resolve_sequence(
        &mut self,
        path: &Utf8Path,
        id: &SequenceId,
    ) -> Result<(), LoadProjectError> {
        if self.project.sequences.contains_key(id) {
            return Ok(());
        }
        let (document_path, _, value) = self
            .loader
            .object_value(&ResolvedObject::Sequence(id.clone()))?;
        let audio = self.parse_audio(&document_path, &value)?;
        let mark_collections = optional_sequence(&value, "mark_collections")
            .unwrap_or_default()
            .iter()
            .map(|collection| parse_mark_collection(&document_path, collection))
            .collect::<Result<Vec<_>, _>>()?;
        let effects = optional_sequence(&value, "effects")
            .unwrap_or_default()
            .iter()
            .map(|effect| self.parse_effect_inst(&document_path, effect))
            .collect::<Result<Vec<_>, _>>()?;
        let automation_clips = optional_sequence(&value, "automation_clips")
            .unwrap_or_default()
            .iter()
            .map(|clip| self.parse_automation_clip(&document_path, clip))
            .collect::<Result<Vec<_>, _>>()?;
        self.project.sequences.insert(
            id.clone(),
            Sequence {
                id: id.clone(),
                duration: parse_duration(string_field(&document_path, &value, "duration")?)?,
                frame_rate: u32_field(&document_path, &value, "frame_rate")?,
                audio,
                mark_collections,
                effects,
                automation_clips,
            },
        );
        let _ = path;
        Ok(())
    }

    fn parse_audio(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<SequenceAudio, LoadProjectError> {
        let Some(audio) = optional_field(value, "audio") else {
            return Ok(SequenceAudio::None);
        };
        if matches!(audio, Value::Null) {
            return Ok(SequenceAudio::None);
        }
        let Some(audio_path) = audio.as_str() else {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                message: "audio must be null or a path string".to_string(),
            });
        };
        let document_absolute = self.loader.source_root.join(path);
        let document_dir = document_absolute
            .parent()
            .unwrap_or(&self.loader.source_root);
        let absolute = document_dir
            .join(audio_path)
            .canonicalize_utf8()
            .map_err(|source| LoadProjectError::Io {
                path: document_dir.join(audio_path),
                source,
            })?;
        if !absolute.is_file() {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                message: format!("audio asset does not exist: {audio_path}"),
            });
        }
        let relative = relative_path(&self.loader.source_root, &absolute)
            .unwrap_or_else(|_| Utf8PathBuf::from(audio_path));
        if let Some(existing) = self
            .loader
            .referenced_assets
            .iter()
            .find(|asset| asset.relative_path == relative)
        {
            return Ok(SequenceAudio::Asset(existing.id.clone()));
        }
        let id = AssetId(self.loader.next_asset_id);
        self.loader.next_asset_id += 1;
        self.loader.referenced_assets.push(ReferencedAsset {
            id: id.clone(),
            relative_path: relative,
            absolute_path: absolute,
        });
        Ok(SequenceAudio::Asset(id))
    }

    fn parse_effect_inst(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<EffectInst, LoadProjectError> {
        let script_ref = string_field(path, value, "script")?;
        let definition = match self.loader.resolve_reference(path, script_ref)? {
            ResolvedObject::EffectDefinition(definition) => definition,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    reference: script_ref.to_string(),
                })
            }
        };
        self.resolve_effect_definition(&definition)?;
        let params = optional_mapping(value, "params")
            .map(|mapping| {
                mapping
                    .iter()
                    .map(|(key, value)| {
                        let key =
                            key.as_str()
                                .ok_or_else(|| LoadProjectError::InvalidDocument {
                                    path: path.to_path_buf(),
                                    message: "effect param keys must be strings".to_string(),
                                })?;
                        let identifier = Identifier::new(key.to_string()).map_err(|_| {
                            LoadProjectError::InvalidDocument {
                                path: path.to_path_buf(),
                                message: format!("invalid effect param name `{key}`"),
                            }
                        })?;
                        Ok((identifier, self.parse_effect_param(path, value)?))
                    })
                    .collect::<Result<IndexMap<_, _>, LoadProjectError>>()
            })
            .transpose()?
            .unwrap_or_default();
        Ok(EffectInst {
            id: EffectInstId(u32_field(path, value, "id")?),
            start: parse_duration_as_time(string_field(path, value, "start")?)?,
            duration: parse_duration(string_field(path, value, "duration")?)?,
            target: parse_effect_target(path, required_field(path, value, "target")?)?,
            scope: match string_field(path, value, "scope")? {
                "per_fixture" => EffectScope::PerFixture,
                "whole_target" => EffectScope::WholeTarget,
                other => {
                    return Err(LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        message: format!("invalid effect scope `{other}`"),
                    })
                }
            },
            definition,
            param_overrides: params,
        })
    }

    fn resolve_effect_definition(
        &mut self,
        id: &EffectDefinitionId,
    ) -> Result<(), LoadProjectError> {
        if self
            .project
            .definitions
            .effects
            .definitions
            .contains_key(id)
        {
            return Ok(());
        }
        let source = self.loader.effect_source_text.get(id).ok_or_else(|| {
            LoadProjectError::InvalidReference {
                path: self.loader.entrypoint.clone(),
                reference: id.0.clone(),
            }
        })?;
        let compiled =
            compile_effects(source).map_err(|diagnostics| LoadProjectError::InvalidEffect {
                path: self.loader.entrypoint.clone(),
                diagnostics: diagnostics
                    .into_iter()
                    .map(|diagnostic| {
                        format!(
                            "{}..{} {}",
                            diagnostic.span.start, diagnostic.span.end, diagnostic.message
                        )
                    })
                    .collect(),
            })?;
        let compiled = compiled
            .into_iter()
            .find(|effect| effect.name().as_str() == id.0)
            .ok_or_else(|| LoadProjectError::InvalidReference {
                path: self.loader.entrypoint.clone(),
                reference: id.0.clone(),
            })?;
        self.project
            .definitions
            .effects
            .insert(id.clone(), EffectDefinition { compiled });
        Ok(())
    }

    fn parse_effect_param(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<EffectParamValue, LoadProjectError> {
        match string_field(path, value, "type")? {
            "integer" => Ok(EffectParamValue::Int(i64_field(path, value, "value")?)),
            "float" => Ok(EffectParamValue::Float(f64_field(path, value, "value")?)),
            "bool" => Ok(EffectParamValue::Bool(bool_field(path, value, "value")?)),
            "color" => Ok(EffectParamValue::Color(parse_color(string_field(
                path, value, "value",
            )?)?)),
            "enum" => Ok(EffectParamValue::Enum(
                Identifier::new(string_field(path, value, "value")?.to_string()).map_err(|_| {
                    LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        message: "invalid enum value".to_string(),
                    }
                })?,
            )),
            "marks" => Ok(EffectParamValue::Marks(MarkCollectionKey {
                name: string_field(path, value, "key")?.to_string(),
            })),
            "curve" => Ok(EffectParamValue::Curve(
                self.parse_curve_source(path, required_field(path, value, "curve")?)?,
            )),
            "array" => {
                let values = sequence_values(path, value, "values")?
                    .iter()
                    .map(|item| self.parse_array_item(path, item))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(EffectParamValue::Array(values))
            }
            other => Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                message: format!("unsupported effect param type `{other}`"),
            }),
        }
    }

    fn parse_array_item(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<EffectParamValue, LoadProjectError> {
        let curve = required_field(path, value, "curve")?;
        Ok(EffectParamValue::Curve(
            self.parse_curve_source(path, curve)?,
        ))
    }

    fn parse_curve_source(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<CurveSource, LoadProjectError> {
        if let Some(reference) = value.as_str() {
            let id = match self.loader.resolve_reference(path, reference)? {
                ResolvedObject::Curve(curve) => curve,
                _ => {
                    return Err(LoadProjectError::InvalidReference {
                        path: path.to_path_buf(),
                        reference: reference.to_string(),
                    })
                }
            };
            self.resolve_curve(path, &id)?;
            return Ok(CurveSource::Reference(id));
        }
        if let Some(curve_value) = optional_field(value, "curve") {
            return self.parse_curve_source(path, curve_value);
        }
        Ok(CurveSource::Inline(parse_curve(path, value)?))
    }

    fn resolve_curve(&mut self, path: &Utf8Path, id: &CurveId) -> Result<(), LoadProjectError> {
        if self.project.definitions.curves.definitions.contains_key(id) {
            return Ok(());
        }
        let (_, _, value) = self
            .loader
            .object_value(&ResolvedObject::Curve(id.clone()))?;
        self.project.definitions.curves.insert(
            id.clone(),
            CurveDefinition {
                curve: parse_curve(path, &value)?,
            },
        );
        Ok(())
    }

    fn parse_automation_clip(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<AutomationClip, LoadProjectError> {
        let curve_ref = string_field(path, value, "curve")?;
        let curve = match self.loader.resolve_reference(path, curve_ref)? {
            ResolvedObject::Curve(curve) => curve,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    reference: curve_ref.to_string(),
                })
            }
        };
        self.resolve_curve(path, &curve)?;
        Ok(AutomationClip {
            id: AutomationClipId(u32_field(path, value, "id")?),
            targets: sequence_values(path, value, "targets")?
                .iter()
                .map(|target| {
                    target
                        .as_u64()
                        .map(|value| EffectInstId(value as u32))
                        .ok_or_else(|| LoadProjectError::InvalidDocument {
                            path: path.to_path_buf(),
                            message: "automation targets must be effect ids".to_string(),
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
            start: parse_duration_as_time(string_field(path, value, "start")?)?,
            duration: parse_duration(string_field(path, value, "duration")?)?,
            curve,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct AliasObjectKey {
    alias: Option<String>,
    object: String,
}

#[derive(Clone, Debug, PartialEq)]
enum ResolvedObject {
    Project(ProjectId),
    Display(DisplayId),
    Controller(ControllerId),
    Layout(LayoutId),
    Patch(PatchId),
    FixtureDefinition(FixtureDefinitionId),
    Curve(CurveId),
    Sequence(SequenceId),
    EffectDefinition(EffectDefinitionId),
}

impl ResolvedObject {
    fn source_kind(&self) -> SourceObjectKind {
        match self {
            Self::Project(_) => SourceObjectKind::Project,
            Self::Display(_) => SourceObjectKind::Display,
            Self::Controller(_) => SourceObjectKind::Controller,
            Self::Layout(_) => SourceObjectKind::Layout,
            Self::Patch(_) => SourceObjectKind::Patch,
            Self::FixtureDefinition(_) => SourceObjectKind::FixtureDefinition,
            Self::Curve(_) => SourceObjectKind::Curve,
            Self::Sequence(_) => SourceObjectKind::Sequence,
            Self::EffectDefinition(_) => SourceObjectKind::EffectDefinition,
        }
    }

    fn id_string(&self) -> String {
        match self {
            Self::Project(id) => id.0.clone(),
            Self::Display(id) => id.0.clone(),
            Self::Controller(id) => id.0.clone(),
            Self::Layout(id) => id.0.clone(),
            Self::Patch(id) => id.0.clone(),
            Self::FixtureDefinition(id) => id.0.clone(),
            Self::Curve(id) => id.0.clone(),
            Self::Sequence(id) => id.0.clone(),
            Self::EffectDefinition(id) => id.0.clone(),
        }
    }
}

struct SourceObjectValue<'a> {
    key: String,
    value: &'a Value,
}

fn parse_imports(path: &Utf8Path, map: &Mapping) -> Result<Vec<ImportDecl>, LoadProjectError> {
    let Some(imports) = map.get(Value::String("imports".to_string())) else {
        return Ok(Vec::new());
    };
    let imports = imports
        .as_sequence()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            message: "imports must be a sequence".to_string(),
        })?;
    imports
        .iter()
        .map(|import| {
            Ok(ImportDecl {
                from: Utf8PathBuf::from(string_field(path, import, "from")?),
                alias: string_field(path, import, "as")?.to_string(),
            })
        })
        .collect()
}

fn parse_layout_target(path: &Utf8Path, value: &Value) -> Result<LayoutTarget, LoadProjectError> {
    match string_field(path, value, "type")? {
        "fixture" => Ok(LayoutTarget::Fixture(FixtureInstanceId(u32_field(
            path, value, "id",
        )?))),
        "group" => Ok(LayoutTarget::Group(FixtureGroupId(u32_field(
            path, value, "id",
        )?))),
        other => Err(LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            message: format!("invalid layout target type `{other}`"),
        }),
    }
}

fn parse_fixture_group(path: &Utf8Path, value: &Value) -> Result<FixtureGroup, LoadProjectError> {
    Ok(FixtureGroup {
        id: FixtureGroupId(u32_field(path, value, "id")?),
        name: string_field(path, value, "name")?.to_string(),
        fixtures: sequence_values(path, value, "members")?
            .iter()
            .map(|member| {
                member
                    .as_u64()
                    .map(|value| FixtureInstanceId(value as u32))
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        message: "group members must be fixture ids".to_string(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn parse_mark_collection(
    path: &Utf8Path,
    value: &Value,
) -> Result<MarkCollection, LoadProjectError> {
    Ok(MarkCollection {
        key: MarkCollectionKey {
            name: string_field(path, value, "key")?.to_string(),
        },
        name: string_field(path, value, "name")?.to_string(),
        display_color: parse_color(string_field(path, value, "color")?)?,
        marks: sequence_values(path, value, "marks")?
            .iter()
            .map(|mark| {
                mark.as_str()
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        message: "marks must be duration strings".to_string(),
                    })
                    .and_then(parse_duration_as_time)
            })
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn parse_effect_target(path: &Utf8Path, value: &Value) -> Result<EffectTarget, LoadProjectError> {
    match string_field(path, value, "type")? {
        "fixture" => Ok(EffectTarget::Fixture(FixtureInstanceId(u32_field(
            path, value, "id",
        )?))),
        "group" => Ok(EffectTarget::Group(FixtureGroupId(u32_field(
            path, value, "id",
        )?))),
        other => Err(LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            message: format!("invalid effect target type `{other}`"),
        }),
    }
}

fn parse_curve(path: &Utf8Path, value: &Value) -> Result<Curve, LoadProjectError> {
    let value_type = string_field(path, value, "value_type")?;
    let points = sequence_values(path, value, "points")?
        .iter()
        .map(|point| {
            let position = f64_field(path, point, "time")?;
            let value = required_field(path, point, "value")?;
            let value = match value_type {
                "float" => CurveValue::Float(value.as_f64().ok_or_else(|| {
                    LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        message: "curve float point must be numeric".to_string(),
                    }
                })?),
                "color" => CurveValue::Color(parse_color(value.as_str().ok_or_else(|| {
                    LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        message: "curve color point must be a color string".to_string(),
                    }
                })?)?),
                other => {
                    return Err(LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        message: format!("unsupported curve value type `{other}`"),
                    })
                }
            };
            Ok(CurvePoint { position, value })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Curve { points })
}

fn parse_point3(value: &Value) -> Result<Point3, LoadProjectError> {
    Ok(Point3 {
        x: distance(f64_field(Utf8Path::new("<inline>"), value, "x")?),
        y: distance(f64_field(Utf8Path::new("<inline>"), value, "y")?),
        z: distance(f64_field(Utf8Path::new("<inline>"), value, "z")?),
    })
}

fn parse_rotation3(value: &Value) -> Result<Rotation3, LoadProjectError> {
    Ok(Rotation3 {
        x: f64_field(Utf8Path::new("<inline>"), value, "x")?,
        y: f64_field(Utf8Path::new("<inline>"), value, "y")?,
        z: f64_field(Utf8Path::new("<inline>"), value, "z")?,
    })
}

fn parse_scale3(value: &Value) -> Result<Scale3, LoadProjectError> {
    Ok(Scale3 {
        x: f64_field(Utf8Path::new("<inline>"), value, "x")?,
        y: f64_field(Utf8Path::new("<inline>"), value, "y")?,
        z: f64_field(Utf8Path::new("<inline>"), value, "z")?,
    })
}

fn parse_controller_address(value: &str) -> Result<ControllerAddress, String> {
    let (ip, port) = value
        .split_once(':')
        .ok_or_else(|| "controller destination must be ip:port".to_string())?;
    let ip = ip
        .parse()
        .map_err(|_| "controller destination ip is invalid".to_string())?;
    let port = port
        .parse()
        .map_err(|_| "controller destination port is invalid".to_string())?;
    Ok(ControllerAddress { ip, port })
}

fn parse_channel_order(value: &str) -> Option<RgbChannelOrder> {
    match value {
        "rgb" => Some(RgbChannelOrder::Rgb),
        "rbg" => Some(RgbChannelOrder::Rbg),
        "grb" => Some(RgbChannelOrder::Grb),
        "gbr" => Some(RgbChannelOrder::Gbr),
        "brg" => Some(RgbChannelOrder::Brg),
        "bgr" => Some(RgbChannelOrder::Bgr),
        _ => None,
    }
}

fn parse_slot_range(value: &str) -> Option<usize> {
    let (start, end) = value.split_once("..")?;
    let start = start.parse::<usize>().ok()?;
    let end = end.parse::<usize>().ok()?;
    end.checked_sub(start).map(|slots| slots + 1)
}

fn parse_duration(value: &str) -> Result<DawnDuration, LoadProjectError> {
    Ok(DawnDuration(Duration::from_secs_f64(parse_seconds(value)?)))
}

fn parse_duration_as_time(value: &str) -> Result<DawnTime, LoadProjectError> {
    Ok(DawnTime(Duration::from_secs_f64(parse_seconds(value)?)))
}

fn parse_seconds(value: &str) -> Result<f64, LoadProjectError> {
    let seconds = value
        .strip_suffix('s')
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: Utf8PathBuf::from("<duration>"),
            message: format!("duration must end in `s`: {value}"),
        })?;
    seconds
        .parse()
        .map_err(|_| LoadProjectError::InvalidDocument {
            path: Utf8PathBuf::from("<duration>"),
            message: format!("invalid duration: {value}"),
        })
}

fn parse_color(value: &str) -> Result<Color, LoadProjectError> {
    if value.len() != 7 || !value.starts_with('#') {
        return Err(LoadProjectError::InvalidDocument {
            path: Utf8PathBuf::from("<color>"),
            message: format!("invalid color: {value}"),
        });
    }
    Ok(Color {
        red: u8::from_str_radix(&value[1..3], 16).map_err(|_| {
            LoadProjectError::InvalidDocument {
                path: Utf8PathBuf::from("<color>"),
                message: format!("invalid color: {value}"),
            }
        })?,
        green: u8::from_str_radix(&value[3..5], 16).map_err(|_| {
            LoadProjectError::InvalidDocument {
                path: Utf8PathBuf::from("<color>"),
                message: format!("invalid color: {value}"),
            }
        })?,
        blue: u8::from_str_radix(&value[5..7], 16).map_err(|_| {
            LoadProjectError::InvalidDocument {
                path: Utf8PathBuf::from("<color>"),
                message: format!("invalid color: {value}"),
            }
        })?,
    })
}

fn distance(value: f64) -> Distance {
    Distance {
        micrometers: (value * 1_000_000.0).round() as i64,
    }
}

fn distance_span(value: f64) -> DistanceSpan {
    DistanceSpan {
        micrometers: (value * 1_000_000.0).round() as u64,
    }
}

fn mapping(value: &Value) -> Option<&Mapping> {
    match value {
        Value::Mapping(mapping) => Some(mapping),
        _ => None,
    }
}

fn required_field<'a>(
    path: &Utf8Path,
    value: &'a Value,
    key: &str,
) -> Result<&'a Value, LoadProjectError> {
    mapping(value)
        .and_then(|mapping| mapping.get(Value::String(key.to_string())))
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            message: format!("missing field `{key}`"),
        })
}

fn optional_field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    mapping(value).and_then(|mapping| mapping.get(Value::String(key.to_string())))
}

fn required_mapping(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<Mapping, LoadProjectError> {
    optional_mapping(value, key).ok_or_else(|| LoadProjectError::InvalidDocument {
        path: path.to_path_buf(),
        message: format!("missing mapping `{key}`"),
    })
}

fn optional_mapping(value: &Value, key: &str) -> Option<Mapping> {
    optional_field(value, key).and_then(|value| mapping(value).cloned())
}

fn optional_mapping_ref<'a>(value: &'a Value, key: &str) -> Option<&'a Mapping> {
    optional_field(value, key).and_then(mapping)
}

fn optional_sequence(value: &Value, key: &str) -> Option<Vec<Value>> {
    optional_field(value, key).and_then(|value| value.as_sequence().cloned())
}

fn sequence_values<'a>(
    path: &Utf8Path,
    value: &'a Value,
    key: &str,
) -> Result<&'a Vec<Value>, LoadProjectError> {
    required_field(path, value, key)?
        .as_sequence()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            message: format!("field `{key}` must be a sequence"),
        })
}

fn sequence_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<Vec<String>, LoadProjectError> {
    sequence_values(path, value, key)?
        .iter()
        .map(|value| {
            value.as_str().map(ToString::to_string).ok_or_else(|| {
                LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    message: format!("field `{key}` values must be strings"),
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()
}

fn string_field<'a>(
    path: &Utf8Path,
    value: &'a Value,
    key: &str,
) -> Result<&'a str, LoadProjectError> {
    required_field(path, value, key)?
        .as_str()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            message: format!("field `{key}` must be a string"),
        })
}

fn optional_string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    optional_field(value, key).and_then(Value::as_str)
}

fn u32_field(path: &Utf8Path, value: &Value, key: &str) -> Result<u32, LoadProjectError> {
    required_field(path, value, key)?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            message: format!("field `{key}` must be a u32"),
        })
}

fn usize_field(path: &Utf8Path, value: &Value, key: &str) -> Result<usize, LoadProjectError> {
    required_field(path, value, key)?
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            message: format!("field `{key}` must be a usize"),
        })
}

fn i64_field(path: &Utf8Path, value: &Value, key: &str) -> Result<i64, LoadProjectError> {
    required_field(path, value, key)?
        .as_i64()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            message: format!("field `{key}` must be an integer"),
        })
}

fn f64_field(path: &Utf8Path, value: &Value, key: &str) -> Result<f64, LoadProjectError> {
    required_field(path, value, key)?
        .as_f64()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            message: format!("field `{key}` must be a number"),
        })
}

fn bool_field(path: &Utf8Path, value: &Value, key: &str) -> Result<bool, LoadProjectError> {
    required_field(path, value, key)?
        .as_bool()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            message: format!("field `{key}` must be a bool"),
        })
}

fn relative_path(root: &Utf8Path, path: &Utf8Path) -> Result<Utf8PathBuf, LoadProjectError> {
    path.strip_prefix(root)
        .map(Utf8Path::to_path_buf)
        .map_err(|_| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            message: format!("path is outside source root {root}"),
        })
}

fn normalize_relative(path: Utf8PathBuf) -> Utf8PathBuf {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            camino::Utf8Component::CurDir => {}
            camino::Utf8Component::ParentDir => {
                let _ = parts.pop();
            }
            camino::Utf8Component::Normal(part) => parts.push(part.to_string()),
            camino::Utf8Component::RootDir | camino::Utf8Component::Prefix(_) => {}
        }
    }
    parts.into_iter().collect()
}

fn common_parent(paths: &[Utf8PathBuf]) -> Option<Utf8PathBuf> {
    let mut iter = paths.iter();
    let first = iter.next()?;
    let mut common = first.parent()?.to_path_buf();
    for path in iter {
        let parent = path.parent()?;
        while !parent.starts_with(&common) {
            common = common.parent()?.to_path_buf();
        }
    }
    Some(common)
}

pub fn source_file_list(session: &ProjectSession) -> BTreeMap<Utf8PathBuf, Vec<String>> {
    session
        .source
        .documents
        .iter()
        .map(|(path, document)| (path.clone(), document.exported_objects.clone()))
        .collect()
}
