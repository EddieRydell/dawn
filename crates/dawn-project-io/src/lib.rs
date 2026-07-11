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
use dawn_language::dsl::{
    Diagnostic as DslDiagnostic, Identifier, compile_effects, compile_operators,
};
use dawn_language::effect::{
    CurveDefinition, CurveId, CurveSource, EffectDefinition, EffectDefinitionId, EffectInst,
    EffectInstId, EffectParamValue, EffectScope, EffectTarget,
};
use dawn_language::identity::SourceIdentity;
use dawn_language::model::{DawnProject, ProjectDefinitionStores, ProjectId, ProjectRoot};
use dawn_language::operator::{
    BuiltinOperator, GraphOperatorNode, OperatorDefinitionId, OperatorRef,
    custom_operator_definition, validate_composition_graph,
};
use dawn_language::sequence::{
    AssetId, AutomationBinding, AutomationClip, AutomationClipId, AutomationMapping,
    AutomationTarget, CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind,
    EffectClip, EffectGraphEdge, GraphNodePosition, GraphPortId, MarkCollection, MarkCollectionKey,
    Sequence, SequenceAudio, SequenceCompositionGraph, SequenceId, SequenceLayer, SequenceLayerId,
};
use dawn_language::setup::{
    ControllerAddress, ControllerDefinition, ControllerDefinitionId, ControllerId,
    ControllerOutput, ControllerOutputIndex, FixtureDefinition, FixtureDefinitionId, FixtureGroup,
    FixtureGroupId, FixtureInst, FixtureInstanceId, Geometry, Layout, LayoutId, LayoutTarget,
    Patch, PatchId, PatchRoute, PixelRange, Protocol, RgbChannelOrder, Setup, SetupId,
};
use dawn_language::values::{
    Color, Curve, CurvePoint, CurveValue, DawnDuration, DawnTime, Distance, DistanceSpan, Point3,
    Rotation3, Scale3,
};
use indexmap::{IndexMap, IndexSet};
use marked_yaml::{LoadError as MarkedYamlError, Marker, Node};
use std::cell::RefCell;
use std::fmt;
use std::fs;
use std::io;
use std::time::Duration;
use yaml_serde::{Mapping, Value};

thread_local! {
    static YAML_SOURCE_INDICES: RefCell<IndexMap<Utf8PathBuf, YamlSourceIndex>> =
        RefCell::new(IndexMap::new());
}

mod source;
pub use source::{
    ExportReport, ImportEdge, ProjectSession, ReferencedAsset, SaveReport, SourceDocument,
    SourceDocumentKind, SourceObjectId, SourceObjectKind, SourceProject, is_project_owned_path,
    relative_path_from_document, source_file_list,
};

pub fn load_project(path: &Utf8Path) -> Result<ProjectSession, LoadProjectError> {
    Loader::new(path)?.load()
}

pub fn check_project(path: &Utf8Path) -> ProjectCheckReport {
    let mut diagnostics = Vec::new();
    let (source_root, mut reachable_files) = match discover_reachable_files(path) {
        Ok(discovered) => discovered,
        Err(error) => {
            push_diagnostic(&mut diagnostics, load_error_diagnostic(error));
            (Utf8PathBuf::new(), Vec::new())
        }
    };
    reachable_files.sort();
    reachable_files.dedup();

    for file in &reachable_files {
        diagnostics.extend(check_absolute_document(&source_root, file));
    }

    match load_project(path) {
        Ok(session) => ProjectCheckReport {
            session: Some(session),
            diagnostics,
        },
        Err(error) => {
            push_load_error_diagnostics(&mut diagnostics, error);
            ProjectCheckReport {
                session: None,
                diagnostics,
            }
        }
    }
}

pub fn check_document_text(path: &Utf8Path, text: &str) -> Vec<IoDiagnostic> {
    if path
        .file_name()
        .is_some_and(|file_name| file_name.ends_with(".effect.dawn"))
    {
        return effect_diagnostics(path, text);
    }
    if path
        .file_name()
        .is_some_and(|file_name| file_name.ends_with(".operator.dawn"))
    {
        return operator_diagnostics(path, text);
    }

    match parse_yaml_value(path, text) {
        Ok(_) => Vec::new(),
        Err(LoadProjectError::ParseYaml { message, range, .. }) => vec![IoDiagnostic {
            path: path.to_path_buf(),
            range,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::YamlParse,
            message,
        }],
        Err(error) => vec![load_error_diagnostic(error)],
    }
}

