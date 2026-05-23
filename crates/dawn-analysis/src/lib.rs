use std::collections::HashMap;
use std::fmt;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use dawn_project::{
    parse_document, DocumentRole, ParsedDocument, ProjectSymbolKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(u32);

impl FileId {
    fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    fn raw(self) -> u32 {
        self.0
    }

    fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct SourceRootId(u32);

impl SourceRootId {
    fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceRoot {
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    id: FileId,
    root: SourceRootId,
    path: PathBuf,
    text: String,
    revision: u64,
}

impl SourceFile {
    pub fn id(&self) -> FileId {
        self.id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}

pub struct Analysis {
    roots: Vec<SourceRoot>,
    default_root: SourceRootId,
    files: Vec<SourceFile>,
    paths: HashMap<(SourceRootId, PathBuf), FileId>,
    workspace_paths: HashMap<PathBuf, FileId>,
}

impl fmt::Debug for Analysis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Analysis")
            .field("roots", &self.roots)
            .field("files", &self.files)
            .field("paths", &self.paths)
            .field("workspace_paths", &self.workspace_paths)
            .finish()
    }
}

impl Default for Analysis {
    fn default() -> Self {
        let default_root = SourceRootId(0);
        Self {
            roots: vec![SourceRoot {
                path: PathBuf::new(),
            }],
            default_root,
            files: Vec::new(),
            paths: HashMap::new(),
            workspace_paths: HashMap::new(),
        }
    }
}

impl Analysis {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_file(&mut self, path: impl Into<PathBuf>, text: impl Into<String>) -> FileId {
        self.set_file_in_root(self.default_root, path, text)
    }

    fn set_file_in_root(
        &mut self,
        root: SourceRootId,
        path: impl Into<PathBuf>,
        text: impl Into<String>,
    ) -> FileId {
        let path = normalize_path(path.into());
        let text = text.into();
        self.source_root(root);
        let path_key = (root, path.clone());

        if let Some(id) = self.paths.get(&path_key).copied() {
            let source = &mut self.files[id.index()];
            if source.text != text {
                source.text = text;
                source.revision += 1;
            }
            return id;
        }

        let id = FileId::from_raw(self.files.len() as u32);
        let workspace_path = self.workspace_path(root, &path);
        self.files.push(SourceFile {
            id,
            root,
            path: path.clone(),
            text,
            revision: 0,
        });
        self.paths.insert(path_key, id);
        self.workspace_paths.entry(workspace_path).or_insert(id);
        id
    }

    #[cfg(test)]
    #[allow(dead_code)]
    fn add_source_root(&mut self, path: impl Into<PathBuf>) -> SourceRootId {
        let id = SourceRootId(self.roots.len() as u32);
        self.roots.push(SourceRoot {
            path: normalize_path(path.into()),
        });
        id
    }

    pub fn update_file(
        &mut self,
        file: FileId,
        text: impl Into<String>,
    ) -> Result<(), AnalysisError> {
        let text = text.into();
        let source = self.source_file_mut(file)?;
        if source.text != text {
            source.text = text;
            source.revision += 1;
        }
        Ok(())
    }

    pub fn file(&self, file: FileId) -> Result<&SourceFile, AnalysisError> {
        self.source_file(file)
    }

    pub fn analyze_file(&self, file: FileId) -> Result<FileAnalysis, AnalysisError> {
        let source = self.source_file(file)?;
        let parsed = parse_document(source.path(), source.text());
        let mut diagnostics = parsed
            .diagnostics
            .iter()
            .map(|diagnostic| AnalysisDiagnostic {
                file,
                message: diagnostic.message.clone(),
                severity: DiagnosticSeverity::Error,
                range: diagnostic.range.clone(),
                source: DiagnosticSource::Analysis,
                code: DiagnosticCode::InvalidDocument,
            })
            .collect::<Vec<_>>();

        let imports = self.path_refs(source, &parsed, &mut diagnostics);
        let document_symbols = parsed
            .symbols
            .into_iter()
            .map(|symbol| DocumentSymbol {
                name: symbol.name,
                kind: symbol_kind(symbol.kind),
                file,
                range: symbol.range,
                selection_range: symbol.selection_range,
            })
            .collect();

        Ok(FileAnalysis {
            diagnostics,
            imports,
            document_symbols,
            role: parsed.role,
        })
    }

    pub fn diagnostics(&self, file: FileId) -> Result<Vec<AnalysisDiagnostic>, AnalysisError> {
        self.analyze_file(file).map(|analysis| analysis.diagnostics)
    }

    pub fn imports(&self, file: FileId) -> Result<Vec<ImportInfo>, AnalysisError> {
        self.analyze_file(file).map(|analysis| analysis.imports)
    }

    pub fn document_symbols(&self, file: FileId) -> Result<Vec<DocumentSymbol>, AnalysisError> {
        self.analyze_file(file)
            .map(|analysis| analysis.document_symbols)
    }

    pub fn role(&self, file: FileId) -> Result<DocumentRole, AnalysisError> {
        self.analyze_file(file).map(|analysis| analysis.role)
    }

    fn path_refs(
        &self,
        source: &SourceFile,
        parsed: &ParsedDocument,
        diagnostics: &mut Vec<AnalysisDiagnostic>,
    ) -> Vec<ImportInfo> {
        parsed
            .path_refs
            .iter()
            .map(|path_ref| {
                let (path, path_error) = match DawnPath::parse(path_ref.raw_path.clone()) {
                    Ok(path) => (Some(path), None),
                    Err(error) => {
                        diagnostics.push(AnalysisDiagnostic {
                            file: source.id,
                            message: format!(
                                "invalid path `{}`: {error}",
                                path_ref.raw_path
                            ),
                            severity: DiagnosticSeverity::Error,
                            range: path_ref.range.clone(),
                            source: DiagnosticSource::Analysis,
                            code: DiagnosticCode::InvalidImportPath,
                        });
                        (None, Some(error))
                    }
                };
                let resolved_file = path
                    .as_ref()
                    .and_then(|path| self.resolve_import(source, path));
                if path.is_some() && resolved_file.is_none() {
                    diagnostics.push(AnalysisDiagnostic {
                        file: source.id,
                        message: format!("unresolved path `{}`", path_ref.raw_path),
                        severity: DiagnosticSeverity::Error,
                        range: path_ref.range.clone(),
                        source: DiagnosticSource::Analysis,
                        code: DiagnosticCode::UnresolvedImport,
                    });
                }

                ImportInfo {
                    file: source.id,
                    kind: path_ref.label.clone(),
                    name: path_ref.label.clone(),
                    raw_path: path_ref.raw_path.clone(),
                    path,
                    resolved_file,
                    range: path_ref.range.clone().unwrap_or(0..0),
                    path_range: path_ref.range.clone(),
                    path_error,
                }
            })
            .collect()
    }

    fn source_file(&self, file: FileId) -> Result<&SourceFile, AnalysisError> {
        self.files
            .get(file.index())
            .filter(|source| source.id == file)
            .ok_or(AnalysisError::UnknownFile(file))
    }

    fn source_file_mut(&mut self, file: FileId) -> Result<&mut SourceFile, AnalysisError> {
        self.files
            .get_mut(file.index())
            .filter(|source| source.id == file)
            .ok_or(AnalysisError::UnknownFile(file))
    }

    fn source_root(&self, root: SourceRootId) -> &SourceRoot {
        &self.roots[root.index()]
    }

    fn workspace_path(&self, root: SourceRootId, path: &Path) -> PathBuf {
        let mut workspace_path = self.source_root(root).path.clone();
        workspace_path.push(path);
        normalize_path(workspace_path)
    }

    fn source_workspace_path(&self, source: &SourceFile) -> PathBuf {
        self.workspace_path(source.root, &source.path)
    }

    fn resolve_import(&self, source: &SourceFile, path: &DawnPath) -> Option<FileId> {
        let mut root_relative = source
            .path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf();
        root_relative.push(path.to_path_buf());
        let root_relative = normalize_path(root_relative);
        if let Some(file) = self.paths.get(&(source.root, root_relative)).copied() {
            return Some(file);
        }

        let mut workspace_path = self
            .source_workspace_path(source)
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf();
        workspace_path.push(path.to_path_buf());
        let workspace_path = normalize_path(workspace_path);
        self.workspace_paths.get(&workspace_path).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisError {
    UnknownFile(FileId),
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFile(file) => write!(f, "unknown file id {}", file.raw()),
        }
    }
}

impl std::error::Error for AnalysisError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAnalysis {
    pub diagnostics: Vec<AnalysisDiagnostic>,
    pub imports: Vec<ImportInfo>,
    pub document_symbols: Vec<DocumentSymbol>,
    pub role: DocumentRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisDiagnostic {
    pub file: FileId,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub range: Option<Range<usize>>,
    pub source: DiagnosticSource,
    pub code: DiagnosticCode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticCode {
    InvalidDocument,
    InvalidImportPath,
    UnresolvedImport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSource {
    Analysis,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportInfo {
    pub file: FileId,
    pub kind: String,
    pub name: String,
    pub raw_path: String,
    pub path: Option<DawnPath>,
    pub resolved_file: Option<FileId>,
    pub range: Range<usize>,
    pub path_range: Option<Range<usize>>,
    pub path_error: Option<DawnPathParseError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DawnPath {
    raw_text: String,
    components: Vec<DawnPathComponent>,
}

impl DawnPath {
    pub fn parse(raw_text: impl Into<String>) -> Result<Self, DawnPathParseError> {
        let raw_text = raw_text.into();
        if raw_text.is_empty() {
            return Err(DawnPathParseError::Empty);
        }
        if raw_text.contains('\\') {
            return Err(DawnPathParseError::Backslash);
        }
        if raw_text.starts_with('/') {
            return Err(DawnPathParseError::Rooted);
        }

        let mut components = Vec::new();
        for component in raw_text.split('/') {
            if component.is_empty() {
                return Err(DawnPathParseError::EmptyComponent);
            }
            components.push(match component {
                "." => DawnPathComponent::Current,
                ".." => DawnPathComponent::Parent,
                name => DawnPathComponent::Name(name.to_string()),
            });
        }

        Ok(Self {
            raw_text,
            components,
        })
    }

    pub fn raw_text(&self) -> &str {
        &self.raw_text
    }

    pub fn components(&self) -> &[DawnPathComponent] {
        &self.components
    }

    pub fn to_path_buf(&self) -> PathBuf {
        let mut path = PathBuf::new();
        for component in &self.components {
            match component {
                DawnPathComponent::Name(name) => path.push(name),
                DawnPathComponent::Current => path.push("."),
                DawnPathComponent::Parent => path.push(".."),
            }
        }
        path
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DawnPathParseError {
    Empty,
    EmptyComponent,
    Backslash,
    Rooted,
}

impl fmt::Display for DawnPathParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "path is empty",
            Self::EmptyComponent => "path contains an empty component",
            Self::Backslash => "path contains a backslash",
            Self::Rooted => "path is rooted",
        };
        f.write_str(message)
    }
}

impl std::error::Error for DawnPathParseError {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DawnPathComponent {
    Name(String),
    Current,
    Parent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file: FileId,
    pub range: Range<usize>,
    pub selection_range: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Document,
    Import,
    Function,
    Parameter,
    Let,
    Command,
}

fn symbol_kind(kind: ProjectSymbolKind) -> SymbolKind {
    match kind {
        ProjectSymbolKind::Document | ProjectSymbolKind::Effect => SymbolKind::Document,
        ProjectSymbolKind::Event => SymbolKind::Command,
        ProjectSymbolKind::Fixture | ProjectSymbolKind::Group | ProjectSymbolKind::Controller => {
            SymbolKind::Import
        }
    }
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push("..");
                }
            }
            Component::Normal(component) => normalized.push(component),
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_and_updating_files_preserves_stable_ids() {
        let mut analysis = Analysis::new();
        let first = analysis.set_file("layouts/../project.dawn", "project:\n  name: old\n  layout: layout.dawn\n");
        let second = analysis.set_file("project.dawn", "project:\n  name: new\n  layout: layout.dawn\n");

        assert_eq!(first, second);
        assert_eq!(analysis.file(first).unwrap().revision(), 1);
    }

    #[test]
    fn role_detection_is_exposed() {
        let mut analysis = Analysis::new();
        let layout = analysis.set_file("stage.layout.dawn", "stage:\n  type: layout\n  name: stage\n");
        let events = analysis.set_file("opening.events.dawn", "");
        let effect = analysis.set_file("pulse.effect.dawn", "effect Pulse {}");

        assert_eq!(analysis.role(layout).unwrap(), DocumentRole::Yaml);
        assert_eq!(analysis.role(events).unwrap(), DocumentRole::Events);
        assert_eq!(analysis.role(effect).unwrap(), DocumentRole::Effect);
    }

    #[test]
    fn yaml_path_refs_resolve_relative_to_file() {
        let mut analysis = Analysis::new();
        let project = analysis.set_file(
            "project.dawn",
            "club:\n  type: project\n  name: club\n  display:\n    import: displays/main.display.dawn::main\n  sequences:\n    - import: sequences/opening.sequence.dawn::opening\n",
        );
        let display = analysis.set_file(
            "displays/main.display.dawn",
            "main:\n  type: display\n  name: main\n  layout:\n    import: ../layouts/stage.layout.dawn::stage\n  patch:\n    import: ../patches/house.patch.dawn::house\n",
        );
        let sequence = analysis.set_file(
            "sequences/opening.sequence.dawn",
            "opening:\n  type: sequence\n  duration: 45s\n  frame_rate: 60\n  events: opening.events.dawn\n",
        );

        let imports = analysis.imports(project).unwrap();
        assert_eq!(imports[0].resolved_file, Some(display));
        assert_eq!(imports[1].resolved_file, Some(sequence));
        assert!(analysis.diagnostics(project).unwrap().is_empty());
    }

    #[test]
    fn unresolved_yaml_path_is_diagnostic() {
        let mut analysis = Analysis::new();
        let project = analysis.set_file(
            "project.dawn",
            "club:\n  type: project\n  name: club\n  display:\n    import: missing.display.dawn::main\n",
        );

        assert!(analysis
            .diagnostics(project)
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::UnresolvedImport));
    }

    #[test]
    fn jsonl_event_paths_resolve() {
        let mut analysis = Analysis::new();
        let events = analysis.set_file(
            "sequences/opening.events.dawn",
            "{\"id\":\"evt_1\",\"type\":\"effect\",\"target\":\"Bars\",\"effect\":\"../effects/pulse.effect.dawn\",\"start\":0,\"duration\":8}\n",
        );
        let effect = analysis.set_file("effects/pulse.effect.dawn", "effect Pulse {}");

        let imports = analysis.imports(events).unwrap();
        assert_eq!(imports[0].resolved_file, Some(effect));
        assert!(analysis
            .document_symbols(events)
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "evt_1"));
    }

    #[test]
    fn invalid_yaml_is_diagnostic() {
        let mut analysis = Analysis::new();
        let file = analysis.set_file("project.dawn", "club:\n  type: project\n  display:");

        assert!(analysis
            .diagnostics(file)
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::InvalidDocument));
    }

    #[test]
    fn sequence_durations_accept_mixed_millisecond_units() {
        let mut analysis = Analysis::new();
        for (index, duration) in ["1m10s", "12s500ms", "120943ms", "14m4833ms", "400s"]
            .iter()
            .enumerate()
        {
            analysis.set_file(format!("sequences/opening_{index}.events.dawn"), "");
            let file = analysis.set_file(
                format!("sequences/duration_{index}.sequence.dawn"),
                format!(
                    "opening:\n  type: sequence\n  duration: {duration}\n  frame_rate: 60\n  events: opening_{index}.events.dawn\n"
                ),
            );

            assert_eq!(analysis.diagnostics(file).unwrap(), []);
        }
    }

    #[test]
    fn sequence_durations_reject_bare_numbers() {
        let mut analysis = Analysis::new();
        let file = analysis.set_file(
            "sequences/duration.sequence.dawn",
            "opening:\n  type: sequence\n  duration: 42\n  frame_rate: 60\n  events: opening.events.dawn\n",
        );

        assert!(analysis
            .diagnostics(file)
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::InvalidDocument));
    }

    #[test]
    fn converted_club_rig_project_loads_without_diagnostics() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/club-rig");
        let mut analysis = Analysis::new();
        let mut files = Vec::new();
        let mut pending = vec![root.clone()];
        while let Some(path) = pending.pop() {
            for entry in std::fs::read_dir(&path).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().and_then(|ext| ext.to_str()) == Some("dawn") {
                    let relative = path.strip_prefix(&root).unwrap().to_path_buf();
                    let text = std::fs::read_to_string(&path).unwrap();
                    files.push(analysis.set_file(relative, text));
                }
            }
        }

        assert!(!files.is_empty());
        for file in files {
            assert_eq!(analysis.diagnostics(file).unwrap(), []);
        }
    }
}
