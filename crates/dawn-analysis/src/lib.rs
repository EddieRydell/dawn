use std::collections::HashMap;
use std::fmt;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use dawn_semantics::{hir, lower_parse, LowerDiagnosticKind, LoweredSourceFile};
use dawn_syntax::parse;
use salsa::Setter;

#[salsa::input]
struct FileInput {
    id: u32,
    path: PathBuf,
    #[returns(ref)]
    text: String,
    revision: u64,
}

#[salsa::db]
#[derive(Clone, Default)]
struct AnalysisDb {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for AnalysisDb {}

#[salsa::tracked]
fn parse_file(db: &dyn salsa::Database, file: FileInput) -> dawn_syntax::Parse {
    parse(file.text(db))
}

#[salsa::tracked]
fn lower_file(db: &dyn salsa::Database, file: FileInput) -> LoweredSourceFile {
    let parse = parse_file(db, file);
    lower_parse(&parse)
}

#[salsa::tracked]
fn file_import_facts(db: &dyn salsa::Database, file: FileInput) -> Vec<ImportFact> {
    let lowered = lower_file(db, file);

    lowered
        .root
        .as_ref()
        .map(collect_import_facts)
        .unwrap_or_default()
}

#[salsa::tracked]
fn file_document_symbols(db: &dyn salsa::Database, file: FileInput) -> Vec<DocumentSymbol> {
    let lowered = lower_file(db, file);
    let file_id = FileId::from_raw(file.id(db));

    let mut document_symbols = Vec::new();
    if let Some(root) = &lowered.root {
        collect_document_symbols(file_id, root, &mut document_symbols);
    }
    document_symbols
}

#[salsa::tracked]
fn file_local_diagnostics(db: &dyn salsa::Database, file: FileInput) -> Vec<AnalysisDiagnostic> {
    let parse = parse_file(db, file);
    let lowered = lower_file(db, file);
    let file_id = FileId::from_raw(file.id(db));

    let mut diagnostics = Vec::new();
    diagnostics.extend(
        parse
            .diagnostics()
            .iter()
            .map(|diagnostic| AnalysisDiagnostic {
                file: file_id,
                message: diagnostic.message.clone(),
                severity: DiagnosticSeverity::Error,
                range: Some(diagnostic.range.clone()),
                source: DiagnosticSource::Syntax,
                code: DiagnosticCode::Syntax(diagnostic.kind),
            }),
    );
    diagnostics.extend(
        lowered
            .diagnostics
            .iter()
            .map(|diagnostic| AnalysisDiagnostic {
                file: file_id,
                message: diagnostic.message(),
                severity: DiagnosticSeverity::Error,
                range: diagnostic.range.clone(),
                source: DiagnosticSource::Lowering,
                code: DiagnosticCode::Lowering(diagnostic.kind.clone().into()),
            }),
    );

    diagnostics.extend(
        file_import_facts(db, file)
            .into_iter()
            .filter_map(|import| {
                import.path_error.map(|_| AnalysisDiagnostic {
                    file: file_id,
                    message: format!("invalid import path '{}'", import.raw_path),
                    severity: DiagnosticSeverity::Error,
                    range: import.path_range.clone(),
                    source: DiagnosticSource::Analysis,
                    code: DiagnosticCode::InvalidImportPath,
                })
            }),
    );

    diagnostics
}

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
    inputs: Vec<FileInput>,
    db: AnalysisDb,
}