pub fn check_project_document_text(
    entrypoint: &Utf8Path,
    document: &Utf8Path,
    text: &str,
) -> Vec<IoDiagnostic> {
    let loader = match Loader::new(entrypoint) {
        Ok(mut loader) => {
            loader
                .source_overrides
                .insert(normalize_relative(document.to_path_buf()), text.to_string());
            loader
        }
        Err(error) => return vec![load_error_diagnostic(error)],
    };
    match loader.load() {
        Ok(_) => Vec::new(),
        Err(error) => {
            let mut diagnostics = Vec::new();
            push_load_error_diagnostics(&mut diagnostics, error);
            diagnostics
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectCheckReport {
    pub session: Option<ProjectSession>,
    pub diagnostics: Vec<IoDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct IoDiagnostic {
    pub path: Utf8PathBuf,
    pub range: Option<TextRange>,
    pub severity: IoDiagnosticSeverity,
    pub code: IoDiagnosticCode,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum IoDiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum IoDiagnosticCode {
    DawnLoad,
    DawnReference,
    EffectCompile,
    OperatorCompile,
    IoRead,
    YamlParse,
}

impl IoDiagnosticCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DawnLoad => "dawn.load",
            Self::DawnReference => "dawn.reference",
            Self::EffectCompile => "effect.compile",
            Self::OperatorCompile => "operator.compile",
            Self::IoRead => "io.read",
            Self::YamlParse => "yaml.parse",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
enum YamlPathSegment {
    Key(String),
    Index(usize),
}

#[derive(Clone, Debug, Default)]
struct YamlSourceIndex {
    entries: Vec<YamlSourceEntry>,
    value_bindings: IndexMap<usize, Vec<YamlPathSegment>>,
    claimed_value_paths: IndexSet<Vec<YamlPathSegment>>,
    scalar_bindings: IndexMap<usize, Vec<YamlPathSegment>>,
    claimed_scalar_paths: IndexSet<Vec<YamlPathSegment>>,
}

#[derive(Clone, Debug)]
struct YamlSourceEntry {
    path: Vec<YamlPathSegment>,
    value: Value,
    range: Option<TextRange>,
}

impl YamlSourceIndex {
    fn from_value_and_node(value: &Value, node: &Node) -> Self {
        let mut index = Self::default();
        let mut path = Vec::new();
        index.push(value, node, &mut path);
        index
    }

    fn push(&mut self, value: &Value, node: &Node, path: &mut Vec<YamlPathSegment>) {
        self.entries.push(YamlSourceEntry {
            path: path.clone(),
            value: value.clone(),
            range: node_range(node),
        });

        match (value, node) {
            (Value::Mapping(mapping), Node::Mapping(marked_mapping)) => {
                for (key, child_value) in mapping {
                    let Some(key) = key.as_str() else {
                        continue;
                    };
                    let Some(child_node) = marked_mapping.get_node(key) else {
                        continue;
                    };
                    path.push(YamlPathSegment::Key(key.to_string()));
                    self.push(child_value, child_node, path);
                    let _ = path.pop();
                }
            }
            (Value::Sequence(sequence), Node::Sequence(marked_sequence)) => {
                for (index, child_value) in sequence.iter().enumerate() {
                    let Some(child_node) = marked_sequence.get_node(index) else {
                        continue;
                    };
                    path.push(YamlPathSegment::Index(index));
                    self.push(child_value, child_node, path);
                    let _ = path.pop();
                }
            }
            _ => {}
        }
    }

    fn bound_value_path(&mut self, value: &Value) -> Option<Vec<YamlPathSegment>> {
        let pointer = std::ptr::from_ref(value).addr();
        if let Some(path) = self.value_bindings.get(&pointer) {
            return Some(path.clone());
        }
        let path = self
            .entries
            .iter()
            .filter(|entry| &entry.value == value)
            .map(|entry| &entry.path)
            .find(|path| !self.claimed_value_paths.contains(*path))?
            .clone();
        self.claimed_value_paths.insert(path.clone());
        self.value_bindings.insert(pointer, path.clone());
        Some(path)
    }

    fn range_for_value(&mut self, value: &Value) -> Option<TextRange> {
        let path = self.bound_value_path(value)?;
        self.entries
            .iter()
            .find(|entry| entry.path == path)
            .and_then(|entry| entry.range.clone())
    }

    fn range_for_field_value(&mut self, parent: &Value, key: &str) -> Option<TextRange> {
        let parent_path = self.bound_value_path(parent)?;
        let mut field_path = parent_path;
        field_path.push(YamlPathSegment::Key(key.to_string()));
        self.entries
            .iter()
            .find(|entry| entry.path == field_path)
            .and_then(|entry| entry.range.clone())
    }

    fn range_for_scalar(&mut self, value: &str) -> Option<TextRange> {
        let pointer = value.as_ptr().addr();
        let path = if let Some(path) = self.scalar_bindings.get(&pointer) {
            path.clone()
        } else {
            let path = self
                .entries
                .iter()
                .filter(|entry| entry.value.as_str() == Some(value))
                .map(|entry| &entry.path)
                .find(|path| !self.claimed_scalar_paths.contains(*path))?
                .clone();
            self.claimed_scalar_paths.insert(path.clone());
            self.scalar_bindings.insert(pointer, path.clone());
            path
        };
        self.entries
            .iter()
            .find(|entry| entry.path == path)
            .and_then(|entry| entry.range.clone())
    }
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

    // Export alone clones the session because external asset paths are rewritten
    // for the destination. Normal saves serialize the shared session directly.
    let mut synced = session.clone();
    for path in synced.source.documents.keys() {
        if !is_project_owned_path(path) {
            return Err(ExportProjectError::InvalidReference {
                path: path.clone(),
                reference: path.to_string(),
                message: "external imported documents must be vendored before export".to_string(),
            });
        }
    }
    for asset in &mut synced.source.referenced_assets {
        if !is_project_owned_path(&asset.relative_path) {
            let file_name = asset.absolute_path.file_name().ok_or_else(|| {
                ExportProjectError::InvalidReference {
                    path: asset.absolute_path.clone(),
                    reference: asset.absolute_path.to_string(),
                    message: "external asset has no file name".to_string(),
                }
            })?;
            asset.relative_path = Utf8PathBuf::from("assets")
                .join(asset.id.0.to_string())
                .join(file_name);
        }
    }
    let written_files = write_source_documents(&synced, output_root)?;

    let mut copied_assets = Vec::new();
    for (source_asset, exported_asset) in session
        .source
        .referenced_assets
        .iter()
        .zip(&synced.source.referenced_assets)
    {
        let output_path = output_root.join(&exported_asset.relative_path);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|source| ExportProjectError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::copy(&source_asset.absolute_path, &output_path).map_err(|source| {
            ExportProjectError::Io {
                path: output_path.clone(),
                source,
            }
        })?;
        copied_assets.push(exported_asset.relative_path.clone());
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

pub fn source_document_text(
    session: &ProjectSession,
    relative_path: &Utf8Path,
) -> Result<Option<String>, ExportProjectError> {
    let Some(document) = session.source.documents.get(relative_path) else {
        return Ok(None);
    };
    document_text(session, relative_path, document).map(Some)
}

fn ensure_document_imports_target(
    session: &mut ProjectSession,
    from_document: &Utf8Path,
    kind: &SourceObjectKind,
    reference: &str,
    target_document: Utf8PathBuf,
) -> Result<(), ExportProjectError> {
    let alias_base =
        canonical_reference_alias(kind).ok_or_else(|| ExportProjectError::InvalidReference {
            path: from_document.to_path_buf(),
            reference: reference.to_string(),
            message: format!("no canonical import alias exists for {kind:?} references"),
        })?;
    if session
        .source
        .documents
        .get(from_document)
        .is_some_and(|document| {
            document
                .imports
                .iter()
                .any(|edge| edge.targets.contains(&target_document))
        })
    {
        return Ok(());
    }

    let document = session.source.documents.get(from_document).ok_or_else(|| {
        ExportProjectError::InvalidReference {
            path: from_document.to_path_buf(),
            reference: reference.to_string(),
            message: "source document is missing from the source project".to_string(),
        }
    })?;
    let alias = available_import_alias(document, alias_base).ok_or_else(|| {
        ExportProjectError::InvalidReference {
            path: from_document.to_path_buf(),
            reference: reference.to_string(),
            message: format!("no import alias remains for `{alias_base}`"),
        }
    })?;
    let import_from = relative_path_from_document(from_document, &target_document);

    let document = session
        .source
        .documents
        .get_mut(from_document)
        .ok_or_else(|| ExportProjectError::InvalidReference {
            path: from_document.to_path_buf(),
            reference: reference.to_string(),
            message: "source document is missing from the source project".to_string(),
        })?;
    document.imports.push(ImportEdge {
        from: import_from.clone(),
        alias: alias.clone(),
        targets: vec![target_document],
    });
    Ok(())
}

pub fn ensure_document_can_reference_source(
    session: &mut ProjectSession,
    from_document: &Utf8Path,
    kind: SourceObjectKind,
    identity: &SourceIdentity,
) -> Result<(), ExportProjectError> {
    session
        .source
        .documents
        .get(identity.document())
        .and_then(|document| {
            document
                .objects
                .iter()
                .find(|object| object.kind == kind && object.id == identity.object())
        })
        .ok_or_else(|| ExportProjectError::InvalidReference {
            path: from_document.to_path_buf(),
            reference: identity.object().to_string(),
            message: "target is missing from its source document".to_string(),
        })?;
    ensure_document_imports_target(
        session,
        from_document,
        &kind,
        identity.object(),
        identity.document().to_path_buf(),
    )
}

pub fn insert_sequence(
    session: &mut ProjectSession,
    path: Utf8PathBuf,
    object_key: String,
    duration: DawnDuration,
    frame_rate: u32,
) -> Result<SequenceId, ExportProjectError> {
    if !is_project_owned_path(&path)
        || !path.starts_with("sequences")
        || !path
            .file_name()
            .is_some_and(|name| name.ends_with(".sequence.dawn"))
    {
        return Err(ExportProjectError::InvalidReference {
            path,
            reference: object_key,
            message: "sequence path must be an owned .sequence.dawn document under sequences/"
                .to_string(),
        });
    }
    if Identifier::new(object_key.clone()).is_err()
        || !duration.as_seconds_f64().is_finite()
        || duration.as_seconds_f64() <= 0.0
        || frame_rate == 0
    {
        return Err(ExportProjectError::InvalidReference {
            path,
            reference: object_key,
            message: "sequence identity, duration, or frame rate is invalid".to_string(),
        });
    }
    if session.source.documents.contains_key(&path)
        || session.source.source_root.join(&path).exists()
    {
        return Err(ExportProjectError::InvalidReference {
            path,
            reference: object_key,
            message: "source document already exists".to_string(),
        });
    }
    let identity = SourceIdentity::new(path.clone(), object_key.clone());
    let id = SequenceId(identity.clone());
    if session.project.sequences.contains_key(&id) {
        return Err(ExportProjectError::InvalidReference {
            path,
            reference: object_key,
            message: "sequence already exists".to_string(),
        });
    }
    let layer_id = SequenceLayerId(0);
    let sequence = Sequence {
        id: id.clone(),
        duration,
        frame_rate,
        audio: SequenceAudio::None,
        mark_collections: vec![MarkCollection {
            key: MarkCollectionKey {
                name: "marks".to_string(),
            },
            name: "Marks".to_string(),
            display_color: Color {
                red: 56,
                green: 189,
                blue: 248,
            },
            marks: Vec::new(),
        }],
        layers: vec![SequenceLayer {
            id: layer_id.clone(),
            name: "Default".to_string(),
            color: Color {
                red: 56,
                green: 189,
                blue: 248,
            },
            enabled: true,
        }],
        effects: Vec::new(),
        composition_graph: SequenceCompositionGraph {
            nodes: vec![
                CompositionGraphNode {
                    id: CompositionGraphNodeId(1),
                    position: GraphNodePosition { x: 80.0, y: 80.0 },
                    kind: CompositionGraphNodeKind::Layer { layer_id },
                },
                CompositionGraphNode {
                    id: CompositionGraphNodeId(2),
                    position: GraphNodePosition { x: 420.0, y: 80.0 },
                    kind: CompositionGraphNodeKind::Output,
                },
            ],
            edges: vec![EffectGraphEdge {
                from: CompositionGraphNodeId(1),
                from_port: GraphPortId("output".to_string()),
                to: CompositionGraphNodeId(2),
                to_port: GraphPortId("input".to_string()),
            }],
        },
        automation_clips: Vec::new(),
    };
    let source_document = SourceDocument::new(
        Vec::new(),
        vec![SourceObjectId {
            kind: SourceObjectKind::Sequence,
            id: identity.object().to_string(),
        }],
        SourceDocumentKind::Dawn {
            original_value: Value::Mapping(Mapping::new()),
        },
    )
    .map_err(|message| ExportProjectError::InvalidReference {
        path: path.clone(),
        reference: object_key.clone(),
        message,
    })?;
    session
        .source
        .documents
        .insert(path.clone(), source_document);
    session.project.sequences.insert(id.clone(), sequence);
    session.project.root.sequences.push(id.clone());
    let entrypoint = session.source.entrypoint.clone();
    ensure_document_can_reference_source(
        session,
        &entrypoint,
        SourceObjectKind::Sequence,
        &identity,
    )?;
    Ok(id)
}

fn available_import_alias(document: &SourceDocument, base: &str) -> Option<String> {
    if document.imports.iter().all(|import| import.alias != base) {
        return Some(base.to_string());
    }
    (2_u32..)
        .map(|suffix| format!("{base}_{suffix}"))
        .find(|candidate| {
            document
                .imports
                .iter()
                .all(|import| import.alias != *candidate)
        })
}

fn canonical_reference_alias(kind: &SourceObjectKind) -> Option<&'static str> {
    match kind {
        SourceObjectKind::EffectDefinition => Some("effects"),
        SourceObjectKind::OperatorDefinition => Some("operators"),
        SourceObjectKind::Curve => Some("curves"),
        SourceObjectKind::Sequence => Some("sequences"),
        _ => None,
    }
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
        message: String,
        range: Option<TextRange>,
    },
    InvalidDocument {
        path: Utf8PathBuf,
        range: Option<TextRange>,
        message: String,
    },
    InvalidReference {
        path: Utf8PathBuf,
        range: Option<TextRange>,
        reference: String,
    },
    InvalidEffect {
        path: Utf8PathBuf,
        diagnostics: Vec<IoDiagnostic>,
    },
    InvalidOperator {
        path: Utf8PathBuf,
        diagnostics: Vec<IoDiagnostic>,
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
    InvalidReference {
        path: Utf8PathBuf,
        reference: String,
        message: String,
    },
}

pub type SaveProjectError = ExportProjectError;

impl fmt::Display for LoadProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEntrypoint { path } => write!(formatter, "invalid entrypoint {path}"),
            Self::Io { path, source } => write!(formatter, "{path}: {source}"),
            Self::ParseYaml { path, message, .. } => write!(formatter, "{path}: {message}"),
            Self::InvalidDocument { path, message, .. } => write!(formatter, "{path}: {message}"),
            Self::InvalidReference {
                path, reference, ..
            } => {
                write!(formatter, "{path}: invalid reference {reference}")
            }
            Self::InvalidEffect { path, diagnostics } => {
                write!(
                    formatter,
                    "{path}: invalid effect: {}",
                    diagnostics
                        .iter()
                        .map(|diagnostic| diagnostic.message.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Self::InvalidOperator { path, diagnostics } => {
                write!(
                    formatter,
                    "{path}: invalid operator: {}",
                    diagnostics
                        .iter()
                        .map(|diagnostic| diagnostic.message.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
    }
}

impl std::error::Error for LoadProjectError {}

fn discover_reachable_files(
    path: &Utf8Path,
) -> Result<(Utf8PathBuf, Vec<Utf8PathBuf>), LoadProjectError> {
    let loader = Loader::new(path)?;
    let mut discovered = IndexSet::new();
    discover_reachable_file(&loader.source_root, &loader.entrypoint, &mut discovered)?;
    Ok((loader.source_root, discovered.into_iter().collect()))
}

fn discover_reachable_file(
    source_root: &Utf8Path,
    relative: &Utf8Path,
    discovered: &mut IndexSet<Utf8PathBuf>,
) -> Result<(), LoadProjectError> {
    if !discovered.insert(relative.to_path_buf()) {
        return Ok(());
    }

    let absolute = source_root.join(relative);
    let text = fs::read_to_string(&absolute).map_err(|source| LoadProjectError::Io {
        path: absolute.clone(),
        source,
    })?;
    if relative.file_name().is_some_and(|file_name| {
        file_name.ends_with(".effect.dawn") || file_name.ends_with(".operator.dawn")
    }) {
        return Ok(());
    }

    let value = parse_yaml_value(relative, &text)?;
    let Some(map) = mapping(&value) else {
        return Ok(());
    };
    for import in parse_imports(relative, map)? {
        let importer_dir = relative.parent().unwrap_or_else(|| Utf8Path::new(""));
        let target_relative = normalize_relative(importer_dir.join(&import.from));
        let target_absolute = source_root.join(&target_relative);
        if target_absolute.is_dir() {
            for child in sorted_dawn_children(source_root, &target_absolute)? {
                discover_reachable_file(source_root, &child, discovered)?;
            }
        } else if target_absolute.is_file() {
            discover_reachable_file(source_root, &target_relative, discovered)?;
        }
    }
    Ok(())
}

fn parse_yaml_value(path: &Utf8Path, text: &str) -> Result<Value, LoadProjectError> {
    let node = marked_yaml::parse_yaml_with_options(
        0,
        text,
        marked_yaml::LoaderOptions::default().error_on_duplicate_keys(true),
    )
    .map_err(|source| LoadProjectError::ParseYaml {
        path: path.to_path_buf(),
        range: marked_yaml_error_range(&source),
        message: source.to_string(),
    })?;
    let value: Value =
        yaml_serde::from_str(text).map_err(|source| LoadProjectError::ParseYaml {
            path: path.to_path_buf(),
            range: yaml_error_range(&source),
            message: source.to_string(),
        })?;
    let source_index = YamlSourceIndex::from_value_and_node(&value, &node);
    YAML_SOURCE_INDICES.with(|indices| {
        indices
            .borrow_mut()
            .insert(path.to_path_buf(), source_index);
    });
    Ok(value)
}

fn sorted_dawn_children(
    source_root: &Utf8Path,
    absolute: &Utf8Path,
) -> Result<Vec<Utf8PathBuf>, LoadProjectError> {
    let mut children = Vec::new();
    for entry in fs::read_dir(absolute).map_err(|source| LoadProjectError::Io {
        path: absolute.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| LoadProjectError::Io {
            path: absolute.to_path_buf(),
            source,
        })?;
        let path = Utf8PathBuf::from_path_buf(entry.path()).map_err(|path| {
            LoadProjectError::InvalidDocument {
                path: absolute.to_path_buf(),
                range: None,
                message: format!("non-utf8 import child path `{}`", path.display()),
            }
        })?;
        if path.is_file() && path.file_name().is_some_and(|name| name.ends_with(".dawn")) {
            children.push(relative_path(source_root, &path)?);
        }
    }
    children.sort();
    Ok(children)
}

fn check_absolute_document(source_root: &Utf8Path, relative: &Utf8Path) -> Vec<IoDiagnostic> {
    let path = source_root.join(relative);
    match fs::read_to_string(&path) {
        Ok(text) => {
            let diagnostic_path = relative.to_path_buf();
            check_document_text(&diagnostic_path, &text)
        }
        Err(source) => vec![IoDiagnostic {
            path: relative.to_path_buf(),
            range: None,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::IoRead,
            message: source.to_string(),
        }],
    }
}

fn effect_diagnostics(path: &Utf8Path, text: &str) -> Vec<IoDiagnostic> {
    match compile_effects(text) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics
            .into_iter()
            .map(|diagnostic| {
                dsl_diagnostic(path, text, diagnostic, IoDiagnosticCode::EffectCompile)
            })
            .collect(),
    }
}

fn operator_diagnostics(path: &Utf8Path, text: &str) -> Vec<IoDiagnostic> {
    match compile_operators(text) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics
            .into_iter()
            .map(|diagnostic| {
                dsl_diagnostic(path, text, diagnostic, IoDiagnosticCode::OperatorCompile)
            })
            .collect(),
    }
}

fn dsl_diagnostic(
    path: &Utf8Path,
    text: &str,
    diagnostic: DslDiagnostic,
    code: IoDiagnosticCode,
) -> IoDiagnostic {
    IoDiagnostic {
        path: path.to_path_buf(),
        range: Some(byte_range(text, diagnostic.span.start, diagnostic.span.end)),
        severity: IoDiagnosticSeverity::Error,
        code,
        message: diagnostic.message,
    }
}

fn load_error_diagnostic(error: LoadProjectError) -> IoDiagnostic {
    match error {
        LoadProjectError::InvalidEntrypoint { path } => IoDiagnostic {
            path,
            range: None,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::DawnLoad,
            message: "invalid entrypoint".to_string(),
        },
        LoadProjectError::Io { path, source } => IoDiagnostic {
            path,
            range: None,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::IoRead,
            message: source.to_string(),
        },
        LoadProjectError::ParseYaml {
            path,
            message,
            range,
        } => IoDiagnostic {
            path,
            range,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::YamlParse,
            message,
        },
        LoadProjectError::InvalidDocument {
            path,
            range,
            message,
        } => IoDiagnostic {
            path,
            range,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::DawnLoad,
            message,
        },
        LoadProjectError::InvalidReference {
            path,
            range,
            reference,
        } => IoDiagnostic {
            path,
            range,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::DawnReference,
            message: format!("invalid reference {reference}"),
        },
        LoadProjectError::InvalidEffect { path, diagnostics } => IoDiagnostic {
            path,
            range: None,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::EffectCompile,
            message: diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
                .join(", "),
        },
        LoadProjectError::InvalidOperator { path, diagnostics } => IoDiagnostic {
            path,
            range: None,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::OperatorCompile,
            message: diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
                .join(", "),
        },
    }
}

fn push_diagnostic(diagnostics: &mut Vec<IoDiagnostic>, diagnostic: IoDiagnostic) {
    if !diagnostics.contains(&diagnostic) {
        diagnostics.push(diagnostic);
    }
}

fn push_load_error_diagnostics(diagnostics: &mut Vec<IoDiagnostic>, error: LoadProjectError) {
    match error {
        LoadProjectError::InvalidEffect {
            diagnostics: effect_diagnostics,
            ..
        } => {
            for diagnostic in effect_diagnostics {
                push_diagnostic(diagnostics, diagnostic);
            }
        }
        LoadProjectError::InvalidOperator {
            diagnostics: operator_diagnostics,
            ..
        } => {
            for diagnostic in operator_diagnostics {
                push_diagnostic(diagnostics, diagnostic);
            }
        }
        other => push_diagnostic(diagnostics, load_error_diagnostic(other)),
    }
}

fn with_yaml_location(
    error: LoadProjectError,
    path: &Utf8Path,
    range: Option<TextRange>,
) -> LoadProjectError {
    match error {
        LoadProjectError::InvalidDocument { message, .. } => LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range,
            message,
        },
        LoadProjectError::InvalidReference { reference, .. } => {
            LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range,
                reference,
            }
        }
        other => other,
    }
}

fn yaml_error_range(error: &yaml_serde::Error) -> Option<TextRange> {
    let location = error.location()?;
    let line = location.line().saturating_sub(1) as u32;
    let character = location.column().saturating_sub(1) as u32;
    Some(TextRange {
        start: TextPosition { line, character },
        end: TextPosition {
            line,
            character: character.saturating_add(1),
        },
    })
}

fn marked_yaml_error_range(error: &MarkedYamlError) -> Option<TextRange> {
    match error {
        MarkedYamlError::TopLevelMustBeMapping(marker)
        | MarkedYamlError::TopLevelMustBeSequence(marker)
        | MarkedYamlError::UnexpectedAnchor(marker)
        | MarkedYamlError::MappingKeyMustBeScalar(marker)
        | MarkedYamlError::UnexpectedTag(marker)
        | MarkedYamlError::ScanError(marker, _) => Some(marker_range(marker)),
        MarkedYamlError::DuplicateKey(inner) => span_range(inner.key.span()),
    }
}

fn marker_range(marker: &Marker) -> TextRange {
    let line = marker.line().saturating_sub(1) as u32;
    let character = marker.column().saturating_sub(1) as u32;
    TextRange {
        start: TextPosition { line, character },
        end: TextPosition {
            line,
            character: character.saturating_add(1),
        },
    }
}

fn span_range(span: &marked_yaml::Span) -> Option<TextRange> {
    let start = span.start()?;
    let end = span.end().unwrap_or(start);
    let start_line = start.line().saturating_sub(1) as u32;
    let start_character = start.column().saturating_sub(1) as u32;
    let end_line = end.line().saturating_sub(1) as u32;
    let mut end_character = end.column().saturating_sub(1) as u32;
    if start_line == end_line && start_character == end_character {
        end_character = end_character.saturating_add(1);
    }
    Some(TextRange {
        start: TextPosition {
            line: start_line,
            character: start_character,
        },
        end: TextPosition {
            line: end_line,
            character: end_character,
        },
    })
}

fn node_range(node: &Node) -> Option<TextRange> {
    match node {
        Node::Scalar(scalar) => scalar_range(scalar),
        Node::Mapping(mapping) => span_range(mapping.span()),
        Node::Sequence(sequence) => span_range(sequence.span()),
    }
}

fn scalar_range(scalar: &marked_yaml::types::MarkedScalarNode) -> Option<TextRange> {
    let start = scalar.span().start()?;
    let line = start.line().saturating_sub(1) as u32;
    let character = start.column().saturating_sub(1) as u32;
    let width = scalar.as_str().chars().count().max(1) as u32;
    Some(TextRange {
        start: TextPosition { line, character },
        end: TextPosition {
            line,
            character: character.saturating_add(width),
        },
    })
}

fn source_range_for_value(path: &Utf8Path, value: &Value) -> Option<TextRange> {
    YAML_SOURCE_INDICES.with(|indices| {
        indices
            .borrow_mut()
            .get_mut(path)
            .and_then(|index| index.range_for_value(value))
    })
}

fn source_range_for_field_value(path: &Utf8Path, value: &Value, key: &str) -> Option<TextRange> {
    YAML_SOURCE_INDICES.with(|indices| {
        indices
            .borrow_mut()
            .get_mut(path)
            .and_then(|index| index.range_for_field_value(value, key))
    })
}

fn source_range_for_scalar(path: &Utf8Path, value: &str) -> Option<TextRange> {
    YAML_SOURCE_INDICES.with(|indices| {
        indices
            .borrow_mut()
            .get_mut(path)
            .and_then(|index| index.range_for_scalar(value))
    })
}

fn byte_range(text: &str, start: usize, end: usize) -> TextRange {
    let start = byte_position(text, start);
    let mut end = byte_position(text, end);
    if end == start {
        end.character = end.character.saturating_add(1);
    }
    TextRange { start, end }
}

fn byte_position(text: &str, byte_offset: usize) -> TextPosition {
    let clamped = byte_offset.min(text.len());
    let mut line = 0;
    let mut line_start = 0;
    for (index, character) in text.char_indices() {
        if index >= clamped {
            break;
        }
        if character == '\n' {
            line += 1;
            line_start = index + character.len_utf8();
        }
    }
    TextPosition {
        line,
        character: text[line_start..clamped].chars().count() as u32,
    }
}

impl fmt::Display for ExportProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputRootIsFile { path } => write!(formatter, "output root is a file: {path}"),
            Self::Io { path, source } => write!(formatter, "{path}: {source}"),
            Self::Serialize { path, source } => write!(formatter, "{path}: {source}"),
            Self::InvalidReference {
                path,
                reference,
                message,
            } => write!(
                formatter,
                "{path}: invalid reference {reference}: {message}"
            ),
        }
    }
}

impl std::error::Error for ExportProjectError {}

fn write_source_documents(
    session: &ProjectSession,
    output_root: &Utf8Path,
) -> Result<Vec<Utf8PathBuf>, ExportProjectError> {
    let mut prepared = Vec::new();
    for (relative_path, document) in &session.source.documents {
        if !is_project_owned_path(relative_path) {
            continue;
        }
        let output_path = output_root.join(relative_path);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|source| ExportProjectError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let text =
            document_text(session, relative_path, document).map_err(|error| match error {
                ExportProjectError::Serialize { source, .. } => ExportProjectError::Serialize {
                    path: output_path.clone(),
                    source,
                },
                other => other,
            })?;
        let previous = if output_path.is_file() {
            Some(
                fs::read(&output_path).map_err(|source| ExportProjectError::Io {
                    path: output_path.clone(),
                    source,
                })?,
            )
        } else {
            None
        };
        prepared.push((relative_path.clone(), output_path, text, previous));
    }
    let mut written = 0usize;
    while written < prepared.len() {
        let (_, output_path, text, _) = &prepared[written];
        if let Err(write_error) = fs::write(output_path, text) {
            let mut rollback_failures = Vec::new();
            for (_, written_path, _, previous) in prepared[..=written].iter().rev() {
                let rollback = match previous {
                    Some(bytes) => fs::write(written_path, bytes),
                    None => fs::remove_file(written_path),
                };
                if let Err(error) = rollback {
                    rollback_failures.push(format!("{written_path}: {error}"));
                }
            }
            if rollback_failures.is_empty() {
                return Err(ExportProjectError::Io {
                    path: output_path.clone(),
                    source: write_error,
                });
            }
            return Err(ExportProjectError::Io {
                path: output_path.clone(),
                source: io::Error::other(format!(
                    "{write_error}; rollback also failed: {}",
                    rollback_failures.join(", ")
                )),
            });
        }
        written += 1;
    }
    Ok(prepared
        .into_iter()
        .map(|(relative_path, _, _, _)| relative_path)
        .collect())
}

fn document_text(
    session: &ProjectSession,
    relative_path: &Utf8Path,
    document: &SourceDocument,
) -> Result<String, ExportProjectError> {
    match &document.kind {
        SourceDocumentKind::Dawn { original_value } => {
            let existing = mapping(original_value);
            let mut root = Mapping::new();
            if !document.imports.is_empty() {
                root.insert(
                    string_value("imports"),
                    import_decls_value(&document.imports),
                );
            }
            for object in &document.objects {
                let value = if has_typed_object(session, relative_path, object) {
                    serialize_source_object(session, relative_path, object)?
                } else {
                    existing
                        .and_then(|mapping| mapping.get(string_value(&object.id)))
                        .cloned()
                        .ok_or_else(|| ExportProjectError::InvalidReference {
                            path: relative_path.to_path_buf(),
                            reference: object.id.clone(),
                            message: "source object is missing from the source document"
                                .to_string(),
                        })?
                };
                root.insert(string_value(&object.id), value);
            }
            yaml_serde::to_string(&Value::Mapping(root)).map_err(|source| {
                ExportProjectError::Serialize {
                    path: relative_path.to_path_buf(),
                    source,
                }
            })
        }
        SourceDocumentKind::Effect { source } => Ok(source.clone()),
        SourceDocumentKind::Operator { source } => Ok(source.clone()),
    }
}

fn has_typed_object(session: &ProjectSession, document: &Utf8Path, id: &SourceObjectId) -> bool {
    match id.kind {
        SourceObjectKind::Project => qualified_identity(session, document, id)
            .is_some_and(|identity| session.project.root.id == ProjectId(identity)),
        SourceObjectKind::Setup => qualified_identity(session, document, id)
            .is_some_and(|identity| session.project.setups.contains_key(&SetupId(identity))),
        SourceObjectKind::Controller => {
            qualified_identity(session, document, id).is_some_and(|identity| {
                session
                    .project
                    .controllers
                    .contains_key(&ControllerId(identity))
            })
        }
        SourceObjectKind::Layout => qualified_identity(session, document, id)
            .is_some_and(|identity| session.project.layouts.contains_key(&LayoutId(identity))),
        SourceObjectKind::Patch => qualified_identity(session, document, id)
            .is_some_and(|identity| session.project.patches.contains_key(&PatchId(identity))),
        SourceObjectKind::FixtureDefinition => qualified_identity(session, document, id)
            .is_some_and(|identity| {
                session
                    .project
                    .definitions
                    .fixtures
                    .definitions
                    .contains_key(&FixtureDefinitionId(identity))
            }),
        SourceObjectKind::Curve => {
            qualified_identity(session, document, id).is_some_and(|identity| {
                session
                    .project
                    .definitions
                    .curves
                    .definitions
                    .contains_key(&CurveId(identity))
            })
        }
        SourceObjectKind::Sequence => {
            qualified_identity(session, document, id).is_some_and(|identity| {
                session
                    .project
                    .sequences
                    .contains_key(&SequenceId(identity))
            })
        }
        SourceObjectKind::EffectDefinition => qualified_identity(session, document, id)
            .is_some_and(|identity| {
                session
                    .project
                    .definitions
                    .effects
                    .definitions
                    .contains_key(&EffectDefinitionId(identity))
            }),
        SourceObjectKind::OperatorDefinition => qualified_identity(session, document, id)
            .is_some_and(|identity| {
                session
                    .project
                    .definitions
                    .operators
                    .definitions
                    .contains_key(&OperatorDefinitionId(identity))
            }),
        SourceObjectKind::EffectInstance => false,
    }
}

fn qualified_identity(
    session: &ProjectSession,
    document: &Utf8Path,
    id: &SourceObjectId,
) -> Option<SourceIdentity> {
    session
        .source
        .documents
        .get(document)
        .is_some_and(|document| document.objects.contains(id))
        .then(|| SourceIdentity::new(document.to_path_buf(), id.id.clone()))
}

fn serialize_source_object(
    session: &ProjectSession,
    from_document: &Utf8Path,
    id: &SourceObjectId,
) -> Result<Value, ExportProjectError> {
    match id.kind {
        SourceObjectKind::Project => project_root_value(session, from_document),
        SourceObjectKind::Setup => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let setup = session
                .project
                .setups
                .get(&SetupId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            setup_value(session, from_document, setup)
        }
        SourceObjectKind::Controller => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let controller = session
                .project
                .controllers
                .get(&ControllerId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            controller_value(controller)
        }
        SourceObjectKind::Layout => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let layout = session
                .project
                .layouts
                .get(&LayoutId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            layout_value(session, from_document, layout)
        }
        SourceObjectKind::Patch => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let patch = session
                .project
                .patches
                .get(&PatchId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            patch_value(session, from_document, patch)
        }
        SourceObjectKind::FixtureDefinition => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let definition = session
                .project
                .definitions
                .fixtures
                .definitions
                .get(&FixtureDefinitionId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            fixture_definition_value(definition)
        }
        SourceObjectKind::Curve => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let curve = session
                .project
                .definitions
                .curves
                .definitions
                .get(&CurveId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            curve_value(&curve.curve)
        }
        SourceObjectKind::Sequence => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let sequence = session
                .project
                .sequences
                .get(&SequenceId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            sequence_value(session, from_document, sequence)
        }
        SourceObjectKind::EffectDefinition
        | SourceObjectKind::OperatorDefinition
        | SourceObjectKind::EffectInstance => Err(ExportProjectError::InvalidReference {
            path: from_document.to_path_buf(),
            reference: id.id.clone(),
            message: "DSL definitions are preserved as source documents".to_string(),
        }),
    }
}

fn missing_typed_object(path: &Utf8Path, id: &SourceObjectId) -> ExportProjectError {
    ExportProjectError::InvalidReference {
        path: path.to_path_buf(),
        reference: id.id.clone(),
        message: "typed project object is missing".to_string(),
    }
}

fn import_decls_value(imports: &[ImportEdge]) -> Value {
    Value::Sequence(
        imports
            .iter()
            .map(|import| {
                let mut value = Mapping::new();
                value.insert(string_value("from"), Value::String(import.from.to_string()));
                value.insert(string_value("as"), Value::String(import.alias.clone()));
                Value::Mapping(value)
            })
            .collect(),
    )
}

fn project_root_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("project");
    value.insert(
        string_value("setup"),
        Value::String(write_source_reference(
            session,
            from_document,
            SourceObjectKind::Setup,
            &session.project.root.setup.0,
        )?),
    );
    value.insert(
        string_value("sequences"),
        Value::Sequence(
            session
                .project
                .root
                .sequences
                .iter()
                .map(|sequence| {
                    write_source_reference(
                        session,
                        from_document,
                        SourceObjectKind::Sequence,
                        &sequence.0,
                    )
                    .map(Value::String)
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

fn setup_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    setup: &Setup,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("setup");
    value.insert(
        string_value("layout"),
        Value::String(write_source_reference(
            session,
            from_document,
            SourceObjectKind::Layout,
            &setup.layout.0,
        )?),
    );
    value.insert(
        string_value("patch"),
        Value::String(write_source_reference(
            session,
            from_document,
            SourceObjectKind::Patch,
            &setup.patch.0,
        )?),
    );
    value.insert(
        string_value("controllers"),
        Value::Sequence(
            setup
                .controllers
                .iter()
                .map(|controller| Value::String(controller.0.object().to_string()))
                .collect(),
        ),
    );
    Ok(Value::Mapping(value))
}

fn controller_value(controller: &ControllerDefinition) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("controller");
    value.insert(
        string_value("protocol"),
        Value::String(
            match controller.protocol {
                Protocol::E131 => "sacn",
                Protocol::Artnet => "artnet",
            }
            .to_string(),
        ),
    );
    if let Some(address) = &controller.address {
        value.insert(
            string_value("destination"),
            Value::String(format!("{}:{}", address.ip, address.port)),
        );
    }
    let Some(first) = controller.outputs.first() else {
        value.insert(string_value("output"), Value::Mapping(Mapping::new()));
        return Ok(Value::Mapping(value));
    };
    let linear = controller
        .outputs
        .iter()
        .enumerate()
        .all(|(index, output)| {
            output.channel_order == first.channel_order
                && output.pixels == first.pixels
                && output.first_universe == first.first_universe + index as u32
        });
    let mut output = Mapping::new();
    output.insert(
        string_value("channel_order"),
        Value::String(channel_order_name(&first.channel_order).to_string()),
    );
    if linear {
        output.insert(
            string_value("type"),
            Value::String("linear_rgb".to_string()),
        );
        output.insert(
            string_value("output_count"),
            number_value(controller.outputs.len() as u32)?,
        );
        output.insert(
            string_value("pixels_per_output"),
            number_value(first.pixels as u32)?,
        );
        output.insert(
            string_value("first_universe"),
            number_value(first.first_universe)?,
        );
    } else {
        output.insert(
            string_value("type"),
            Value::String("patched_dmx".to_string()),
        );
        output.insert(
            string_value("universes"),
            Value::Sequence(
                controller
                    .outputs
                    .iter()
                    .map(|output| {
                        let mut universe = Mapping::new();
                        universe.insert(string_value("id"), number_value(output.first_universe)?);
                        universe.insert(
                            string_value("range"),
                            Value::String(format!("1..{}", output.pixels * 3)),
                        );
                        Ok(Value::Mapping(universe))
                    })
                    .collect::<Result<Vec<_>, ExportProjectError>>()?,
            ),
        );
    }
    value.insert(string_value("output"), Value::Mapping(output));
    Ok(Value::Mapping(value))
}

fn layout_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    layout: &Layout,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("layout");
    value.insert(
        string_value("target_order"),
        Value::Sequence(
            layout
                .target_order
                .iter()
                .map(layout_target_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("fixtures"),
        Value::Sequence(
            layout
                .fixtures
                .iter()
                .map(|fixture| fixture_inst_value(session, from_document, fixture))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("groups"),
        Value::Sequence(
            layout
                .groups
                .iter()
                .map(fixture_group_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

fn fixture_inst_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    fixture: &FixtureInst,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), number_value(fixture.id.0)?);
    value.insert(string_value("name"), Value::String(fixture.name.clone()));
    value.insert(
        string_value("fixture"),
        Value::String(write_source_reference(
            session,
            from_document,
            SourceObjectKind::FixtureDefinition,
            &fixture.definition.0,
        )?),
    );
    value.insert(string_value("transform"), transform_value(fixture)?);
    Ok(Value::Mapping(value))
}

fn fixture_group_value(group: &FixtureGroup) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), number_value(group.id.0)?);
    value.insert(string_value("name"), Value::String(group.name.clone()));
    value.insert(
        string_value("members"),
        Value::Sequence(
            group
                .fixtures
                .iter()
                .map(|fixture| number_value(fixture.0))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

fn patch_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    patch: &Patch,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("patch");
    value.insert(
        string_value("routes"),
        Value::Sequence(
            patch
                .routes
                .iter()
                .map(|route| patch_route_value(session, from_document, route))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

fn patch_route_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    route: &PatchRoute,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("fixture"), number_value(route.fixture.0)?);
    value.insert(
        string_value("controller"),
        Value::String(write_source_reference(
            session,
            from_document,
            SourceObjectKind::Controller,
            &route.controller.0,
        )?),
    );
    value.insert(string_value("output"), number_value(route.output.0)?);
    value.insert(
        string_value("start_channel_offset"),
        number_value(route.start_channel_offset)?,
    );
    value.insert(
        string_value("fixture_pixel_start"),
        number_value(route.fixture_pixels.start)?,
    );
    value.insert(
        string_value("fixture_pixel_count"),
        number_value(route.fixture_pixels.count)?,
    );
    Ok(Value::Mapping(value))
}

fn fixture_definition_value(definition: &FixtureDefinition) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("fixture");
    value.insert(
        string_value("bulb_diameter"),
        number_value(distance_span_meters(definition.bulb_radius) * 2.0)?,
    );
    value.insert(
        string_value("geometry"),
        geometry_value(&definition.geometry)?,
    );
    Ok(Value::Mapping(value))
}

fn sequence_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    sequence: &Sequence,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("sequence");
    value.insert(
        string_value("duration"),
        Value::String(seconds_string(sequence.duration.as_seconds_f64())),
    );
    value.insert(
        string_value("frame_rate"),
        number_value(sequence.frame_rate)?,
    );
    match &sequence.audio {
        SequenceAudio::None => {
            value.insert(string_value("audio"), Value::Null);
        }
        SequenceAudio::Asset(id) => {
            let asset = session
                .source
                .referenced_assets
                .iter()
                .find(|asset| asset.id == *id)
                .ok_or_else(|| ExportProjectError::InvalidReference {
                    path: from_document.to_path_buf(),
                    reference: id.0.to_string(),
                    message: "sequence audio asset is missing from source metadata".to_string(),
                })?;
            value.insert(
                string_value("audio"),
                Value::String(
                    relative_path_from_document(from_document, &asset.relative_path).to_string(),
                ),
            );
        }
    }
    value.insert(
        string_value("mark_collections"),
        Value::Sequence(
            sequence
                .mark_collections
                .iter()
                .map(mark_collection_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("layers"),
        Value::Sequence(
            sequence
                .layers
                .iter()
                .map(sequence_layer_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("effects"),
        Value::Sequence(
            sequence
                .effects
                .iter()
                .map(|effect| sequence_effect_value(session, from_document, effect))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("composition_graph"),
        composition_graph_value(session, from_document, &sequence.composition_graph)?,
    );
    value.insert(
        string_value("automation_clips"),
        Value::Sequence(
            sequence
                .automation_clips
                .iter()
                .map(automation_clip_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

fn sequence_layer_value(layer: &SequenceLayer) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), number_value(layer.id.0)?);
    value.insert(string_value("name"), Value::String(layer.name.clone()));
    value.insert(
        string_value("color"),
        Value::String(color_string(layer.color)),
    );
    value.insert(string_value("enabled"), Value::Bool(layer.enabled));
    Ok(Value::Mapping(value))
}

fn sequence_effect_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    effect: &EffectInst,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), number_value(effect.id.0)?);
    value.insert(string_value("layer_id"), number_value(effect.layer_id.0)?);
    value.insert(
        string_value("start"),
        Value::String(seconds_string(effect.start.as_seconds_f64())),
    );
    value.insert(
        string_value("duration"),
        Value::String(seconds_string(effect.duration.as_seconds_f64())),
    );
    value.insert(string_value("target"), effect_target_value(&effect.target)?);
    value.insert(
        string_value("scope"),
        Value::String(
            match effect.scope {
                EffectScope::PerFixture => "per_fixture",
                EffectScope::WholeTarget => "whole_target",
            }
            .to_string(),
        ),
    );
    write_effect_clip_fields(
        session,
        from_document,
        &mut value,
        &EffectClip {
            definition: effect.definition.clone(),
            param_overrides: effect.param_overrides.clone(),
        },
    )?;
    Ok(Value::Mapping(value))
}

fn composition_graph_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    graph: &SequenceCompositionGraph,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(
        string_value("nodes"),
        Value::Sequence(
            graph
                .nodes
                .iter()
                .map(|node| composition_graph_node_value(session, from_document, node))
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    value.insert(
        string_value("edges"),
        Value::Sequence(
            graph
                .edges
                .iter()
                .map(graph_edge_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

fn composition_graph_node_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    node: &CompositionGraphNode,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), number_value(node.id.0)?);
    value.insert(
        string_value("position"),
        graph_position_value(&node.position)?,
    );
    match &node.kind {
        CompositionGraphNodeKind::Layer { layer_id } => {
            value.insert(string_value("type"), Value::String("layer".to_string()));
            value.insert(string_value("layer_id"), number_value(layer_id.0)?);
        }
        CompositionGraphNodeKind::Operator(operator) => {
            value.insert(string_value("type"), Value::String("operator".to_string()));
            value.insert(
                string_value("operator"),
                Value::String(graph_operator_name(
                    session,
                    from_document,
                    &operator.operator,
                )?),
            );
            if !operator.params.is_empty() {
                value.insert(
                    string_value("params"),
                    Value::Mapping(
                        operator
                            .params
                            .iter()
                            .map(|(name, param)| {
                                Ok((
                                    string_value(name.as_str()),
                                    effect_param_value(session, from_document, param)?,
                                ))
                            })
                            .collect::<Result<Mapping, ExportProjectError>>()?,
                    ),
                );
            }
        }
        CompositionGraphNodeKind::Output => {
            value.insert(string_value("type"), Value::String("output".to_string()));
        }
    }
    Ok(Value::Mapping(value))
}

fn mark_collection_value(collection: &MarkCollection) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(
        string_value("key"),
        Value::String(collection.key.name.clone()),
    );
    value.insert(string_value("name"), Value::String(collection.name.clone()));
    value.insert(
        string_value("color"),
        Value::String(color_string(collection.display_color)),
    );
    value.insert(
        string_value("marks"),
        Value::Sequence(
            collection
                .marks
                .iter()
                .map(|time| Value::String(seconds_string(time.as_seconds_f64())))
                .collect(),
        ),
    );
    Ok(Value::Mapping(value))
}

fn write_effect_clip_fields(
    session: &ProjectSession,
    from_document: &Utf8Path,
    value: &mut Mapping,
    effect: &EffectClip,
) -> Result<(), ExportProjectError> {
    if !effect.param_overrides.is_empty() {
        value.insert(
            string_value("params"),
            Value::Mapping(
                effect
                    .param_overrides
                    .iter()
                    .map(|(name, param)| {
                        Ok((
                            string_value(name.as_str()),
                            effect_param_value(session, from_document, param)?,
                        ))
                    })
                    .collect::<Result<Mapping, ExportProjectError>>()?,
            ),
        );
    }
    value.insert(
        string_value("script"),
        Value::String(write_source_reference(
            session,
            from_document,
            SourceObjectKind::EffectDefinition,
            &effect.definition.0,
        )?),
    );
    Ok(())
}

fn graph_position_value(position: &GraphNodePosition) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("x"), number_value(position.x)?);
    value.insert(string_value("y"), number_value(position.y)?);
    Ok(Value::Mapping(value))
}

fn graph_edge_value(edge: &EffectGraphEdge) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("from"), number_value(edge.from.0)?);
    value.insert(
        string_value("from_port"),
        Value::String(edge.from_port.0.clone()),
    );
    value.insert(string_value("to"), number_value(edge.to.0)?);
    value.insert(
        string_value("to_port"),
        Value::String(edge.to_port.0.clone()),
    );
    Ok(Value::Mapping(value))
}

fn graph_operator_name(
    session: &ProjectSession,
    from_document: &Utf8Path,
    operator: &OperatorRef,
) -> Result<String, ExportProjectError> {
    match operator {
        OperatorRef::Builtin(operator) => Ok(operator.definition().source_name.clone()),
        OperatorRef::Custom(id) => write_source_reference(
            session,
            from_document,
            SourceObjectKind::OperatorDefinition,
            &id.0,
        ),
    }
}

fn automation_clip_value(clip: &AutomationClip) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), number_value(clip.id.0)?);
    value.insert(
        string_value("start"),
        Value::String(seconds_string(clip.start.as_seconds_f64())),
    );
    value.insert(
        string_value("duration"),
        Value::String(seconds_string(clip.duration.as_seconds_f64())),
    );
    value.insert(
        string_value("anchor_lane_index"),
        number_value(clip.anchor_lane_index)?,
    );
    value.insert(string_value("lane_index"), number_value(clip.lane_index)?);
    value.insert(string_value("curve"), automation_curve_value(&clip.curve)?);
    value.insert(
        string_value("bindings"),
        Value::Sequence(
            clip.bindings
                .iter()
                .map(automation_binding_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

fn automation_curve_value(curve: &Curve) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(
        string_value("value_type"),
        Value::String("float".to_string()),
    );
    value.insert(
        string_value("points"),
        Value::Sequence(
            curve
                .points
                .iter()
                .map(|point| {
                    let mut value = Mapping::new();
                    value.insert(string_value("time"), number_value(point.position)?);
                    let CurveValue::Float(point_value) = point.value else {
                        return Err(ExportProjectError::InvalidReference {
                            path: Utf8PathBuf::from("<sync>"),
                            reference: "automation.curve".to_string(),
                            message: "automation curves must contain float points".to_string(),
                        });
                    };
                    value.insert(string_value("value"), number_value(point_value)?);
                    Ok(Value::Mapping(value))
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

fn automation_binding_value(binding: &AutomationBinding) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(
        string_value("target"),
        automation_target_value(&binding.target)?,
    );
    value.insert(
        string_value("mapping"),
        automation_mapping_value(&binding.mapping)?,
    );
    Ok(Value::Mapping(value))
}

fn automation_target_value(target: &AutomationTarget) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    match target {
        AutomationTarget::EffectParam { effect_id, param } => {
            value.insert(
                string_value("type"),
                Value::String("effect_param".to_string()),
            );
            value.insert(string_value("effect_id"), number_value(effect_id.0)?);
            value.insert(
                string_value("param"),
                Value::String(param.as_str().to_string()),
            );
        }
        AutomationTarget::CompositionNodeParam { node_id, param } => {
            value.insert(
                string_value("type"),
                Value::String("composition_node_param".to_string()),
            );
            value.insert(string_value("node_id"), number_value(node_id.0)?);
            value.insert(
                string_value("param"),
                Value::String(param.as_str().to_string()),
            );
        }
    }
    Ok(Value::Mapping(value))
}

fn automation_mapping_value(mapping: &AutomationMapping) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    match mapping {
        AutomationMapping::Float { min, max } => {
            value.insert(string_value("type"), Value::String("float".to_string()));
            value.insert(string_value("min"), number_value(*min)?);
            value.insert(string_value("max"), number_value(*max)?);
        }
        AutomationMapping::Int { min, max } => {
            value.insert(string_value("type"), Value::String("int".to_string()));
            value.insert(string_value("min"), number_value(*min)?);
            value.insert(string_value("max"), number_value(*max)?);
        }
        AutomationMapping::Bool => {
            value.insert(string_value("type"), Value::String("bool".to_string()));
        }
        AutomationMapping::Enum { values } => {
            value.insert(string_value("type"), Value::String("enum".to_string()));
            value.insert(
                string_value("values"),
                Value::Sequence(
                    values
                        .iter()
                        .map(|value| Value::String(value.as_str().to_string()))
                        .collect(),
                ),
            );
        }
        AutomationMapping::FloatCurve { min, max } => {
            value.insert(
                string_value("type"),
                Value::String("float_curve".to_string()),
            );
            value.insert(string_value("min"), number_value(*min)?);
            value.insert(string_value("max"), number_value(*max)?);
        }
    }
    Ok(Value::Mapping(value))
}

fn effect_param_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    param: &EffectParamValue,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    match param {
        EffectParamValue::Int(inner) => {
            value.insert(string_value("type"), Value::String("integer".to_string()));
            value.insert(string_value("value"), number_value(*inner)?);
        }
        EffectParamValue::Float(inner) => {
            value.insert(string_value("type"), Value::String("float".to_string()));
            value.insert(string_value("value"), number_value(*inner)?);
        }
        EffectParamValue::Bool(inner) => {
            value.insert(string_value("type"), Value::String("bool".to_string()));
            value.insert(string_value("value"), Value::Bool(*inner));
        }
        EffectParamValue::Color(inner) => {
            value.insert(string_value("type"), Value::String("color".to_string()));
            value.insert(string_value("value"), Value::String(color_string(*inner)));
        }
        EffectParamValue::Enum(inner) => {
            value.insert(string_value("type"), Value::String("enum".to_string()));
            value.insert(
                string_value("value"),
                Value::String(inner.as_str().to_string()),
            );
        }
        EffectParamValue::Marks(inner) => {
            value.insert(string_value("type"), Value::String("marks".to_string()));
            value.insert(string_value("key"), Value::String(inner.name.clone()));
        }
        EffectParamValue::Curve(inner) => {
            value.insert(string_value("type"), Value::String("curve".to_string()));
            value.insert(
                string_value("curve"),
                curve_source_value(session, from_document, inner)?,
            );
        }
        EffectParamValue::Array(values) => {
            value.insert(string_value("type"), Value::String("array".to_string()));
            value.insert(
                string_value("element_type"),
                Value::String(array_element_type(values).to_string()),
            );
            value.insert(
                string_value("values"),
                Value::Sequence(
                    values
                        .iter()
                        .map(|item| array_item_value(session, from_document, item))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            );
        }
    }
    Ok(Value::Mapping(value))
}

fn array_item_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    param: &EffectParamValue,
) -> Result<Value, ExportProjectError> {
    match param {
        EffectParamValue::Curve(source) => {
            let mut value = Mapping::new();
            value.insert(
                string_value("curve"),
                curve_source_value(session, from_document, source)?,
            );
            Ok(Value::Mapping(value))
        }
        _ => effect_param_value(session, from_document, param),
    }
}

fn array_element_type(values: &[EffectParamValue]) -> &'static str {
    match values.first() {
        Some(EffectParamValue::Int(_)) => "integer",
        Some(EffectParamValue::Float(_)) => "float",
        Some(EffectParamValue::Bool(_)) => "bool",
        Some(EffectParamValue::Color(_)) => "color",
        Some(EffectParamValue::Curve(CurveSource::Inline(curve))) => match curve.points.first() {
            Some(CurvePoint {
                value: CurveValue::Color(_),
                ..
            }) => "curve_color",
            _ => "curve_float",
        },
        Some(EffectParamValue::Curve(CurveSource::Reference(_))) => "curve",
        _ => "float",
    }
}

fn curve_source_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    source: &CurveSource,
) -> Result<Value, ExportProjectError> {
    match source {
        CurveSource::Inline(curve) => curve_value(curve),
        CurveSource::Reference(id) => Ok(Value::String(write_source_reference(
            session,
            from_document,
            SourceObjectKind::Curve,
            &id.0,
        )?)),
    }
}

fn curve_value(curve: &Curve) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("curve");
    let value_type = match curve.points.first() {
        Some(CurvePoint {
            value: CurveValue::Color(_),
            ..
        }) => "color",
        _ => "float",
    };
    value.insert(
        string_value("value_type"),
        Value::String(value_type.to_string()),
    );
    value.insert(
        string_value("points"),
        Value::Sequence(
            curve
                .points
                .iter()
                .map(|point| {
                    let mut value = Mapping::new();
                    value.insert(string_value("time"), number_value(point.position)?);
                    value.insert(
                        string_value("value"),
                        match point.value {
                            CurveValue::Float(inner) => number_value(inner)?,
                            CurveValue::Color(inner) => Value::String(color_string(inner)),
                        },
                    );
                    Ok(Value::Mapping(value))
                })
                .collect::<Result<Vec<_>, ExportProjectError>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

fn geometry_value(geometry: &Geometry) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    match geometry {
        Geometry::Points { points } => {
            value.insert(string_value("type"), Value::String("points".to_string()));
            value.insert(
                string_value("points"),
                Value::Sequence(
                    points
                        .iter()
                        .map(point_value)
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            );
        }
        Geometry::Lines { points, pixels } => {
            value.insert(string_value("type"), Value::String("lines".to_string()));
            value.insert(
                string_value("points"),
                Value::Sequence(
                    points
                        .iter()
                        .map(point_value)
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            );
            value.insert(string_value("pixels"), number_value(*pixels)?);
        }
        Geometry::Arc {
            center,
            radius,
            start_degrees,
            end_degrees,
            pixels,
        } => {
            value.insert(string_value("type"), Value::String("arc".to_string()));
            value.insert(string_value("center"), point_value(center)?);
            value.insert(
                string_value("radius"),
                number_value(distance_span_meters(*radius))?,
            );
            value.insert(string_value("startDegrees"), number_value(*start_degrees)?);
            value.insert(string_value("endDegrees"), number_value(*end_degrees)?);
            value.insert(string_value("pixels"), number_value(*pixels)?);
        }
    }
    Ok(Value::Mapping(value))
}

fn transform_value(fixture: &FixtureInst) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("position"), point_value(&fixture.position)?);
    value.insert(string_value("rotation"), rotation_value(&fixture.rotation)?);
    value.insert(string_value("scale"), scale_value(&fixture.scale)?);
    Ok(Value::Mapping(value))
}

fn point_value(point: &Point3) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("x"), number_value(distance_meters(point.x))?);
    value.insert(string_value("y"), number_value(distance_meters(point.y))?);
    value.insert(string_value("z"), number_value(distance_meters(point.z))?);
    Ok(Value::Mapping(value))
}

fn rotation_value(rotation: &Rotation3) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("x"), number_value(rotation.x)?);
    value.insert(string_value("y"), number_value(rotation.y)?);
    value.insert(string_value("z"), number_value(rotation.z)?);
    Ok(Value::Mapping(value))
}

fn scale_value(scale: &Scale3) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("x"), number_value(scale.x)?);
    value.insert(string_value("y"), number_value(scale.y)?);
    value.insert(string_value("z"), number_value(scale.z)?);
    Ok(Value::Mapping(value))
}

