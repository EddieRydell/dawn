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

use camino::Utf8Component;
use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::effect::{
    CurveDefinition, CurveId, CurveSource, EffectDefinition, EffectDefinitionId, EffectInst,
    EffectInstId, EffectParamValue, EffectScope, EffectTarget,
};
use dawn_language::effect_dsl::{Diagnostic as EffectDiagnostic, Identifier, compile_effects};
use dawn_language::model::{DawnProject, ProjectDefinitionStores, ProjectId, ProjectRoot};
use dawn_language::sequence::{
    AssetId, AutomationBinding, AutomationClip, AutomationClipId, AutomationMapping,
    AutomationTarget, EffectClip, EffectGraphClip, EffectGraphEdge, EffectGraphNode,
    EffectGraphNodeId, EffectGraphNodeKind, GraphNodePosition, GraphOperator, GraphOperatorNode,
    GraphPortId, GraphSourceNode, MarkCollection, MarkCollectionKey, Sequence, SequenceAudio,
    SequenceClip, SequenceClipId, SequenceClipKind, SequenceId,
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
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::time::Duration;
use yaml_serde::{Mapping, Value};

thread_local! {
    static YAML_SOURCE_INDICES: RefCell<IndexMap<Utf8PathBuf, YamlSourceIndex>> =
        RefCell::new(IndexMap::new());
}

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
    IoRead,
    YamlParse,
}

impl IoDiagnosticCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DawnLoad => "dawn.load",
            Self::DawnReference => "dawn.reference",
            Self::EffectCompile => "effect.compile",
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

    fn range_for_value(&self, value: &Value) -> Option<TextRange> {
        self.entries
            .iter()
            .find(|entry| &entry.value == value)
            .and_then(|entry| entry.range.clone())
    }

    fn range_for_field_value(&self, parent: &Value, key: &str) -> Option<TextRange> {
        let parent_path = self
            .entries
            .iter()
            .find(|entry| &entry.value == parent)
            .map(|entry| entry.path.clone())?;
        let mut field_path = parent_path;
        field_path.push(YamlPathSegment::Key(key.to_string()));
        self.entries
            .iter()
            .find(|entry| entry.path == field_path)
            .and_then(|entry| entry.range.clone())
    }

    fn range_for_scalar(&self, value: &str) -> Option<TextRange> {
        self.entries
            .iter()
            .find(|entry| entry.value.as_str() == Some(value))
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

    let mut synced = session.clone();
    sync_project_source(&mut synced)?;
    let written_files = write_source_documents(&synced, output_root)?;

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
    let mut synced = session.clone();
    sync_project_source(&mut synced)?;
    let written_files = write_source_documents(&synced, &synced.source.source_root)?;
    Ok(SaveReport { written_files })
}

pub fn source_document_text(
    session: &ProjectSession,
    relative_path: &Utf8Path,
) -> Result<Option<String>, ExportProjectError> {
    let mut synced = session.clone();
    sync_project_source(&mut synced)?;
    let Some(document) = synced.source.documents.get(relative_path) else {
        return Ok(None);
    };
    document_text(document).map(Some)
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
    Setup,
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
    if relative
        .file_name()
        .is_some_and(|file_name| file_name.ends_with(".effect.dawn"))
    {
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
            .map(|diagnostic| effect_diagnostic(path, text, diagnostic))
            .collect(),
    }
}

fn effect_diagnostic(path: &Utf8Path, text: &str, diagnostic: EffectDiagnostic) -> IoDiagnostic {
    IoDiagnostic {
        path: path.to_path_buf(),
        range: Some(byte_range(text, diagnostic.span.start, diagnostic.span.end)),
        severity: IoDiagnosticSeverity::Error,
        code: IoDiagnosticCode::EffectCompile,
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
            .borrow()
            .get(path)
            .and_then(|index| index.range_for_value(value))
    })
}

fn source_range_for_field_value(path: &Utf8Path, value: &Value, key: &str) -> Option<TextRange> {
    YAML_SOURCE_INDICES.with(|indices| {
        indices
            .borrow()
            .get(path)
            .and_then(|index| index.range_for_field_value(value, key))
    })
}

