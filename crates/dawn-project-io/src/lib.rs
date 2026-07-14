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
use dawn_language::dsl::Identifier;
use dawn_language::identity::SourceIdentity;
use dawn_language::sequence::{
    CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge,
    GraphNodePosition, GraphPortId, MarkCollection, MarkCollectionKey, Sequence, SequenceAudio,
    SequenceCompositionGraph, SequenceId, SequenceLayer, SequenceLayerId,
};
use dawn_language::values::{Color, DawnDuration};
use indexmap::{IndexMap, IndexSet};
use marked_yaml::Node;
use std::cell::RefCell;
use std::fmt;
use std::fs;
use std::io;
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

pub fn save_project(session: &ProjectSession) -> Result<SaveReport, ExportProjectError> {
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
    let document = session
        .source
        .documents
        .get_mut(from_document)
        .ok_or_else(|| ExportProjectError::InvalidReference {
            path: from_document.to_path_buf(),
            reference: reference.to_string(),
            message: "source document is missing from the source project".to_string(),
        })?;
    if document
        .imports
        .iter()
        .any(|edge| edge.targets.contains(&target_document))
    {
        return Ok(());
    }
    let alias = available_import_alias(document, alias_base).ok_or_else(|| {
        ExportProjectError::InvalidReference {
            path: from_document.to_path_buf(),
            reference: reference.to_string(),
            message: format!("no import alias remains for `{alias_base}`"),
        }
    })?;
    let import_from = relative_path_from_document(from_document, &target_document);
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
        control_clips: Vec::new(),
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
        SourceObjectKind::Gradient => Some("gradients"),
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

mod diagnostics;
use diagnostics::{
    check_absolute_document, discover_reachable_files, effect_diagnostics, load_error_diagnostic,
    node_range, operator_diagnostics, parse_yaml_value, push_diagnostic,
    push_load_error_diagnostics,
};

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

mod serialization;
use serialization::{document_text, write_source_documents};

mod loader;
use loader::{Loader, normalize_relative};