fn layout_target_value(target: &LayoutTarget) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    match target {
        LayoutTarget::Fixture(id) => {
            value.insert(string_value("type"), Value::String("fixture".to_string()));
            value.insert(string_value("id"), number_value(id.0)?);
        }
        LayoutTarget::Group(id) => {
            value.insert(string_value("type"), Value::String("group".to_string()));
            value.insert(string_value("id"), number_value(id.0)?);
        }
    }
    Ok(Value::Mapping(value))
}

fn effect_target_value(target: &EffectTarget) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    match target {
        EffectTarget::Fixture(id) => {
            value.insert(string_value("type"), Value::String("fixture".to_string()));
            value.insert(string_value("id"), number_value(id.0)?);
        }
        EffectTarget::Group(id) => {
            value.insert(string_value("type"), Value::String("group".to_string()));
            value.insert(string_value("id"), number_value(id.0)?);
        }
    }
    Ok(Value::Mapping(value))
}

fn write_source_reference(
    session: &ProjectSession,
    from_document: &Utf8Path,
    kind: SourceObjectKind,
    identity: &SourceIdentity,
) -> Result<String, ExportProjectError> {
    let alias = session
        .source
        .documents
        .get(from_document)
        .into_iter()
        .flat_map(|document| &document.imports)
        .find(|edge| {
            edge.targets
                .iter()
                .any(|target| target == identity.document())
        })
        .map(|edge| edge.alias.clone())
        .ok_or_else(|| ExportProjectError::InvalidReference {
            path: from_document.to_path_buf(),
            reference: identity.object().to_string(),
            message: format!(
                "no import alias makes the {kind:?} target visible from this document"
            ),
        })?;
    Ok(format!("{alias}.{}", identity.object()))
}