fn source_range_for_scalar(path: &Utf8Path, value: &str) -> Option<TextRange> {
    YAML_SOURCE_INDICES.with(|indices| {
        indices
            .borrow()
            .get(path)
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
    let mut written_files = Vec::new();
    for document in session.source.documents.values() {
        let output_path = output_root.join(&document.relative_path);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|source| ExportProjectError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let text = document_text(document).map_err(|error| match error {
            ExportProjectError::Serialize { source, .. } => ExportProjectError::Serialize {
                path: output_path.clone(),
                source,
            },
            other => other,
        })?;
        fs::write(&output_path, text).map_err(|source| ExportProjectError::Io {
            path: output_path.clone(),
            source,
        })?;
        written_files.push(document.relative_path.clone());
    }
    Ok(written_files)
}

fn document_text(document: &SourceDocument) -> Result<String, ExportProjectError> {
    match &document.kind {
        SourceDocumentKind::Dawn { value, .. } => {
            yaml_serde::to_string(value).map_err(|source| ExportProjectError::Serialize {
                path: document.relative_path.clone(),
                source,
            })
        }
        SourceDocumentKind::Effect { source } => Ok(source.clone()),
    }
}

fn sync_project_source(session: &mut ProjectSession) -> Result<(), ExportProjectError> {
    let documents = session.source.documents.keys().cloned().collect::<Vec<_>>();
    for path in documents {
        let Some(document) = session.source.documents.get(&path) else {
            continue;
        };
        let SourceDocumentKind::Dawn { object_types, .. } = &document.kind else {
            continue;
        };
        let object_types = object_types.clone();
        let imports = document.imports.clone();
        let exported_objects = document.exported_objects.clone();
        let existing = match &document.kind {
            SourceDocumentKind::Dawn { value, .. } => mapping(value).cloned(),
            SourceDocumentKind::Effect { .. } => None,
        };

        let mut root = Mapping::new();
        if !imports.is_empty() {
            root.insert(string_value("imports"), import_decls_value(&imports));
        }
        for object_key in &exported_objects {
            let object_id =
                source_object_id_for_location(session, &path, object_key).ok_or_else(|| {
                    ExportProjectError::InvalidReference {
                        path: path.clone(),
                        reference: object_key.clone(),
                        message: "source object is missing from the source map".to_string(),
                    }
                })?;
            let value = if has_typed_object(session, &object_id) {
                serialize_source_object(session, &path, &object_id)?
            } else {
                existing
                    .as_ref()
                    .and_then(|mapping| mapping.get(string_value(object_key)))
                    .cloned()
                    .ok_or_else(|| ExportProjectError::InvalidReference {
                        path: path.clone(),
                        reference: object_key.clone(),
                        message: "source object is missing from the source document".to_string(),
                    })?
            };
            root.insert(string_value(object_key), value);
        }

        let Some(document) = session.source.documents.get_mut(&path) else {
            continue;
        };
        document.kind = SourceDocumentKind::Dawn {
            value: Value::Mapping(root),
            object_types,
        };
    }
    Ok(())
}

fn has_typed_object(session: &ProjectSession, id: &SourceObjectId) -> bool {
    match id.kind {
        SourceObjectKind::Project => session.project.root.id.0 == id.id,
        SourceObjectKind::Setup => session.project.setups.contains_key(&SetupId(id.id.clone())),
        SourceObjectKind::Controller => session
            .project
            .controllers
            .contains_key(&ControllerId(id.id.clone())),
        SourceObjectKind::Layout => session
            .project
            .layouts
            .contains_key(&LayoutId(id.id.clone())),
        SourceObjectKind::Patch => session
            .project
            .patches
            .contains_key(&PatchId(id.id.clone())),
        SourceObjectKind::FixtureDefinition => session
            .project
            .definitions
            .fixtures
            .definitions
            .contains_key(&FixtureDefinitionId(id.id.clone())),
        SourceObjectKind::Curve => session
            .project
            .definitions
            .curves
            .definitions
            .contains_key(&CurveId(id.id.clone())),
        SourceObjectKind::Sequence => session
            .project
            .sequences
            .contains_key(&SequenceId(id.id.clone())),
        SourceObjectKind::EffectDefinition => session
            .project
            .definitions
            .effects
            .definitions
            .contains_key(&EffectDefinitionId(id.id.clone())),
        SourceObjectKind::EffectInstance => false,
    }
}

fn source_object_id_for_location(
    session: &ProjectSession,
    document: &Utf8Path,
    object_key: &str,
) -> Option<SourceObjectId> {
    session
        .source
        .source_map
        .objects
        .iter()
        .find_map(|(id, location)| {
            (location.document == document && location.object_key == object_key).then(|| id.clone())
        })
}

fn serialize_source_object(
    session: &ProjectSession,
    from_document: &Utf8Path,
    id: &SourceObjectId,
) -> Result<Value, ExportProjectError> {
    match id.kind {
        SourceObjectKind::Project => project_root_value(session, from_document),
        SourceObjectKind::Setup => {
            let setup = session
                .project
                .setups
                .get(&SetupId(id.id.clone()))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            setup_value(session, from_document, setup)
        }
        SourceObjectKind::Controller => {
            let controller = session
                .project
                .controllers
                .get(&ControllerId(id.id.clone()))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            controller_value(controller)
        }
        SourceObjectKind::Layout => {
            let layout = session
                .project
                .layouts
                .get(&LayoutId(id.id.clone()))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            layout_value(session, from_document, layout)
        }
        SourceObjectKind::Patch => {
            let patch = session
                .project
                .patches
                .get(&PatchId(id.id.clone()))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            patch_value(session, from_document, patch)
        }
        SourceObjectKind::FixtureDefinition => {
            let definition = session
                .project
                .definitions
                .fixtures
                .definitions
                .get(&FixtureDefinitionId(id.id.clone()))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            fixture_definition_value(definition)
        }
        SourceObjectKind::Curve => {
            let curve = session
                .project
                .definitions
                .curves
                .definitions
                .get(&CurveId(id.id.clone()))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            curve_value(&curve.curve)
        }
        SourceObjectKind::Sequence => {
            let sequence = session
                .project
                .sequences
                .get(&SequenceId(id.id.clone()))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            sequence_value(session, from_document, sequence)
        }
        SourceObjectKind::EffectDefinition | SourceObjectKind::EffectInstance => {
            Err(ExportProjectError::InvalidReference {
                path: from_document.to_path_buf(),
                reference: id.id.clone(),
                message: "effect definitions are preserved as effect source documents".to_string(),
            })
        }
    }
}

fn missing_typed_object(path: &Utf8Path, id: &SourceObjectId) -> ExportProjectError {
    ExportProjectError::InvalidReference {
        path: path.to_path_buf(),
        reference: id.id.clone(),
        message: "typed project object is missing".to_string(),
    }
}

fn import_decls_value(imports: &[ImportDecl]) -> Value {
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
        Value::String(write_reference(
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
                    write_reference(
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
        Value::String(write_reference(
            session,
            from_document,
            SourceObjectKind::Layout,
            &setup.layout.0,
        )?),
    );
    value.insert(
        string_value("patch"),
        Value::String(write_reference(
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
                .map(|controller| Value::String(controller.0.clone()))
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
        Value::String(write_reference(
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
        Value::String(write_reference(
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
        string_value("clips"),
        Value::Sequence(
            sequence
                .clips
                .iter()
                .map(|clip| sequence_clip_value(session, from_document, clip))
                .collect::<Result<Vec<_>, _>>()?,
        ),
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

fn sequence_clip_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    clip: &SequenceClip,
) -> Result<Value, ExportProjectError> {
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
    value.insert(string_value("target"), effect_target_value(&clip.target)?);
    value.insert(
        string_value("scope"),
        Value::String(
            match clip.scope {
                EffectScope::PerFixture => "per_fixture",
                EffectScope::WholeTarget => "whole_target",
            }
            .to_string(),
        ),
    );
    match &clip.kind {
        SequenceClipKind::Effect(effect) => {
            value.insert(string_value("type"), Value::String("effect".to_string()));
            write_effect_clip_fields(session, from_document, &mut value, effect)?;
        }
        SequenceClipKind::Graph(graph) => {
            value.insert(string_value("type"), Value::String("graph".to_string()));
            value.insert(
                string_value("nodes"),
                Value::Sequence(
                    graph
                        .nodes
                        .iter()
                        .map(|node| graph_node_value(session, from_document, node))
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
        }
    }
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
        Value::String(write_reference(
            session,
            from_document,
            SourceObjectKind::EffectDefinition,
            &effect.definition.0,
        )?),
    );
    Ok(())
}

fn graph_node_value(
    session: &ProjectSession,
    from_document: &Utf8Path,
    node: &EffectGraphNode,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), number_value(node.id.0)?);
    value.insert(
        string_value("position"),
        graph_position_value(&node.position)?,
    );
    match &node.kind {
        EffectGraphNodeKind::Source(source) => {
            value.insert(string_value("type"), Value::String("source".to_string()));
            value.insert(
                string_value("start"),
                Value::String(seconds_string(source.start.as_seconds_f64())),
            );
            value.insert(
                string_value("duration"),
                Value::String(seconds_string(source.duration.as_seconds_f64())),
            );
            value.insert(string_value("target"), effect_target_value(&source.target)?);
            value.insert(
                string_value("scope"),
                Value::String(
                    match source.scope {
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
                    definition: source.definition.clone(),
                    param_overrides: source.param_overrides.clone(),
                },
            )?;
        }
        EffectGraphNodeKind::Operator(operator) => {
            value.insert(string_value("type"), Value::String("operator".to_string()));
            value.insert(
                string_value("operator"),
                Value::String(graph_operator_name(&operator.operator).to_string()),
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
        EffectGraphNodeKind::Output => {
            value.insert(string_value("type"), Value::String("output".to_string()));
        }
    }
    Ok(Value::Mapping(value))
}

fn graph_position_value(position: &GraphNodePosition) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("x"), number_value(position.x)?);
    value.insert(string_value("y"), number_value(position.y)?);
    Ok(Value::Mapping(value))
}

fn graph_edge_value(edge: &EffectGraphEdge) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("from_node"), number_value(edge.from_node.0)?);
    value.insert(
        string_value("from_port"),
        Value::String(edge.from_port.0.clone()),
    );
    value.insert(string_value("to_node"), number_value(edge.to_node.0)?);
    value.insert(
        string_value("to_port"),
        Value::String(edge.to_port.0.clone()),
    );
    Ok(Value::Mapping(value))
}

fn graph_operator_name(operator: &GraphOperator) -> &'static str {
    match operator {
        GraphOperator::Max => "max",
        GraphOperator::Add => "add",
        GraphOperator::Multiply => "multiply",
        GraphOperator::IntensityModulate => "intensity_modulate",
        GraphOperator::Dim => "dim",
        GraphOperator::Invert => "invert",
        GraphOperator::Colorize => "colorize",
        GraphOperator::Delay => "delay",
        GraphOperator::Echo => "echo",
        GraphOperator::RemapNearest => "remap_nearest",
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
        AutomationTarget::EffectClipParam { clip_id, param } => {
            value.insert(
                string_value("type"),
                Value::String("effect_clip_param".to_string()),
            );
            value.insert(string_value("clip_id"), number_value(clip_id.0)?);
            value.insert(
                string_value("param"),
                Value::String(param.as_str().to_string()),
            );
        }
        AutomationTarget::GraphNodeParam {
            clip_id,
            node_id,
            param,
        } => {
            value.insert(
                string_value("type"),
                Value::String("graph_node_param".to_string()),
            );
            value.insert(string_value("clip_id"), number_value(clip_id.0)?);
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
        CurveSource::Reference(id) => Ok(Value::String(write_reference(
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

fn write_reference(
    session: &ProjectSession,
    from_document: &Utf8Path,
    kind: SourceObjectKind,
    id: &str,
) -> Result<String, ExportProjectError> {
    let target = SourceObjectId {
        kind: kind.clone(),
        id: id.to_string(),
    };
    let location = session
        .source
        .source_map
        .objects
        .get(&target)
        .ok_or_else(|| ExportProjectError::InvalidReference {
            path: from_document.to_path_buf(),
            reference: id.to_string(),
            message: "target is missing from the source map".to_string(),
        })?;
    let alias = session
        .source
        .import_graph
        .get(from_document)
        .into_iter()
        .flatten()
        .find(|edge| {
            edge.targets
                .iter()
                .any(|target| target == &location.document)
        })
        .map(|edge| edge.alias.clone())
        .ok_or_else(|| ExportProjectError::InvalidReference {
            path: from_document.to_path_buf(),
            reference: id.to_string(),
            message: "no import alias makes the target visible from this document".to_string(),
        })?;
    Ok(format!("{alias}.{}", location.object_key))
}

fn relative_path_from_document(from_document: &Utf8Path, target: &Utf8Path) -> Utf8PathBuf {
    let from_dir = from_document.parent().unwrap_or_else(|| Utf8Path::new(""));
    let from_components = normal_components(from_dir);
    let target_components = normal_components(target);
    let common_len = from_components
        .iter()
        .zip(target_components.iter())
        .take_while(|(left, right)| left == right)
        .count();
    let mut relative = Utf8PathBuf::new();
    for _ in common_len..from_components.len() {
        relative.push("..");
    }
    for component in target_components.iter().skip(common_len) {
        relative.push(component);
    }
    relative
}

fn normal_components(path: &Utf8Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Utf8Component::Normal(value) => Some(value.to_string()),
            Utf8Component::ParentDir => Some("..".to_string()),
            _ => None,
        })
        .collect()
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
                    .map(|diagnostic| effect_diagnostic(relative, &source, diagnostic))
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
        let value = parse_yaml_value(relative, &text)?;
        let map = mapping(&value).ok_or_else(|| LoadProjectError::InvalidDocument {
            path: relative.to_path_buf(),
            range: None,
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
                    range: None,
                    message: "object keys must be strings".to_string(),
                });
            };
            if key == "imports" {
                continue;
            }
            let object_type = string_field(relative, object_value, "type")?;
            let object = match object_type {
                "project" => ResolvedObject::Project(ProjectId(key.to_string())),
                "setup" => ResolvedObject::Setup(SetupId(key.to_string())),
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
                        range: source_range_for_field_value(relative, object_value, "type"),
                        message: format!("unsupported object type `{other}`"),
                    });
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
        let root_id = ProjectId(root_object.key.clone());
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
        let location = self
            .source_map
            .objects
            .get(&SourceObjectId {
                kind: id.source_kind(),
                id: id.id_string(),
            })
            .ok_or_else(|| LoadProjectError::InvalidReference {
                path: self.entrypoint.clone(),
                range: None,
                reference: id.id_string(),
            })?;
        let document = self.dawn_document(&location.document)?;
        let document_map = mapping(document).ok_or_else(|| LoadProjectError::InvalidDocument {
            path: location.document.clone(),
            range: None,
            message: "document root must be a mapping".to_string(),
        })?;
        let value = document_map
            .get(Value::String(location.object_key.clone()))
            .ok_or_else(|| LoadProjectError::InvalidReference {
                path: location.document.clone(),
                range: None,
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
                    range: None,
                    message: "document not loaded".to_string(),
                })?;
        match &document.kind {
            SourceDocumentKind::Dawn { value, .. } => Ok(value),
            SourceDocumentKind::Effect { .. } => Err(LoadProjectError::InvalidDocument {
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
        let (alias, object) =
            reference
                .split_once('.')
                .ok_or_else(|| LoadProjectError::InvalidReference {
                    path: path.to_path_buf(),
                    range: source_range_for_scalar(path, reference),
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
                range: source_range_for_scalar(path, reference),
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
            .map(|name| ControllerId(name.to_string()))
            .collect::<Vec<_>>();
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
                    range: None,
                    message: format!("unsupported fixture geometry `{other}`"),
                });
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
                    range: None,
                    reference: controller_ref.to_string(),
                });
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
        let audio = self.parse_audio(&document_path, &value)?;
        let mark_collections = optional_sequence(&value, "mark_collections")
            .unwrap_or_default()
            .iter()
            .map(|collection| parse_mark_collection(&document_path, collection))
            .collect::<Result<Vec<_>, _>>()?;
        let clips = sequence_values(&document_path, &value, "clips")?
            .iter()
            .map(|clip| self.parse_sequence_clip(&document_path, clip))
            .collect::<Result<Vec<_>, _>>()?;
        let effects = effect_inst_mirror_from_clips(&clips);
        let automation_clips = optional_sequence(&value, "automation_clips")
            .unwrap_or_default()
            .iter()
            .map(|clip| self.parse_automation_clip(&document_path, clip))
            .collect::<Result<Vec<_>, _>>()?;
        self.project.sequences.insert(
            id.clone(),
            Sequence {
                id: id.clone(),
                duration: parse_duration(string_field(&document_path, &value, "duration")?)
                    .map_err(|error| {
                        with_yaml_location(
                            error,
                            &document_path,
                            source_range_for_field_value(&document_path, &value, "duration"),
                        )
                    })?,
                frame_rate: u32_field(&document_path, &value, "frame_rate")?,
                audio,
                mark_collections,
                clips,
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

    fn parse_sequence_clip(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<SequenceClip, LoadProjectError> {
        let kind = match string_field(path, value, "type")? {
            "effect" => SequenceClipKind::Effect(self.parse_effect_clip(path, value)?),
            "graph" => SequenceClipKind::Graph(EffectGraphClip {
                nodes: sequence_values(path, value, "nodes")?
                    .iter()
                    .map(|node| self.parse_graph_node(path, node))
                    .collect::<Result<Vec<_>, _>>()?,
                edges: sequence_values(path, value, "edges")?
                    .iter()
                    .map(|edge| parse_graph_edge(path, edge))
                    .collect::<Result<Vec<_>, _>>()?,
            }),
            other => {
                return Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_field_value(path, value, "type"),
                    message: format!("unsupported sequence clip type `{other}`"),
                });
            }
        };
        Ok(SequenceClip {
            id: SequenceClipId(u32_field(path, value, "id")?),
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

    fn parse_graph_node(
        &mut self,
        path: &Utf8Path,
        value: &Value,
    ) -> Result<EffectGraphNode, LoadProjectError> {
        let kind = match string_field(path, value, "type")? {
            "source" => {
                let effect = self.parse_effect_clip(path, value)?;
                EffectGraphNodeKind::Source(GraphSourceNode {
                    start: parse_duration_as_time(string_field(path, value, "start")?).map_err(
                        |error| {
                            with_yaml_location(
                                error,
                                path,
                                source_range_for_field_value(path, value, "start"),
                            )
                        },
                    )?,
                    duration: parse_duration(string_field(path, value, "duration")?).map_err(
                        |error| {
                            with_yaml_location(
                                error,
                                path,
                                source_range_for_field_value(path, value, "duration"),
                            )
                        },
                    )?,
                    target: parse_effect_target(path, required_field(path, value, "target")?)?,
                    scope: parse_effect_scope(path, value)?,
                    definition: effect.definition,
                    param_overrides: effect.param_overrides,
                })
            }
            "operator" => EffectGraphNodeKind::Operator(GraphOperatorNode {
                operator: parse_graph_operator(path, value)?,
                params: self.parse_graph_operator_params(path, value)?,
            }),
            "output" => EffectGraphNodeKind::Output,
            other => {
                return Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_field_value(path, value, "type"),
                    message: format!("unsupported graph node type `{other}`"),
                });
            }
        };
        Ok(EffectGraphNode {
            id: EffectGraphNodeId(u32_field(path, value, "id")?),
            position: parse_graph_position(path, required_field(path, value, "position")?)?,
            kind,
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
                range: None,
                reference: id.0.clone(),
            }
        })?;
        let compiled =
            compile_effects(source).map_err(|diagnostics| LoadProjectError::InvalidEffect {
                path: self.loader.entrypoint.clone(),
                diagnostics: diagnostics
                    .into_iter()
                    .map(|diagnostic| {
                        effect_diagnostic(&self.loader.entrypoint, source, diagnostic)
                    })
                    .collect(),
            })?;
        if !compiled.iter().any(|effect| effect.name().as_str() == id.0) {
            return Err(LoadProjectError::InvalidReference {
                path: self.loader.entrypoint.clone(),
                range: None,
                reference: id.0.clone(),
            });
        }
        for compiled in compiled {
            let definition_id = EffectDefinitionId(compiled.name().as_str().to_string());
            self.project
                .definitions
                .effects
                .insert(definition_id, EffectDefinition { compiled });
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

fn effect_inst_mirror_from_clips(clips: &[SequenceClip]) -> Vec<EffectInst> {
    clips
        .iter()
        .filter_map(|clip| {
            let SequenceClipKind::Effect(effect) = &clip.kind else {
                return None;
            };
            Some(EffectInst {
                id: EffectInstId(clip.id.0),
                start: clip.start.clone(),
                duration: clip.duration.clone(),
                target: clip.target.clone(),
                scope: clip.scope.clone(),
                definition: effect.definition.clone(),
                param_overrides: effect.param_overrides.clone(),
            })
        })
        .collect()
}

fn parse_automation_binding(
    path: &Utf8Path,
    value: &Value,
) -> Result<AutomationBinding, LoadProjectError> {
    let target = parse_automation_target(path, required_field(path, value, "target")?)?;
    let (effect_id, param) = automation_binding_mirror(&target);
    Ok(AutomationBinding {
        target,
        effect_id,
        param,
        mapping: parse_automation_mapping(path, required_field(path, value, "mapping")?)?,
    })
}

fn automation_binding_mirror(target: &AutomationTarget) -> (EffectInstId, Identifier) {
    match target {
        AutomationTarget::EffectClipParam { clip_id, param } => {
            (EffectInstId(clip_id.0), param.clone())
        }
        AutomationTarget::GraphNodeParam { clip_id, param, .. } => {
            (EffectInstId(clip_id.0), param.clone())
        }
    }
}

fn parse_automation_target(
    path: &Utf8Path,
    value: &Value,
) -> Result<AutomationTarget, LoadProjectError> {
    Ok(match string_field(path, value, "type")? {
        "effect_clip_param" => AutomationTarget::EffectClipParam {
            clip_id: SequenceClipId(u32_field(path, value, "clip_id")?),
            param: parse_identifier_field(path, value, "param")?,
        },
        "graph_node_param" => AutomationTarget::GraphNodeParam {
            clip_id: SequenceClipId(u32_field(path, value, "clip_id")?),
            node_id: EffectGraphNodeId(u32_field(path, value, "node_id")?),
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
}

impl ResolvedObject {
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
        }
    }

    fn id_string(&self) -> String {
        match self {
            Self::Project(id) => id.0.clone(),
            Self::Setup(id) => id.0.clone(),
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
            range: None,
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
        from_node: EffectGraphNodeId(u32_field(path, value, "from_node")?),
        from_port: GraphPortId(string_field(path, value, "from_port")?.to_string()),
        to_node: EffectGraphNodeId(u32_field(path, value, "to_node")?),
        to_port: GraphPortId(string_field(path, value, "to_port")?.to_string()),
    })
}

fn parse_graph_operator(path: &Utf8Path, value: &Value) -> Result<GraphOperator, LoadProjectError> {
    match string_field(path, value, "operator")? {
        "max" => Ok(GraphOperator::Max),
        "add" => Ok(GraphOperator::Add),
        "multiply" => Ok(GraphOperator::Multiply),
        "intensity_modulate" => Ok(GraphOperator::IntensityModulate),
        "dim" => Ok(GraphOperator::Dim),
        "invert" => Ok(GraphOperator::Invert),
        "colorize" => Ok(GraphOperator::Colorize),
        "delay" => Ok(GraphOperator::Delay),
        "echo" => Ok(GraphOperator::Echo),
        "remap_nearest" => Ok(GraphOperator::RemapNearest),
        other => Err(LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, "operator"),
            message: format!("unsupported graph operator `{other}`"),
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