impl fmt::Debug for Analysis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Analysis")
            .field("roots", &self.roots)
            .field("files", &self.files)
            .field("paths", &self.paths)
            .field("workspace_paths", &self.workspace_paths)
            .finish_non_exhaustive()
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
            inputs: Vec::new(),
            db: AnalysisDb::default(),
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
                source.text = text.clone();
                source.revision += 1;
                let input = self.inputs[id.index()];
                input.set_text(&mut self.db).to(text);
                input.set_revision(&mut self.db).to(source.revision);
            }
            return id;
        }

        let id = FileId::from_raw(self.files.len() as u32);
        let input = FileInput::new(&self.db, id.raw(), path.clone(), text.clone(), 0);
        let workspace_path = self.workspace_path(root, &path);
        self.files.push(SourceFile {
            id,
            root,
            path: path.clone(),
            text,
            revision: 0,
        });
        self.inputs.push(input);
        self.paths.insert(path_key, id);
        self.workspace_paths.entry(workspace_path).or_insert(id);
        id
    }

    #[cfg(test)]
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
        let index = file.index();
        let text = text.into();
        let revision = {
            let source = self.source_file_mut(file)?;
            if source.text == text {
                return Ok(());
            }
            source.text = text.clone();
            source.revision += 1;
            source.revision
        };

        {
            let input = self.inputs[index];
            input.set_text(&mut self.db).to(text);
            input.set_revision(&mut self.db).to(revision);
        }
        Ok(())
    }

    pub fn file(&self, file: FileId) -> Result<&SourceFile, AnalysisError> {
        self.source_file(file)
    }

    pub fn analyze_file(&self, file: FileId) -> Result<FileAnalysis, AnalysisError> {
        let source = self.source_file(file)?;
        let input = self.inputs[file.index()];
        let mut diagnostics = file_local_diagnostics(&self.db, input);
        let imports = file_import_facts(&self.db, input)
            .into_iter()
            .map(|import| {
                let resolved_file = import
                    .path
                    .as_ref()
                    .and_then(|path| self.resolve_import(source, path));
                if import.path.is_some() && resolved_file.is_none() {
                    diagnostics.push(AnalysisDiagnostic {
                        file,
                        message: format!("unresolved import '{}'", import.raw_path),
                        severity: DiagnosticSeverity::Error,
                        range: import.path_range.clone(),
                        source: DiagnosticSource::Analysis,
                        code: DiagnosticCode::UnresolvedImport,
                    });
                }

                ImportInfo {
                    file,
                    kind: import.kind,
                    name: import.name,
                    raw_path: import.raw_path,
                    path: import.path,
                    resolved_file,
                    range: import.range,
                    path_range: import.path_range,
                }
            })
            .collect();

        Ok(FileAnalysis {
            diagnostics,
            imports,
            document_symbols: file_document_symbols(&self.db, input),
        })
    }

    #[cfg(test)]
    fn cached_revision(&self, file: FileId) -> Result<u64, AnalysisError> {
        self.source_file(file)?;
        Ok(self.inputs[file.index()].revision(&self.db))
    }

    #[cfg(test)]
    fn cached_path(&self, file: FileId) -> Result<PathBuf, AnalysisError> {
        self.source_file(file)?;
        Ok(self.inputs[file.index()].path(&self.db))
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ImportFact {
    kind: String,
    name: String,
    raw_path: String,
    path: Option<DawnPath>,
    path_error: Option<DawnPathParseError>,
    range: Range<usize>,
    path_range: Option<Range<usize>>,
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
    Syntax(dawn_syntax::DiagnosticKind),
    Lowering(LoweringDiagnosticCode),
    InvalidImportPath,
    UnresolvedImport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoweringDiagnosticCode {
    MissingRequiredSyntax {
        parent: &'static str,
        field: &'static str,
    },
    UnknownOperator {
        operator: String,
    },
}

impl From<LowerDiagnosticKind> for LoweringDiagnosticCode {
    fn from(kind: LowerDiagnosticKind) -> Self {
        match kind {
            LowerDiagnosticKind::MissingRequiredSyntax { parent, field } => {
                Self::MissingRequiredSyntax { parent, field }
            }
            LowerDiagnosticKind::UnknownOperator { operator } => Self::UnknownOperator { operator },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSource {
    Syntax,
    Lowering,
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

fn collect_document_symbols(
    file: FileId,
    root: &hir::SourceFile,
    symbols: &mut Vec<DocumentSymbol>,
) {
    for import in &root.imports {
        symbols.push(DocumentSymbol {
            name: import.name.text.clone(),
            kind: SymbolKind::Import,
            file,
            range: import.range.clone(),
            selection_range: import.name.range.clone(),
        });
    }

    symbols.push(DocumentSymbol {
        name: root.document.name.text.clone(),
        kind: SymbolKind::Document,
        file,
        range: root.document.range.clone(),
        selection_range: root.document.name.range.clone(),
    });

    collect_block_symbols(file, &root.document.block, symbols);
}

fn collect_import_facts(root: &hir::SourceFile) -> Vec<ImportFact> {
    root.imports
        .iter()
        .map(|import| {
            let raw_path = import.path.raw_text.clone();
            let path_range = import.path.inner_range.clone();
            let (path, path_error) = match DawnPath::parse(raw_path.clone()) {
                Ok(path) => (Some(path), None),
                Err(error) => (None, Some(error)),
            };

            ImportFact {
                kind: import.kind.text.clone(),
                name: import.name.text.clone(),
                raw_path,
                path,
                path_error,
                range: import.range.clone(),
                path_range,
            }
        })
        .collect()
}

fn collect_block_symbols(file: FileId, block: &hir::Block, symbols: &mut Vec<DocumentSymbol>) {
    for item in &block.items {
        match item {
            hir::Item::FnDecl(function) => {
                symbols.push(DocumentSymbol {
                    name: function.name.text.clone(),
                    kind: SymbolKind::Function,
                    file,
                    range: function.range.clone(),
                    selection_range: function.name.range.clone(),
                });
                for param in &function.params {
                    symbols.push(DocumentSymbol {
                        name: param.name.text.clone(),
                        kind: SymbolKind::Parameter,
                        file,
                        range: param.range.clone(),
                        selection_range: param.name.range.clone(),
                    });
                }
                collect_block_symbols(file, &function.body, symbols);
            }
            hir::Item::LetStmt(let_stmt) => {
                symbols.push(DocumentSymbol {
                    name: let_stmt.name.text.clone(),
                    kind: SymbolKind::Let,
                    file,
                    range: let_stmt.range.clone(),
                    selection_range: let_stmt.name.range.clone(),
                });
            }
            hir::Item::Command(command) => {
                symbols.push(DocumentSymbol {
                    name: command.name.text.clone(),
                    kind: SymbolKind::Command,
                    file,
                    range: command.range.clone(),
                    selection_range: command.name.range.clone(),
                });
                if let Some(body) = &command.body {
                    collect_block_symbols(file, body, symbols);
                }
            }
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

    fn valid_source() -> &'static str {
        r#"import effect PulseEffect from <effects/pulse.effect.dawn>;
effect Main {
  fn sample(t float) color {
    let phase float = 1.0;
    color phase {
      intensity 1;
    }
  }
}"#
    }

    fn input(analysis: &Analysis, file: FileId) -> FileInput {
        analysis.inputs[file.index()]
    }

    #[test]
    fn adding_and_updating_files_preserves_stable_ids() {
        let mut analysis = Analysis::new();
        let first = analysis.set_file("effects/../main.effect.dawn", "effect Old {}");
        let second = analysis.set_file("main.effect.dawn", "effect New {}");
        let other = analysis.set_file("other.effect.dawn", "effect Other {}");

        assert_eq!(first, second);
        assert_ne!(first, other);
        assert_eq!(analysis.file(first).unwrap().text(), "effect New {}");
        assert_eq!(analysis.file(first).unwrap().revision(), 1);

        analysis.update_file(first, "effect Updated {}").unwrap();
        assert_eq!(analysis.file(first).unwrap().text(), "effect Updated {}");
        assert_eq!(analysis.file(first).unwrap().revision(), 2);

        analysis.update_file(first, "effect Updated {}").unwrap();
        assert_eq!(analysis.file(first).unwrap().revision(), 2);
    }

    #[test]
    fn parses_and_lowers_valid_file_without_diagnostics() {
        let mut analysis = Analysis::new();
        let main = analysis.set_file("show/main.effect.dawn", valid_source());
        analysis.set_file("show/effects/pulse.effect.dawn", "effect Pulse {}");

        let analyzed = analysis.analyze_file(main).unwrap();
        assert_eq!(analyzed.diagnostics, []);
    }

    #[test]
    fn syntax_diagnostics_are_surfaced() {
        let mut analysis = Analysis::new();
        let file = analysis.set_file("main.effect.dawn", "effect Pulse { color true }");

        let diagnostics = analysis.diagnostics(file).unwrap();
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.source == DiagnosticSource::Syntax
                && matches!(diagnostic.code, DiagnosticCode::Syntax(_))));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.range.is_some()));
    }

    #[test]
    fn lowering_diagnostics_are_surfaced_when_root_lowering_fails() {
        let mut analysis = Analysis::new();
        let file = analysis.set_file("empty.effect.dawn", "");

        let diagnostics = analysis.diagnostics(file).unwrap();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.source == DiagnosticSource::Lowering
                && matches!(diagnostic.code, DiagnosticCode::Lowering(_))
                && diagnostic.range.is_some()
        }));
    }

    #[test]
    fn parses_typed_import_paths() {
        let display = DawnPath::parse("displays/main.display.dawn").unwrap();
        assert_eq!(
            display.components(),
            &[
                DawnPathComponent::Name("displays".to_string()),
                DawnPathComponent::Name("main.display.dawn".to_string())
            ]
        );

        let parent = DawnPath::parse("../effects/pulse.effect.dawn").unwrap();
        assert_eq!(
            parent.components(),
            &[
                DawnPathComponent::Parent,
                DawnPathComponent::Name("effects".to_string()),
                DawnPathComponent::Name("pulse.effect.dawn".to_string())
            ]
        );
    }

    #[test]
    fn import_resolution_succeeds_for_registered_target_file() {
        let mut analysis = Analysis::new();
        let main = analysis.set_file(
            "shows/main.effect.dawn",
            "import effect Pulse from <effects/pulse.effect.dawn>;\neffect Main {}",
        );
        let target = analysis.set_file("shows/effects/pulse.effect.dawn", "effect Pulse {}");

        let imports = analysis.imports(main).unwrap();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].resolved_file, Some(target));
    }

    #[test]
    fn unresolved_imports_produce_analysis_diagnostic_with_path_range() {
        let mut analysis = Analysis::new();
        let file = analysis.set_file(
            "shows/main.effect.dawn",
            "import effect Missing from <effects/missing.effect.dawn>;\neffect Main {}",
        );

        let diagnostics = analysis.diagnostics(file).unwrap();
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.source == DiagnosticSource::Analysis)
            .unwrap();

        assert!(diagnostic.message.contains("unresolved import"));
        assert_eq!(diagnostic.code, DiagnosticCode::UnresolvedImport);
        assert_eq!(diagnostic.range, Some(28..55));
    }

    #[test]
    fn file_import_facts_extract_imports_without_unresolved_diagnostics() {
        let mut analysis = Analysis::new();
        let file = analysis.set_file(
            "shows/main.effect.dawn",
            "import effect Missing from <effects/missing.effect.dawn>;\neffect Main {}",
        );

        let imports = file_import_facts(&analysis.db, input(&analysis, file));
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].kind, "effect");
        assert_eq!(imports[0].name, "Missing");
        assert_eq!(imports[0].raw_path, "effects/missing.effect.dawn");
        assert!(imports[0].path.is_some());
        assert!(imports[0].path_error.is_none());

        let diagnostics = file_local_diagnostics(&analysis.db, input(&analysis, file));
        assert!(!diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::UnresolvedImport));
    }

    #[test]
    fn file_local_diagnostics_stay_file_local() {
        let mut analysis = Analysis::new();
        let syntax = analysis.set_file("syntax.effect.dawn", "effect Pulse { color true }");
        let lowering = analysis.set_file("empty.effect.dawn", "");
        let invalid_import = analysis.set_file(
            "invalid-import.effect.dawn",
            "import effect Broken from <>;\neffect Main {}",
        );
        let unresolved_import = analysis.set_file(
            "unresolved.effect.dawn",
            "import effect Missing from <effects/missing.effect.dawn>;\neffect Main {}",
        );

        assert!(
            file_local_diagnostics(&analysis.db, input(&analysis, syntax))
                .iter()
                .any(|diagnostic| diagnostic.source == DiagnosticSource::Syntax)
        );
        assert!(
            file_local_diagnostics(&analysis.db, input(&analysis, lowering))
                .iter()
                .any(|diagnostic| diagnostic.source == DiagnosticSource::Lowering)
        );
        assert!(
            file_local_diagnostics(&analysis.db, input(&analysis, invalid_import))
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::InvalidImportPath)
        );
        assert!(
            !file_local_diagnostics(&analysis.db, input(&analysis, unresolved_import))
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::UnresolvedImport)
        );
    }

    #[test]
    fn document_symbols_include_expected_top_level_and_nested_names() {
        let mut analysis = Analysis::new();
        let main = analysis.set_file("show/main.effect.dawn", valid_source());
        analysis.set_file("show/effects/pulse.effect.dawn", "effect Pulse {}");

        let symbols = analysis.document_symbols(main).unwrap();
        let names = symbols
            .iter()
            .map(|symbol| (symbol.kind, symbol.name.as_str()))
            .collect::<Vec<_>>();

        assert!(names.contains(&(SymbolKind::Import, "PulseEffect")));
        assert!(names.contains(&(SymbolKind::Document, "Main")));
        assert!(names.contains(&(SymbolKind::Function, "sample")));
        assert!(names.contains(&(SymbolKind::Parameter, "t")));
        assert!(names.contains(&(SymbolKind::Let, "phase")));
        assert!(names.contains(&(SymbolKind::Command, "color")));
    }

    #[test]
    fn document_symbols_are_available_from_focused_query() {
        let mut analysis = Analysis::new();
        let main = analysis.set_file("show/main.effect.dawn", valid_source());

        let queried = file_document_symbols(&analysis.db, input(&analysis, main));
        let public = analysis.document_symbols(main).unwrap();

        assert_eq!(queried, public);
        assert!(queried
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Document && symbol.name == "Main"));
    }

    #[test]
    fn recursively_collects_symbols_through_nested_blocks() {
        let mut analysis = Analysis::new();
        let file = analysis.set_file(
            "main.effect.dawn",
            r#"effect Main {
  outer {
    inner {
      let nested = 1;
    }
  }
}"#,
        );

        let symbols = analysis.document_symbols(file).unwrap();
        let names = symbols
            .iter()
            .map(|symbol| (symbol.kind, symbol.name.as_str()))
            .collect::<Vec<_>>();

        assert!(names.contains(&(SymbolKind::Command, "outer")));
        assert!(names.contains(&(SymbolKind::Command, "inner")));
        assert!(names.contains(&(SymbolKind::Let, "nested")));
    }

    #[test]
    fn repeated_public_queries_match_analyze_file() {
        let mut analysis = Analysis::new();
        let main = analysis.set_file("show/main.effect.dawn", valid_source());
        analysis.set_file("show/effects/pulse.effect.dawn", "effect Pulse {}");

        let analyzed = analysis.analyze_file(main).unwrap();

        assert_eq!(analysis.diagnostics(main).unwrap(), analyzed.diagnostics);
        assert_eq!(analysis.imports(main).unwrap(), analyzed.imports);
        assert_eq!(
            analysis.document_symbols(main).unwrap(),
            analyzed.document_symbols
        );
    }

    #[test]
    fn updating_file_changes_cached_diagnostics_and_symbols() {
        let mut analysis = Analysis::new();
        let file = analysis.set_file("main.effect.dawn", "effect Main { let old = 1; }");

        assert!(analysis.diagnostics(file).unwrap().is_empty());
        assert!(analysis
            .document_symbols(file)
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "old"));

        analysis
            .update_file(file, "effect Main { color true }")
            .unwrap();

        assert!(analysis
            .diagnostics(file)
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic.source == DiagnosticSource::Syntax));
        assert!(!analysis
            .document_symbols(file)
            .unwrap()
            .iter()
            .any(|symbol| symbol.name == "old"));
    }

    #[test]
    fn identical_text_update_does_not_increment_source_or_input_revision() {
        let mut analysis = Analysis::new();
        let file = analysis.set_file("main.effect.dawn", "effect Main {}");

        analysis.update_file(file, "effect Main {}").unwrap();

        assert_eq!(analysis.file(file).unwrap().revision(), 0);
        assert_eq!(analysis.cached_revision(file).unwrap(), 0);
        assert_eq!(
            analysis.cached_path(file).unwrap(),
            PathBuf::from("main.effect.dawn")
        );
    }

    #[test]
    fn adding_missing_import_target_clears_unresolved_without_updating_importer() {
        let mut analysis = Analysis::new();
        let main = analysis.set_file(
            "show/main.effect.dawn",
            "import effect Pulse from <effects/pulse.effect.dawn>;\neffect Main {}",
        );

        assert!(analysis
            .diagnostics(main)
            .unwrap()
            .iter()
            .any(|diagnostic| {
                diagnostic.source == DiagnosticSource::Analysis
                    && diagnostic.code == DiagnosticCode::UnresolvedImport
            }));
        assert_eq!(analysis.file(main).unwrap().revision(), 0);

        let target = analysis.set_file("show/effects/pulse.effect.dawn", "effect Pulse {}");

        assert!(!analysis
            .diagnostics(main)
            .unwrap()
            .iter()
            .any(|diagnostic| {
                diagnostic.source == DiagnosticSource::Analysis
                    && diagnostic.code == DiagnosticCode::UnresolvedImport
            }));
        assert_eq!(
            analysis.imports(main).unwrap()[0].resolved_file,
            Some(target)
        );
        assert_eq!(analysis.file(main).unwrap().revision(), 0);
    }

    #[test]
    fn relative_parent_import_resolution_matches_existing_path_behavior() {
        let mut analysis = Analysis::new();
        let main = analysis.set_file(
            "show/scenes/main.effect.dawn",
            "import effect Pulse from <../effects/pulse.effect.dawn>;\neffect Main {}",
        );
        let target = analysis.set_file("show/effects/pulse.effect.dawn", "effect Pulse {}");

        let imports = analysis.imports(main).unwrap();
        assert_eq!(imports[0].resolved_file, Some(target));
        assert!(analysis.diagnostics(main).unwrap().is_empty());
    }

    #[test]
    fn relative_imports_can_escape_source_root_to_registered_workspace_file() {
        let mut analysis = Analysis::new();
        let project = analysis.add_source_root("projects/show");
        let shared = analysis.add_source_root("shared");
        let main = analysis.set_file_in_root(
            project,
            "main.effect.dawn",
            "import effect Pulse from <../../shared/pulse.effect.dawn>;\neffect Main {}",
        );
        let target = analysis.set_file_in_root(shared, "pulse.effect.dawn", "effect Pulse {}");

        let imports = analysis.imports(main).unwrap();
        assert_eq!(imports[0].resolved_file, Some(target));
        assert!(analysis.diagnostics(main).unwrap().is_empty());
    }

    #[test]
    fn invalid_import_path_never_produces_unresolved_import() {
        let mut analysis = Analysis::new();
        let file = analysis.set_file(
            "main.effect.dawn",
            "import effect Broken from <>;\neffect Main {}",
        );

        let diagnostics = analysis.diagnostics(file).unwrap();
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::InvalidImportPath));
        assert!(!diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::UnresolvedImport));
        assert_eq!(analysis.imports(file).unwrap()[0].resolved_file, None);
    }

    #[test]
    fn unknown_file_id_returns_analysis_error() {
        let analysis = Analysis::new();
        let missing = FileId(99);

        assert_eq!(
            analysis.file(missing),
            Err(AnalysisError::UnknownFile(missing))
        );
        assert_eq!(
            analysis.analyze_file(missing),
            Err(AnalysisError::UnknownFile(missing))
        );
        assert_eq!(
            analysis.diagnostics(missing),
            Err(AnalysisError::UnknownFile(missing))
        );
        assert_eq!(
            analysis.imports(missing),
            Err(AnalysisError::UnknownFile(missing))
        );
        assert_eq!(
            analysis.document_symbols(missing),
            Err(AnalysisError::UnknownFile(missing))
        );
    }
}