fn typed_object(object_type: &str) -> Mapping {
    let mut value = Mapping::new();
    value.insert(string_value("type"), Value::String(object_type.to_string()));
    value
}

fn string_value(value: &str) -> Value {
    Value::String(value.to_string())
}

fn number_value<T: serde::Serialize>(value: T) -> Result<Value, ExportProjectError> {
    yaml_serde::to_value(value).map_err(|source| ExportProjectError::Serialize {
        path: Utf8PathBuf::from("<sync>"),
        source,
    })
}

fn color_string(color: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", color.red, color.green, color.blue)
}

fn seconds_string(seconds: f64) -> String {
    format!("{seconds}s")
}

fn distance_meters(distance: Distance) -> f64 {
    distance.micrometers as f64 / 1_000_000.0
}

fn distance_span_meters(distance: DistanceSpan) -> f64 {
    distance.micrometers as f64 / 1_000_000.0
}

fn channel_order_name(order: &RgbChannelOrder) -> &'static str {
    match order {
        RgbChannelOrder::Rgb => "rgb",
        RgbChannelOrder::Rbg => "rbg",
        RgbChannelOrder::Grb => "grb",
        RgbChannelOrder::Gbr => "gbr",
        RgbChannelOrder::Brg => "brg",
        RgbChannelOrder::Bgr => "bgr",
    }
}

struct Loader {
    source_root: Utf8PathBuf,
    entrypoint: Utf8PathBuf,
    documents: IndexMap<Utf8PathBuf, SourceDocument>,
    visible_objects: IndexMap<Utf8PathBuf, IndexMap<AliasObjectKey, ResolvedObject>>,
    loading_documents: IndexSet<Utf8PathBuf>,
    definitions: ProjectDefinitionStores,
    referenced_assets: Vec<ReferencedAsset>,
    next_asset_id: u32,
    source_overrides: IndexMap<Utf8PathBuf, String>,
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
            visible_objects: IndexMap::new(),
            loading_documents: IndexSet::new(),
            definitions: ProjectDefinitionStores::default(),
            referenced_assets: Vec::new(),
            next_asset_id: 1,
            source_overrides: IndexMap::new(),
        })
    }

    fn load(mut self) -> Result<ProjectSession, LoadProjectError> {
        let entrypoint = self.entrypoint.clone();
        self.load_document(&entrypoint)?;
        let project = self.resolve_project(&entrypoint)?;
        let entrypoint = self.entrypoint.clone();
        Ok(ProjectSession {
            project,
            source: SourceProject {
                source_root: self.source_root,
                entrypoint,
                documents: self.documents,
                referenced_assets: self.referenced_assets,
            },
        })
    }

    fn load_document(&mut self, relative: &Utf8Path) -> Result<(), LoadProjectError> {
        if self.documents.contains_key(relative) {
            return Ok(());
        }
        if self.loading_documents.contains(relative) {
            // Import cycles are valid: each Dawn document indexes its local objects
            // before resolving imports, so the active document is already visible.
            return Ok(());
        }
        self.loading_documents.insert(relative.to_path_buf());
        let absolute = self.source_root.join(relative);
        let file_name = relative.file_name().unwrap_or_default();
        let result = if file_name.ends_with(".effect.dawn") {
            self.load_effect_document(relative, &absolute)
        } else if file_name.ends_with(".operator.dawn") {
            self.load_operator_document(relative, &absolute)
        } else {
            self.load_dawn_document(relative, &absolute)
        };
        self.loading_documents.shift_remove(relative);
        result
    }

    fn read_source(
        &self,
        relative: &Utf8Path,
        absolute: &Utf8Path,
    ) -> Result<String, LoadProjectError> {
        if let Some(source) = self.source_overrides.get(relative) {
            return Ok(source.clone());
        }
        fs::read_to_string(absolute).map_err(|source| LoadProjectError::Io {
            path: absolute.to_path_buf(),
            source,
        })
    }

    fn load_effect_document(
        &mut self,
        relative: &Utf8Path,
        absolute: &Utf8Path,
    ) -> Result<(), LoadProjectError> {
        let source = self.read_source(relative, absolute)?;
        let compiled =
            compile_effects(&source).map_err(|diagnostics| LoadProjectError::InvalidEffect {
                path: relative.to_path_buf(),
                diagnostics: diagnostics
                    .into_iter()
                    .map(|diagnostic| {
                        dsl_diagnostic(
                            relative,
                            &source,
                            diagnostic,
                            IoDiagnosticCode::EffectCompile,
                        )
                    })
                    .collect(),
            })?;
        let mut visible = IndexMap::new();
        let mut objects = Vec::new();
        for effect in compiled {
            let name = effect.name().as_str().to_string();
            let id = EffectDefinitionId(SourceIdentity::new(relative.to_path_buf(), name.clone()));
            self.definitions
                .effects
                .insert(id.clone(), EffectDefinition { compiled: effect });
            let source_object = SourceObjectId {
                kind: SourceObjectKind::EffectDefinition,
                id: name.clone(),
            };
            objects.push(source_object);
            visible.insert(
                AliasObjectKey {
                    alias: None,
                    object: name.clone(),
                },
                ResolvedObject::EffectDefinition(id),
            );
        }
        self.visible_objects.insert(relative.to_path_buf(), visible);
        let document =
            SourceDocument::new(Vec::new(), objects, SourceDocumentKind::Effect { source })
                .map_err(|message| LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    range: None,
                    message,
                })?;
        self.documents.insert(relative.to_path_buf(), document);
        Ok(())
    }

    fn load_operator_document(
        &mut self,
        relative: &Utf8Path,
        absolute: &Utf8Path,
    ) -> Result<(), LoadProjectError> {
        let source = self.read_source(relative, absolute)?;
        let compiled = compile_operators(&source).map_err(|diagnostics| {
            LoadProjectError::InvalidOperator {
                path: relative.to_path_buf(),
                diagnostics: diagnostics
                    .into_iter()
                    .map(|diagnostic| {
                        dsl_diagnostic(
                            relative,
                            &source,
                            diagnostic,
                            IoDiagnosticCode::OperatorCompile,
                        )
                    })
                    .collect(),
            }
        })?;
        let mut visible = IndexMap::new();
        let mut objects = Vec::new();
        for operator in compiled {
            let name = operator.name().as_str().to_string();
            let id =
                OperatorDefinitionId(SourceIdentity::new(relative.to_path_buf(), name.clone()));
            let definition = custom_operator_definition(id.clone(), operator);
            self.definitions.operators.insert(id.clone(), definition);
            let source_object = SourceObjectId {
                kind: SourceObjectKind::OperatorDefinition,
                id: name.clone(),
            };
            objects.push(source_object);
            visible.insert(
                AliasObjectKey {
                    alias: None,
                    object: name.clone(),
                },
                ResolvedObject::OperatorDefinition(id),
            );
        }
        self.visible_objects.insert(relative.to_path_buf(), visible);
        let document =
            SourceDocument::new(Vec::new(), objects, SourceDocumentKind::Operator { source })
                .map_err(|message| LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    range: None,
                    message,
                })?;
        self.documents.insert(relative.to_path_buf(), document);
        Ok(())
    }

    fn load_dawn_document(
        &mut self,
        relative: &Utf8Path,
        absolute: &Utf8Path,
    ) -> Result<(), LoadProjectError> {
        let text = self.read_source(relative, absolute)?;
        let value = parse_yaml_value(relative, &text)?;
        let map = mapping(&value).ok_or_else(|| LoadProjectError::InvalidDocument {
            path: relative.to_path_buf(),
            range: None,
            message: "document root must be a mapping".to_string(),
        })?;
        let imports = parse_imports(relative, map)?;
        let mut visible = IndexMap::new();

        let mut objects = Vec::new();
        for (key, object_value) in map {
            let Some(key) = key.as_str() else {
                return Err(LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    range: None,
                    message: "object keys must be strings".to_string(),
                });
            };
            if key == "imports" {
                continue;
            }
            Identifier::new(key.to_string()).map_err(|_| LoadProjectError::InvalidDocument {
                path: relative.to_path_buf(),
                range: source_range_for_value(relative, object_value),
                message: format!("invalid object identifier `{key}`"),
            })?;
            let object_type = string_field(relative, object_value, "type")?;
            let object = match object_type {
                "project" => ResolvedObject::Project(ProjectId(SourceIdentity::new(
                    relative.to_path_buf(),
                    key.to_string(),
                ))),
                "setup" => ResolvedObject::Setup(SetupId(SourceIdentity::new(
                    relative.to_path_buf(),
                    key.to_string(),
                ))),
                "controller" => ResolvedObject::Controller(ControllerId(SourceIdentity::new(
                    relative.to_path_buf(),
                    key.to_string(),
                ))),
                "layout" => ResolvedObject::Layout(LayoutId(SourceIdentity::new(
                    relative.to_path_buf(),
                    key.to_string(),
                ))),
                "patch" => ResolvedObject::Patch(PatchId(SourceIdentity::new(
                    relative.to_path_buf(),
                    key.to_string(),
                ))),
                "fixture" => ResolvedObject::FixtureDefinition(FixtureDefinitionId(
                    SourceIdentity::new(relative.to_path_buf(), key.to_string()),
                )),
                "curve" => ResolvedObject::Curve(CurveId(SourceIdentity::new(
                    relative.to_path_buf(),
                    key.to_string(),
                ))),
                "sequence" => ResolvedObject::Sequence(SequenceId(SourceIdentity::new(
                    relative.to_path_buf(),
                    key.to_string(),
                ))),
                other => {
                    return Err(LoadProjectError::InvalidDocument {
                        path: relative.to_path_buf(),
                        range: source_range_for_field_value(relative, object_value, "type"),
                        message: format!("unsupported object type `{other}`"),
                    });
                }
            };
            if let ResolvedObject::Curve(id) = &object {
                self.definitions.curves.insert(
                    id.clone(),
                    CurveDefinition {
                        curve: parse_curve(relative, object_value)?,
                    },
                );
            }
            if let ResolvedObject::FixtureDefinition(id) = &object {
                self.definitions.fixtures.insert(
                    id.clone(),
                    parse_fixture_definition(relative, object_value)?,
                );
            }
            let source_object = SourceObjectId {
                kind: object.source_kind(),
                id: key.to_string(),
            };
            objects.push(source_object);
            visible.insert(
                AliasObjectKey {
                    alias: None,
                    object: key.to_string(),
                },
                object,
            );
        }

        self.visible_objects.insert(relative.to_path_buf(), visible);

        let mut import_edges = Vec::new();
        let mut imported_visible = Vec::new();
        let mut import_aliases = IndexSet::new();
        let mut imported_targets = IndexSet::new();
        for import in &imports {
            if !import_aliases.insert(import.alias.clone()) {
                return Err(LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    range: None,
                    message: format!("duplicate import alias `{}`", import.alias),
                });
            }
            let targets = self.resolve_import(relative, &import.from)?;
            let mut names = IndexSet::new();
            for target in &targets {
                if !imported_targets.insert(target.clone()) {
                    return Err(LoadProjectError::InvalidDocument {
                        path: relative.to_path_buf(),
                        range: None,
                        message: format!(
                            "document `{target}` is imported more than once; each target must have one alias"
                        ),
                    });
                }
                self.load_document(target)?;
                let target_visible = self.visible_objects.get(target).ok_or_else(|| {
                    LoadProjectError::InvalidDocument {
                        path: target.clone(),
                        range: None,
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
                            range: None,
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

        let document = SourceDocument::new(
            import_edges,
            objects,
            SourceDocumentKind::Dawn {
                original_value: value,
            },
        )
        .map_err(|message| LoadProjectError::InvalidDocument {
            path: relative.to_path_buf(),
            range: None,
            message,
        })?;
        self.documents.insert(relative.to_path_buf(), document);
        Ok(())
    }

    fn resolve_import(
        &self,
        importer: &Utf8Path,
        import_from: &Utf8Path,
    ) -> Result<Vec<Utf8PathBuf>, LoadProjectError> {
        if import_from.is_absolute() {
            return Err(LoadProjectError::InvalidDocument {
                path: importer.to_path_buf(),
                range: None,
                message: "import paths must be relative".to_string(),
            });
        }
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
                        range: None,
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
            range: None,
            message: format!("import target does not exist: {import_from}"),
        })
    }

    fn resolve_project(&mut self, entrypoint: &Utf8Path) -> Result<DawnProject, LoadProjectError> {
        let root_object = self.single_project_object(entrypoint)?;
        let root_id = ProjectId(SourceIdentity::new(
            entrypoint.to_path_buf(),
            Identifier::new(root_object.key.clone())
                .map_err(|_| LoadProjectError::InvalidDocument {
                    path: entrypoint.to_path_buf(),
                    range: None,
                    message: "project object key is not a valid identifier".to_string(),
                })?
                .as_str()
                .to_string(),
        ));
        let setup = self.reference_as_setup(
            entrypoint,
            string_field(entrypoint, root_object.value, "setup")?,
        )?;
        let sequences = sequence_field(entrypoint, root_object.value, "sequences")?
            .iter()
            .map(|reference| self.reference_as_sequence(entrypoint, reference))
            .collect::<Result<Vec<_>, _>>()?;

        let mut project = DawnProject {
            root: ProjectRoot {
                id: root_id,
                setup: setup.clone(),
                sequences: sequences.clone(),
            },
            setups: IndexMap::new(),
            layouts: IndexMap::new(),
            patches: IndexMap::new(),
            controllers: IndexMap::new(),
            sequences: IndexMap::new(),
            definitions: ProjectDefinitionStores::default(),
        };
        project.definitions = self.definitions.clone();

        let mut resolver = DomainResolver {
            loader: self,
            project: &mut project,
        };
        resolver.resolve_setup(entrypoint, &setup)?;
        for sequence in sequences {
            resolver.resolve_sequence(entrypoint, &sequence)?;
        }
        Ok(project)
    }

    fn single_project_object<'a>(
        &'a self,
        path: &Utf8Path,
    ) -> Result<SourceObjectValue<'a>, LoadProjectError> {
        let document = self.dawn_document(path)?;
        let mut found = None;
        let document_map = mapping(document).ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: None,
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
            range: None,
            message: "entrypoint must contain a project object".to_string(),
        })
    }

    fn object_value(
        &self,
        id: &ResolvedObject,
    ) -> Result<(Utf8PathBuf, String, Value), LoadProjectError> {
        let identity = id.source_identity();
        if !self
            .documents
            .get(identity.document())
            .is_some_and(|document| {
                document
                    .objects
                    .iter()
                    .any(|object| object.kind == id.source_kind() && object.id == identity.object())
            })
        {
            return Err(LoadProjectError::InvalidReference {
                path: self.entrypoint.clone(),
                range: None,
                reference: id.id_string(),
            });
        }
        let document = self.dawn_document(identity.document())?;
        let document_map = mapping(document).ok_or_else(|| LoadProjectError::InvalidDocument {
            path: identity.document().to_path_buf(),
            range: None,
            message: "document root must be a mapping".to_string(),
        })?;
        let value = document_map
            .get(Value::String(identity.object().to_string()))
            .ok_or_else(|| LoadProjectError::InvalidReference {
                path: identity.document().to_path_buf(),
                range: None,
                reference: identity.object().to_string(),
            })?
            .clone();
        Ok((
            identity.document().to_path_buf(),
            identity.object().to_string(),
            value,
        ))
    }

    fn dawn_document<'a>(&'a self, path: &Utf8Path) -> Result<&'a Value, LoadProjectError> {
        let document =
            self.documents
                .get(path)
                .ok_or_else(|| LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: None,
                    message: "document not loaded".to_string(),
                })?;
        match &document.kind {
            SourceDocumentKind::Dawn { original_value } => Ok(original_value),
            SourceDocumentKind::Effect { .. } => Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: "expected YAML Dawn document".to_string(),
            }),
            SourceDocumentKind::Operator { .. } => Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: "expected YAML Dawn document".to_string(),
            }),
        }
    }

    fn resolve_reference(
        &self,
        path: &Utf8Path,
        reference: &str,
    ) -> Result<ResolvedObject, LoadProjectError> {
        let range = source_range_for_scalar(path, reference);
        let (alias, object) =
            reference
                .split_once('.')
                .ok_or_else(|| LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    range: range.clone(),
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
                range,
                reference: reference.to_string(),
            })
    }

    fn reference_as_setup(
        &self,
        path: &Utf8Path,
        reference: &str,
    ) -> Result<SetupId, LoadProjectError> {
        match self.resolve_reference(path, reference)? {
            ResolvedObject::Setup(id) => Ok(id),
            _ => Err(LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range: source_range_for_scalar(path, reference),
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
                range: source_range_for_scalar(path, reference),
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
    fn resolve_setup(&mut self, path: &Utf8Path, id: &SetupId) -> Result<(), LoadProjectError> {
        if self.project.setups.contains_key(id) {
            return Ok(());
        }
        let (document_path, _, value) = self
            .loader
            .object_value(&ResolvedObject::Setup(id.clone()))?;
        let layout_ref = string_field(&document_path, &value, "layout")?;
        let patch_ref = string_field(&document_path, &value, "patch")?;
        let layout = match self.loader.resolve_reference(&document_path, layout_ref)? {
            ResolvedObject::Layout(layout) => layout,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: document_path,
                    range: None,
                    reference: layout_ref.to_string(),
                });
            }
        };
        let patch = match self.loader.resolve_reference(&document_path, patch_ref)? {
            ResolvedObject::Patch(patch) => patch,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    range: None,
                    reference: patch_ref.to_string(),
                });
            }
        };
        let controllers = sequence_field(&document_path, &value, "controllers")?
            .iter()
            .map(|name| {
                Identifier::new(name.to_string())
                    .map(|name| {
                        ControllerId(SourceIdentity::new(
                            document_path.clone(),
                            name.as_str().to_string(),
                        ))
                    })
                    .map_err(|_| LoadProjectError::InvalidReference {
                        path: document_path.clone(),
                        range: source_range_for_scalar(&document_path, name),
                        reference: name.to_string(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.project.setups.insert(
            id.clone(),
            Setup {
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
                    range: None,
                    message: format!("unsupported controller protocol `{other}`"),
                });
            }
        };
        let address = optional_string_field(&value, "destination")
            .map(parse_controller_address)
            .transpose()
            .map_err(|message| LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message,
            })?;
        let output = required_mapping(path, &value, "output")?;
        let output_value = Value::Mapping(output);
        let output_type = string_field(path, &output_value, "type")?;
        let channel_order =
            parse_channel_order(string_field(path, &output_value, "channel_order")?).ok_or_else(
                || LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: None,
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
                            range: None,
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
                    range: None,
                    message: format!("unsupported controller output type `{other}`"),
                });
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
                    range: None,
                    reference: fixture_ref.to_string(),
                });
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
        if !self
            .project
            .definitions
            .fixtures
            .definitions
            .contains_key(id)
        {
            return Err(LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range: None,
                reference: id.0.object().to_string(),
            });
        }
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
        let controller = match self.loader.resolve_reference(path, controller_ref)? {
            ResolvedObject::Controller(controller) => controller,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    range: None,
                    reference: controller_ref.to_string(),
                });
            }
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
                range: None,
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
                range: None,
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
        let duration =
            parse_duration(string_field(&document_path, &value, "duration")?).map_err(|error| {
                with_yaml_location(
                    error,
                    &document_path,
                    source_range_for_field_value(&document_path, &value, "duration"),
                )
            })?;
        let audio = self.parse_audio(&document_path, &value)?;
        let mark_collections = optional_sequence(&value, "mark_collections")
            .unwrap_or_default()
            .iter()
            .map(|collection| parse_mark_collection(&document_path, collection))
            .collect::<Result<Vec<_>, _>>()?;
        let layers = sequence_values(&document_path, &value, "layers")?
            .iter()
            .map(|layer| parse_sequence_layer(&document_path, layer))
            .collect::<Result<Vec<_>, _>>()?;
        let effects = sequence_values(&document_path, &value, "effects")?
            .iter()
            .map(|effect| self.parse_sequence_effect(&document_path, effect))
            .collect::<Result<Vec<_>, _>>()?;
        let composition_graph = self.parse_composition_graph(
            &document_path,
            required_field(&document_path, &value, "composition_graph")?,
        )?;
        let automation_clips = optional_sequence(&value, "automation_clips")
            .unwrap_or_default()
            .iter()
            .map(|clip| self.parse_automation_clip(&document_path, clip))
            .collect::<Result<Vec<_>, _>>()?;
        self.project.sequences.insert(
            id.clone(),
            Sequence {
                id: id.clone(),
                duration,
                frame_rate: u32_field(&document_path, &value, "frame_rate")?,
                audio,
                mark_collections,
                layers,
                effects,
                composition_graph,
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
                range: None,
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
                range: None,
                message: format!("audio asset does not exist: {audio_path}"),
            });
        }
        let relative = match relative_path(&self.loader.source_root, &absolute) {
            Ok(relative) => relative,
            Err(_) if !Utf8Path::new(audio_path).is_absolute() => Utf8PathBuf::from(audio_path),
            Err(error) => return Err(error),
        };
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

    fn parse_sequence_effect(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<EffectInst, LoadProjectError> {
        let effect = self.parse_effect_clip(path, value)?;
        Ok(EffectInst {
            id: EffectInstId(u32_field(path, value, "id")?),
            layer_id: SequenceLayerId(u32_field(path, value, "layer_id")?),
            start: parse_duration_as_time(string_field(path, value, "start")?).map_err(
                |error| {
                    with_yaml_location(
                        error,
                        path,
                        source_range_for_field_value(path, value, "start"),
                    )
                },
            )?,
            duration: parse_duration(string_field(path, value, "duration")?).map_err(|error| {
                with_yaml_location(
                    error,
                    path,
                    source_range_for_field_value(path, value, "duration"),
                )
            })?,
            target: parse_effect_target(path, required_field(path, value, "target")?)?,
            scope: parse_effect_scope(path, value)?,
            definition: effect.definition,
            param_overrides: effect.param_overrides,
        })
    }

    fn parse_composition_graph(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<SequenceCompositionGraph, LoadProjectError> {
        let graph = SequenceCompositionGraph {
            nodes: sequence_values(path, value, "nodes")?
                .iter()
                .map(|node| self.parse_composition_graph_node(path, node))
                .collect::<Result<Vec<_>, _>>()?,
            edges: sequence_values(path, value, "edges")?
                .iter()
                .map(|edge| parse_graph_edge(path, edge))
                .collect::<Result<Vec<_>, _>>()?,
        };
        validate_composition_graph(&graph, &self.project.definitions.operators).map_err(
            |error| LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: error.message,
            },
        )?;
        Ok(graph)
    }

    fn parse_composition_graph_node(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<CompositionGraphNode, LoadProjectError> {
        let kind = match string_field(path, value, "type")? {
            "layer" => CompositionGraphNodeKind::Layer {
                layer_id: SequenceLayerId(u32_field(path, value, "layer_id")?),
            },
            "operator" => CompositionGraphNodeKind::Operator(GraphOperatorNode {
                operator: self.parse_graph_operator_ref(path, value)?,
                params: self.parse_graph_operator_params(path, value)?,
            }),
            "output" => CompositionGraphNodeKind::Output,
            other => {
                return Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_field_value(path, value, "type"),
                    message: format!("unsupported composition graph node type `{other}`"),
                });
            }
        };
        Ok(CompositionGraphNode {
            id: CompositionGraphNodeId(u32_field(path, value, "id")?),
            position: parse_graph_position(path, required_field(path, value, "position")?)?,
            kind,
        })
    }

    fn parse_effect_clip(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<EffectClip, LoadProjectError> {
        let script_ref = string_field(path, value, "script")?;
        let definition = match self.loader.resolve_reference(path, script_ref)? {
            ResolvedObject::EffectDefinition(definition) => definition,
            _ => {
                return Err(LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    range: None,
                    reference: script_ref.to_string(),
                });
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
                                    range: None,
                                    message: "effect param keys must be strings".to_string(),
                                })?;
                        let identifier = Identifier::new(key.to_string()).map_err(|_| {
                            LoadProjectError::InvalidDocument {
                                path: path.to_path_buf(),
                                range: None,
                                message: format!("invalid effect param name `{key}`"),
                            }
                        })?;
                        Ok((identifier, self.parse_effect_param(path, value)?))
                    })
                    .collect::<Result<IndexMap<_, _>, LoadProjectError>>()
            })
            .transpose()?
            .unwrap_or_default();
        Ok(EffectClip {
            definition,
            param_overrides: params,
        })
    }

    fn parse_graph_operator_params(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<IndexMap<Identifier, EffectParamValue>, LoadProjectError> {
        optional_mapping(value, "params")
            .map(|mapping| {
                mapping
                    .iter()
                    .map(|(key, value)| {
                        let key =
                            key.as_str()
                                .ok_or_else(|| LoadProjectError::InvalidDocument {
                                    path: path.to_path_buf(),
                                    range: None,
                                    message: "operator param keys must be strings".to_string(),
                                })?;
                        let identifier = Identifier::new(key.to_string()).map_err(|_| {
                            LoadProjectError::InvalidDocument {
                                path: path.to_path_buf(),
                                range: None,
                                message: format!("invalid operator param name `{key}`"),
                            }
                        })?;
                        Ok((identifier, self.parse_effect_param(path, value)?))
                    })
                    .collect::<Result<IndexMap<_, _>, LoadProjectError>>()
            })
            .transpose()
            .map(Option::unwrap_or_default)
    }

    fn parse_graph_operator_ref(
        &self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<OperatorRef, LoadProjectError> {
        let name = string_field(path, value, "operator")?;
        if let Some(builtin) = BuiltinOperator::from_source_name(name) {
            return Ok(OperatorRef::Builtin(builtin));
        }
        match self.loader.resolve_reference(path, name)? {
            ResolvedObject::OperatorDefinition(id) => Ok(OperatorRef::Custom(id)),
            _ => Err(LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range: source_range_for_field_value(path, value, "operator"),
                reference: name.to_string(),
            }),
        }
    }

    fn resolve_effect_definition(
        &mut self,
        id: &EffectDefinitionId,
    ) -> Result<(), LoadProjectError> {
        if !self
            .project
            .definitions
            .effects
            .definitions
            .contains_key(id)
        {
            return Err(LoadProjectError::InvalidReference {
                path: self.loader.entrypoint.clone(),
                range: None,
                reference: id.0.object().to_string(),
            });
        }
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
            "color" => Ok(EffectParamValue::Color(
                parse_color(string_field(path, value, "value")?).map_err(|error| {
                    with_yaml_location(
                        error,
                        path,
                        source_range_for_field_value(path, value, "value"),
                    )
                })?,
            )),
            "enum" => Ok(EffectParamValue::Enum(
                Identifier::new(string_field(path, value, "value")?.to_string()).map_err(|_| {
                    LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: None,
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
                range: None,
                message: format!("unsupported effect param type `{other}`"),
            }),
        }
    }

    fn parse_array_item(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<EffectParamValue, LoadProjectError> {
        if optional_field(value, "type").is_some() {
            return self.parse_effect_param(path, value);
        }
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
                        range: None,
                        reference: reference.to_string(),
                    });
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
        if !self.project.definitions.curves.definitions.contains_key(id) {
            return Err(LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range: None,
                reference: id.0.object().to_string(),
            });
        }
        Ok(())
    }

    fn parse_automation_clip(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<AutomationClip, LoadProjectError> {
        let bindings = sequence_values(path, value, "bindings")?
            .iter()
            .map(|binding| parse_automation_binding(path, binding))
            .collect::<Result<Vec<_>, _>>()?;
        let mut seen = IndexSet::new();
        for binding in &bindings {
            if !seen.insert(binding.target.clone()) {
                return Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_field_value(path, value, "bindings"),
                    message: "automation clip has duplicate bindings for a parameter".to_string(),
                });
            }
        }
        Ok(AutomationClip {
            id: AutomationClipId(u32_field(path, value, "id")?),
            start: parse_duration_as_time(string_field(path, value, "start")?).map_err(
                |error| {
                    with_yaml_location(
                        error,
                        path,
                        source_range_for_field_value(path, value, "start"),
                    )
                },
            )?,
            duration: parse_duration(string_field(path, value, "duration")?).map_err(|error| {
                with_yaml_location(
                    error,
                    path,
                    source_range_for_field_value(path, value, "duration"),
                )
            })?,
            anchor_lane_index: u32_field(path, value, "anchor_lane_index")?,
            lane_index: u32_field(path, value, "lane_index")?,
            curve: parse_automation_curve(path, required_field(path, value, "curve")?)?,
            bindings,
        })
    }
}

fn parse_automation_curve(path: &Utf8Path, value: &Value) -> Result<Curve, LoadProjectError> {
    let curve = parse_curve(path, value)?;
    if curve
        .points
        .iter()
        .any(|point| !matches!(point.value, CurveValue::Float(_)))
    {
        return Err(LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_value(path, value),
            message: "automation curves must be float curves".to_string(),
        });
    }
    Ok(curve)
}

fn parse_sequence_layer(path: &Utf8Path, value: &Value) -> Result<SequenceLayer, LoadProjectError> {
    Ok(SequenceLayer {
        id: SequenceLayerId(u32_field(path, value, "id")?),
        name: string_field(path, value, "name")?.to_string(),
        color: parse_color(string_field(path, value, "color")?).map_err(|error| {
            with_yaml_location(
                error,
                path,
                source_range_for_field_value(path, value, "color"),
            )
        })?,
        enabled: optional_field(value, "enabled")
            .map(|enabled| {
                enabled
                    .as_bool()
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: source_range_for_field_value(path, value, "enabled"),
                        message: "layer enabled must be a bool".to_string(),
                    })
            })
            .transpose()?
            .unwrap_or(true),
    })
}

fn parse_automation_binding(
    path: &Utf8Path,
    value: &Value,
) -> Result<AutomationBinding, LoadProjectError> {
    let target = parse_automation_target(path, required_field(path, value, "target")?)?;
    Ok(AutomationBinding {
        target,
        mapping: parse_automation_mapping(path, required_field(path, value, "mapping")?)?,
    })
}

fn parse_automation_target(
    path: &Utf8Path,
    value: &Value,
) -> Result<AutomationTarget, LoadProjectError> {
    Ok(match string_field(path, value, "type")? {
        "effect_param" => AutomationTarget::EffectParam {
            effect_id: EffectInstId(u32_field(path, value, "effect_id")?),
            param: parse_identifier_field(path, value, "param")?,
        },
        "composition_node_param" => AutomationTarget::CompositionNodeParam {
            node_id: CompositionGraphNodeId(u32_field(path, value, "node_id")?),
            param: parse_identifier_field(path, value, "param")?,
        },
        other => {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: source_range_for_field_value(path, value, "type"),
                message: format!("unsupported automation target `{other}`"),
            });
        }
    })
}

fn parse_automation_mapping(
    path: &Utf8Path,
    value: &Value,
) -> Result<AutomationMapping, LoadProjectError> {
    Ok(match string_field(path, value, "type")? {
        "float" => AutomationMapping::Float {
            min: f64_field(path, value, "min")?,
            max: f64_field(path, value, "max")?,
        },
        "int" => AutomationMapping::Int {
            min: i64_field(path, value, "min")?,
            max: i64_field(path, value, "max")?,
        },
        "bool" => AutomationMapping::Bool,
        "enum" => AutomationMapping::Enum {
            values: sequence_field(path, value, "values")?
                .into_iter()
                .map(|enum_value| {
                    Identifier::new(enum_value).map_err(|_| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: source_range_for_field_value(path, value, "values"),
                        message: "enum automation values must be identifiers".to_string(),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        },
        "float_curve" => AutomationMapping::FloatCurve {
            min: f64_field(path, value, "min")?,
            max: f64_field(path, value, "max")?,
        },
        other => {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: source_range_for_field_value(path, value, "type"),
                message: format!("unsupported automation mapping `{other}`"),
            });
        }
    })
}

fn parse_identifier_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<Identifier, LoadProjectError> {
    let raw = string_field(path, value, key)?;
    Identifier::new(raw.to_string()).map_err(|_| LoadProjectError::InvalidDocument {
        path: path.to_path_buf(),
        range: source_range_for_field_value(path, value, key),
        message: format!("invalid identifier `{raw}`"),
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct AliasObjectKey {
    alias: Option<String>,
    object: String,
}

#[derive(Clone, Debug, PartialEq)]
enum ResolvedObject {
    Project(ProjectId),
    Setup(SetupId),
    Controller(ControllerId),
    Layout(LayoutId),
    Patch(PatchId),
    FixtureDefinition(FixtureDefinitionId),
    Curve(CurveId),
    Sequence(SequenceId),
    EffectDefinition(EffectDefinitionId),
    OperatorDefinition(OperatorDefinitionId),
}

impl ResolvedObject {
    fn source_identity(&self) -> &SourceIdentity {
        match self {
            Self::Project(id) => &id.0,
            Self::Setup(id) => &id.0,
            Self::Controller(id) => &id.0,
            Self::Layout(id) => &id.0,
            Self::Patch(id) => &id.0,
            Self::FixtureDefinition(id) => &id.0,
            Self::Curve(id) => &id.0,
            Self::Sequence(id) => &id.0,
            Self::EffectDefinition(id) => &id.0,
            Self::OperatorDefinition(id) => &id.0,
        }
    }

    fn source_kind(&self) -> SourceObjectKind {
        match self {
            Self::Project(_) => SourceObjectKind::Project,
            Self::Setup(_) => SourceObjectKind::Setup,
            Self::Controller(_) => SourceObjectKind::Controller,
            Self::Layout(_) => SourceObjectKind::Layout,
            Self::Patch(_) => SourceObjectKind::Patch,
            Self::FixtureDefinition(_) => SourceObjectKind::FixtureDefinition,
            Self::Curve(_) => SourceObjectKind::Curve,
            Self::Sequence(_) => SourceObjectKind::Sequence,
            Self::EffectDefinition(_) => SourceObjectKind::EffectDefinition,
            Self::OperatorDefinition(_) => SourceObjectKind::OperatorDefinition,
        }
    }

    fn id_string(&self) -> String {
        match self {
            Self::Project(id) => id.0.object().to_string(),
            Self::Setup(id) => id.0.object().to_string(),
            Self::Controller(id) => id.0.object().to_string(),
            Self::Layout(id) => id.0.object().to_string(),
            Self::Patch(id) => id.0.object().to_string(),
            Self::FixtureDefinition(id) => id.0.object().to_string(),
            Self::Curve(id) => id.0.object().to_string(),
            Self::Sequence(id) => id.0.object().to_string(),
            Self::EffectDefinition(id) => id.0.object().to_string(),
            Self::OperatorDefinition(id) => id.0.object().to_string(),
        }
    }
}

struct SourceObjectValue<'a> {
    key: String,
    value: &'a Value,
}

#[derive(Clone, Debug)]
struct ParsedImport {
    from: Utf8PathBuf,
    alias: String,
}

fn parse_imports(path: &Utf8Path, map: &Mapping) -> Result<Vec<ParsedImport>, LoadProjectError> {
    let Some(imports) = map.get(Value::String("imports".to_string())) else {
        return Ok(Vec::new());
    };
    let imports = imports
        .as_sequence()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: None,
            message: "imports must be a sequence".to_string(),
        })?;
    imports
        .iter()
        .map(|import| {
            Ok(ParsedImport {
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
            range: source_range_for_field_value(path, value, "type"),
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
                        range: None,
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
        display_color: parse_color(string_field(path, value, "color")?).map_err(|error| {
            with_yaml_location(
                error,
                path,
                source_range_for_field_value(path, value, "color"),
            )
        })?,
        marks: sequence_values(path, value, "marks")?
            .iter()
            .map(|mark| {
                mark.as_str()
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: None,
                        message: "marks must be duration strings".to_string(),
                    })
                    .and_then(|duration| {
                        parse_duration_as_time(duration).map_err(|error| {
                            with_yaml_location(error, path, source_range_for_value(path, mark))
                        })
                    })
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
            range: source_range_for_field_value(path, value, "type"),
            message: format!("invalid effect target type `{other}`"),
        }),
    }
}

fn parse_effect_scope(path: &Utf8Path, value: &Value) -> Result<EffectScope, LoadProjectError> {
    match string_field(path, value, "scope")? {
        "per_fixture" => Ok(EffectScope::PerFixture),
        "whole_target" => Ok(EffectScope::WholeTarget),
        other => Err(LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, "scope"),
            message: format!("invalid effect scope `{other}`"),
        }),
    }
}

fn parse_graph_position(
    path: &Utf8Path,
    value: &Value,
) -> Result<GraphNodePosition, LoadProjectError> {
    Ok(GraphNodePosition {
        x: f64_field(path, value, "x")?,
        y: f64_field(path, value, "y")?,
    })
}

fn parse_graph_edge(path: &Utf8Path, value: &Value) -> Result<EffectGraphEdge, LoadProjectError> {
    Ok(EffectGraphEdge {
        from: CompositionGraphNodeId(u32_field(path, value, "from")?),
        from_port: GraphPortId(string_field(path, value, "from_port")?.to_string()),
        to: CompositionGraphNodeId(u32_field(path, value, "to")?),
        to_port: GraphPortId(string_field(path, value, "to_port")?.to_string()),
    })
}

fn parse_fixture_definition(
    path: &Utf8Path,
    value: &Value,
) -> Result<FixtureDefinition, LoadProjectError> {
    let bulb_diameter = f64_field(path, value, "bulb_diameter")?;
    let geometry_value = required_field(path, value, "geometry")?;
    let geometry = match string_field(path, geometry_value, "type")? {
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
                range: source_range_for_field_value(path, geometry_value, "type"),
                message: format!("unsupported fixture geometry `{other}`"),
            });
        }
    };
    Ok(FixtureDefinition {
        bulb_radius: distance_span(bulb_diameter / 2.0),
        geometry,
    })
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
                        range: None,
                        message: "curve float point must be numeric".to_string(),
                    }
                })?),
                "color" => CurveValue::Color(
                    parse_color(value.as_str().ok_or_else(|| {
                        LoadProjectError::InvalidDocument {
                            path: path.to_path_buf(),
                            range: None,
                            message: "curve color point must be a color string".to_string(),
                        }
                    })?)
                    .map_err(|error| {
                        with_yaml_location(error, path, source_range_for_value(path, value))
                    })?,
                ),
                other => {
                    return Err(LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: source_range_for_field_value(path, value, "value_type"),
                        message: format!("unsupported curve value type `{other}`"),
                    });
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
            range: None,
            message: format!("duration must end in `s`: {value}"),
        })?;
    seconds
        .parse()
        .map_err(|_| LoadProjectError::InvalidDocument {
            path: Utf8PathBuf::from("<duration>"),
            range: None,
            message: format!("invalid duration: {value}"),
        })
}

fn parse_color(value: &str) -> Result<Color, LoadProjectError> {
    if value.len() != 7 || !value.starts_with('#') {
        return Err(LoadProjectError::InvalidDocument {
            path: Utf8PathBuf::from("<color>"),
            range: None,
            message: format!("invalid color: {value}"),
        });
    }
    Ok(Color {
        red: u8::from_str_radix(&value[1..3], 16).map_err(|_| {
            LoadProjectError::InvalidDocument {
                path: Utf8PathBuf::from("<color>"),
                range: None,
                message: format!("invalid color: {value}"),
            }
        })?,
        green: u8::from_str_radix(&value[3..5], 16).map_err(|_| {
            LoadProjectError::InvalidDocument {
                path: Utf8PathBuf::from("<color>"),
                range: None,
                message: format!("invalid color: {value}"),
            }
        })?,
        blue: u8::from_str_radix(&value[5..7], 16).map_err(|_| {
            LoadProjectError::InvalidDocument {
                path: Utf8PathBuf::from("<color>"),
                range: None,
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
            range: source_range_for_value(path, value),
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
        range: source_range_for_value(path, value),
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
            range: source_range_for_field_value(path, value, key),
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
                    range: source_range_for_value(path, value),
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
            range: source_range_for_field_value(path, value, key),
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
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a u32"),
        })
}

fn usize_field(path: &Utf8Path, value: &Value, key: &str) -> Result<usize, LoadProjectError> {
    required_field(path, value, key)?
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a usize"),
        })
}

fn i64_field(path: &Utf8Path, value: &Value, key: &str) -> Result<i64, LoadProjectError> {
    required_field(path, value, key)?
        .as_i64()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be an integer"),
        })
}

fn f64_field(path: &Utf8Path, value: &Value, key: &str) -> Result<f64, LoadProjectError> {
    required_field(path, value, key)?
        .as_f64()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a number"),
        })
}

fn bool_field(path: &Utf8Path, value: &Value, key: &str) -> Result<bool, LoadProjectError> {
    required_field(path, value, key)?
        .as_bool()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a bool"),
        })
}

fn relative_path(root: &Utf8Path, path: &Utf8Path) -> Result<Utf8PathBuf, LoadProjectError> {
    path.strip_prefix(root)
        .map(Utf8Path::to_path_buf)
        .map_err(|_| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: None,
            message: format!("path is outside source root {root}"),
        })
}

fn normalize_relative(path: Utf8PathBuf) -> Utf8PathBuf {
    let original = path.clone();
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            camino::Utf8Component::CurDir => {}
            camino::Utf8Component::ParentDir => {
                if parts.last().is_some_and(|part| part != "..") {
                    let _ = parts.pop();
                } else {
                    parts.push("..".to_string());
                }
            }
            camino::Utf8Component::Normal(part) => parts.push(part.to_string()),
            camino::Utf8Component::RootDir | camino::Utf8Component::Prefix(_) => {
                return original;
            }
        }
    }
    parts.into_iter().collect()
}
