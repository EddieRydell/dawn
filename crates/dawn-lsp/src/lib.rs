use std::collections::HashMap;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use dawn_analysis::{
    Analysis, AnalysisDiagnostic, DiagnosticCode, DiagnosticSeverity, FileId, SymbolKind,
};
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result as JsonRpcResult;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, InitializeParams, InitializeResult,
    InitializedParams, Location, MarkupContent, MarkupKind, MessageType, OneOf, Position,
    Range as LspRange, SemanticToken, SemanticTokenType, SemanticTokens, SemanticTokensFullOptions,
    SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo, SymbolKind as LspSymbolKind,
    TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};
use tower_lsp::{async_trait, Client, LanguageServer};
use walkdir::{DirEntry, WalkDir};

const DIAGNOSTIC_SOURCE: &str = "dawn";
const IGNORED_DIRS: &[&str] = &[".git", "target", "node_modules", "dist"];
const KEYWORD_COMPLETIONS: &[&str] = &[
    "project", "layout", "display", "fixture", "patch", "sequence", "name", "sequences",
    "duration", "frame_rate", "events", "fixtures", "groups", "controllers", "routes",
    "geometry", "transform", "position", "rotation", "scale", "group", "type", "true", "false",
];
const TYPE_COMPLETIONS: &[&str] = &["float", "int", "bool", "color", "duration", "Fixture"];
const TOKEN_LEGEND: &[&str] = &[
    "keyword",
    "comment",
    "string",
    "number",
    "operator",
    "type",
    "class",
    "function",
    "method",
    "parameter",
    "variable",
];

#[derive(Debug, Default)]
pub struct LspState {
    analysis: Analysis,
    documents: HashMap<Url, OpenDocument>,
    paths: HashMap<PathBuf, FileId>,
    uris: HashMap<FileId, Url>,
}

impl LspState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn initialize_workspace(&mut self, params: &InitializeParams) -> Vec<PathBuf> {
        let roots = workspace_roots(params);
        for root in &roots {
            self.scan_workspace_root(root);
        }
        roots
    }

    pub fn scan_workspace_root(&mut self, root: &Path) {
        for entry in WalkDir::new(root)
            .into_iter()
            .filter_entry(|entry| !is_ignored_dir(entry))
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("dawn") {
                continue;
            }

            if let Ok(text) = std::fs::read_to_string(path) {
                let path = normalize_path(path.to_path_buf());
                let file = self.analysis.set_file(path.clone(), text);
                self.register_file(path, file);
            }
        }
    }

    pub fn did_open(
        &mut self,
        uri: Url,
        version: i32,
        text: String,
    ) -> Vec<tower_lsp::lsp_types::Diagnostic> {
        let path = uri_to_normalized_path(&uri).unwrap_or_else(|| PathBuf::from(uri.as_str()));
        let file = self.analysis.set_file(path.clone(), text.clone());
        self.register_file(path, file);
        self.uris.insert(file, uri.clone());
        let line_index = LineIndex::new(&text);
        self.documents.insert(
            uri,
            OpenDocument {
                file,
                text,
                version,
                line_index,
                open: true,
            },
        );
        self.diagnostics_for_file(file)
    }

    pub fn did_change(
        &mut self,
        uri: &Url,
        version: i32,
        text: String,
    ) -> Vec<tower_lsp::lsp_types::Diagnostic> {
        if let Some(document) = self.documents.get_mut(uri) {
            document.text = text.clone();
            document.version = version;
            document.line_index = LineIndex::new(&text);
            document.open = true;
            let file = document.file;
            let _ = self.analysis.update_file(file, text);
            return self.diagnostics_for_file(file);
        }

        self.did_open(uri.clone(), version, text)
    }

    pub fn did_close(&mut self, uri: &Url) {
        if let Some(document) = self.documents.get_mut(uri) {
            document.open = false;
        }
    }

    pub fn document_symbols(&self, uri: &Url) -> Option<Vec<DocumentSymbol>> {
        let document = self.documents.get(uri)?;
        let analysis_symbols = self.analysis.document_symbols(document.file).ok()?;
        Some(
            analysis_symbols
                .into_iter()
                .map(|symbol| {
                    #[allow(deprecated)]
                    let document_symbol = DocumentSymbol {
                        name: symbol.name,
                        detail: None,
                        kind: symbol_kind(symbol.kind),
                        tags: None,
                        deprecated: None,
                        range: document.line_index.range(symbol.range),
                        selection_range: document.line_index.range(symbol.selection_range),
                        children: None,
                    };
                    document_symbol
                })
                .collect(),
        )
    }

    pub fn imports_resolve(&self, file: FileId) -> bool {
        self.analysis
            .imports(file)
            .map(|imports| imports.iter().all(|import| import.resolved_file.is_some()))
            .unwrap_or(false)
    }

    pub fn file_for_uri(&self, uri: &Url) -> Option<FileId> {
        self.documents.get(uri).map(|document| document.file)
    }

    pub fn semantic_tokens(&self, uri: &Url) -> Option<SemanticTokens> {
        let document = self.documents.get(uri)?;
        let token_ranges = semantic_token_ranges(&self.analysis, document.file, &document.text);
        Some(encode_semantic_tokens(
            &document.line_index,
            &document.text,
            token_ranges,
        ))
    }

    pub fn completion(&self, uri: &Url, position: Position) -> Option<CompletionResponse> {
        let document = self.documents.get(uri)?;
        let offset = document.line_index.offset(position)?;
        let mut items = Vec::new();

        if self.import_path_at(document.file, offset).is_some() {
            items.extend(self.import_path_completions(document.file));
            return Some(CompletionResponse::Array(items));
        }

        items.extend(
            KEYWORD_COMPLETIONS
                .iter()
                .map(|label| completion_item(label, CompletionItemKind::KEYWORD)),
        );
        items.extend(
            TYPE_COMPLETIONS
                .iter()
                .map(|label| completion_item(label, CompletionItemKind::TYPE_PARAMETER)),
        );

        if let Ok(symbols) = self.analysis.document_symbols(document.file) {
            items.extend(symbols.into_iter().map(symbol_completion_item));
        }

        Some(CompletionResponse::Array(items))
    }

    pub fn hover(&self, uri: &Url, position: Position) -> Option<Hover> {
        let document = self.documents.get(uri)?;
        let offset = document.line_index.offset(position)?;

        if let Some(import) = self.import_path_at(document.file, offset) {
            let contents = match import.resolved_file {
                Some(file) => {
                    let target = self
                        .analysis
                        .file(file)
                        .map(|file| file.path().display().to_string())
                        .unwrap_or_else(|_| import.raw_path.clone());
                    format!("import target `{target}`")
                }
                None => format!("unresolved import `{}`", import.raw_path),
            };
            return Some(markdown_hover(
                contents,
                import
                    .path_range
                    .map(|range| document.line_index.range(range)),
            ));
        }

        if let Some(symbol) = self.symbol_at(document.file, offset) {
            return Some(markdown_hover(
                symbol_hover_label(symbol.kind, &symbol.name),
                Some(document.line_index.range(symbol.selection_range)),
            ));
        }

        let token = token_at(&document.text, offset)?;
        let name = token.text.as_str();
        let symbol = self.resolve_symbol(document.file, name)?;
        Some(markdown_hover(
            symbol_hover_label(symbol.kind, &symbol.name),
            Some(document.line_index.range(symbol.selection_range)),
        ))
    }

    pub fn definition(&self, uri: &Url, position: Position) -> Option<GotoDefinitionResponse> {
        let document = self.documents.get(uri)?;
        let offset = document.line_index.offset(position)?;

        if let Some(import) = self.import_path_at(document.file, offset) {
            let target = import.resolved_file?;
            return self
                .location_for_file_symbol(target)
                .map(|location| GotoDefinitionResponse::Scalar(location));
        }

        if let Some(symbol) = self.symbol_at(document.file, offset) {
            if symbol.kind == SymbolKind::Import {
                if let Some(location) = self.import_alias_definition(document.file, &symbol) {
                    return Some(GotoDefinitionResponse::Scalar(location));
                }
            }
            return self
                .location_for_symbol(symbol)
                .map(GotoDefinitionResponse::Scalar);
        }

        let token = token_at(&document.text, offset)?;
        self.resolve_symbol(document.file, &token.text)
            .and_then(|symbol| self.location_for_symbol(symbol))
            .map(GotoDefinitionResponse::Scalar)
    }

    fn diagnostics_for_file(&self, file: FileId) -> Vec<tower_lsp::lsp_types::Diagnostic> {
        let Ok(document) = self.document_for_file(file) else {
            return Vec::new();
        };
        self.analysis
            .diagnostics(file)
            .unwrap_or_default()
            .into_iter()
            .map(|diagnostic| convert_diagnostic(&document.line_index, diagnostic))
            .collect()
    }

    fn document_for_file(&self, file: FileId) -> Result<&OpenDocument, ()> {
        self.documents
            .values()
            .find(|document| document.file == file)
            .ok_or(())
    }

    fn register_file(&mut self, path: PathBuf, file: FileId) {
        let path = normalize_path(path);
        self.paths.insert(path.clone(), file);
        if let Ok(uri) = Url::from_file_path(&path) {
            self.uris.insert(file, uri);
        }
    }

    fn import_path_at(&self, file: FileId, offset: usize) -> Option<dawn_analysis::ImportInfo> {
        self.analysis
            .imports(file)
            .ok()?
            .into_iter()
            .find(|import| {
                import
                    .path_range
                    .as_ref()
                    .map(|range| range_contains(range, offset))
                    .unwrap_or(false)
            })
    }

    fn import_alias_definition(
        &self,
        file: FileId,
        symbol: &dawn_analysis::DocumentSymbol,
    ) -> Option<Location> {
        let import = self
            .analysis
            .imports(file)
            .ok()?
            .into_iter()
            .find(|import| import.name == symbol.name)?;
        self.location_for_file_symbol(import.resolved_file?)
    }

    fn import_path_completions(&self, file: FileId) -> Vec<CompletionItem> {
        let base = self
            .analysis
            .file(file)
            .ok()
            .and_then(|source| source.path().parent().map(Path::to_path_buf))
            .unwrap_or_default();
        let mut paths = self.paths.keys().cloned().collect::<Vec<_>>();
        paths.sort();
        paths
            .into_iter()
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("dawn"))
            .map(|path| {
                let label = relative_label(&base, &path);
                completion_item(&label, CompletionItemKind::FILE)
            })
            .collect()
    }

    fn symbol_at(&self, file: FileId, offset: usize) -> Option<dawn_analysis::DocumentSymbol> {
        self.analysis
            .document_symbols(file)
            .ok()?
            .into_iter()
            .find(|symbol| range_contains(&symbol.selection_range, offset))
    }

    fn resolve_symbol(&self, file: FileId, name: &str) -> Option<dawn_analysis::DocumentSymbol> {
        let mut candidates = Vec::new();
        if let Ok(symbols) = self.analysis.document_symbols(file) {
            candidates.extend(symbols.into_iter().filter(|symbol| symbol.name == name));
        }

        for target in self.sorted_workspace_files() {
            if target == file {
                continue;
            }
            if let Ok(symbols) = self.analysis.document_symbols(target) {
                candidates.extend(symbols.into_iter().filter(|symbol| symbol.name == name));
            }
        }

        candidates.sort_by(|left, right| {
            let left_path = self
                .analysis
                .file(left.file)
                .map(|file| file.path().to_path_buf())
                .unwrap_or_default();
            let right_path = self
                .analysis
                .file(right.file)
                .map(|file| file.path().to_path_buf())
                .unwrap_or_default();
            left_path
                .cmp(&right_path)
                .then(left.selection_range.start.cmp(&right.selection_range.start))
        });
        candidates.into_iter().next()
    }

    fn location_for_file_symbol(&self, file: FileId) -> Option<Location> {
        let symbols = self.analysis.document_symbols(file).ok()?;
        let symbol = symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::Document)
            .or_else(|| symbols.first())?;
        self.location_for_symbol(symbol.clone())
    }

    fn location_for_symbol(&self, symbol: dawn_analysis::DocumentSymbol) -> Option<Location> {
        let uri = self.uris.get(&symbol.file)?.clone();
        let line_index = self.line_index_for_file(symbol.file)?;
        Some(Location::new(uri, line_index.range(symbol.selection_range)))
    }

    fn line_index_for_file(&self, file: FileId) -> Option<LineIndex> {
        if let Some(document) = self
            .documents
            .values()
            .find(|document| document.file == file)
        {
            return Some(document.line_index.clone());
        }
        let text = self.analysis.file(file).ok()?.text().to_string();
        Some(LineIndex::new(&text))
    }

    fn sorted_workspace_files(&self) -> Vec<FileId> {
        let mut entries = self
            .paths
            .iter()
            .map(|(path, file)| (path.clone(), *file))
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        entries.into_iter().map(|(_, file)| file).collect()
    }
}

#[derive(Debug, Clone)]
struct OpenDocument {
    file: FileId,
    text: String,
    version: i32,
    line_index: LineIndex,
    open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    text: String,
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }

        Self {
            text: text.to_string(),
            line_starts,
        }
    }

    pub fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len());
        let line = self
            .line_starts
            .partition_point(|line_start| *line_start <= offset)
            .saturating_sub(1);
        let line_start = self.line_starts[line];
        let character = self.text[line_start..offset]
            .encode_utf16()
            .count()
            .try_into()
            .unwrap_or(u32::MAX);

        Position {
            line: line.try_into().unwrap_or(u32::MAX),
            character,
        }
    }

    pub fn offset(&self, position: Position) -> Option<usize> {
        let line = usize::try_from(position.line).ok()?;
        let line_start = *self.line_starts.get(line)?;
        let line_end = self.line_end(line);
        let character = usize::try_from(position.character).ok()?;
        let mut utf16 = 0;
        for (relative_offset, char) in self.text[line_start..line_end].char_indices() {
            if utf16 == character {
                return Some(line_start + relative_offset);
            }
            utf16 += char.len_utf16();
            if utf16 > character {
                return None;
            }
        }
        (utf16 == character).then_some(line_end)
    }

    pub fn line_end(&self, line: usize) -> usize {
        let next_line_start = self
            .line_starts
            .get(line + 1)
            .copied()
            .unwrap_or(self.text.len());
        let bytes = self.text.as_bytes();
        if next_line_start > 1
            && bytes.get(next_line_start - 1) == Some(&b'\n')
            && bytes.get(next_line_start - 2) == Some(&b'\r')
        {
            next_line_start - 2
        } else if next_line_start > 0 && bytes.get(next_line_start - 1) == Some(&b'\n') {
            next_line_start - 1
        } else {
            next_line_start
        }
    }

    pub fn range(&self, range: Range<usize>) -> LspRange {
        LspRange {
            start: self.position(range.start),
            end: self.position(range.end),
        }
    }
}

pub fn convert_diagnostic(
    line_index: &LineIndex,
    diagnostic: AnalysisDiagnostic,
) -> tower_lsp::lsp_types::Diagnostic {
    let range = diagnostic.range.unwrap_or(0..0);
    tower_lsp::lsp_types::Diagnostic {
        range: line_index.range(range),
        severity: Some(match diagnostic.severity {
            DiagnosticSeverity::Error => tower_lsp::lsp_types::DiagnosticSeverity::ERROR,
            DiagnosticSeverity::Warning => tower_lsp::lsp_types::DiagnosticSeverity::WARNING,
            DiagnosticSeverity::Info => tower_lsp::lsp_types::DiagnosticSeverity::INFORMATION,
        }),
        code: Some(tower_lsp::lsp_types::NumberOrString::String(
            diagnostic_code(&diagnostic.code).to_string(),
        )),
        code_description: None,
        source: Some(DIAGNOSTIC_SOURCE.to_string()),
        message: diagnostic.message,
        related_information: None,
        tags: None,
        data: None,
    }
}

fn diagnostic_code(code: &DiagnosticCode) -> &'static str {
    match code {
        DiagnosticCode::InvalidDocument => "invalid-document",
        DiagnosticCode::InvalidImportPath => "invalid-import-path",
        DiagnosticCode::UnresolvedImport => "unresolved-import",
    }
}

fn symbol_kind(kind: SymbolKind) -> LspSymbolKind {
    match kind {
        SymbolKind::Document => LspSymbolKind::FILE,
        SymbolKind::Import => LspSymbolKind::MODULE,
        SymbolKind::Function => LspSymbolKind::FUNCTION,
        SymbolKind::Parameter | SymbolKind::Let => LspSymbolKind::VARIABLE,
        SymbolKind::Command => LspSymbolKind::METHOD,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SemanticKind {
    Keyword = 0,
    Comment = 1,
    String = 2,
    Number = 3,
    Operator = 4,
    Type = 5,
    Class = 6,
    Function = 7,
    Method = 8,
    Parameter = 9,
    Variable = 10,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemanticRange {
    range: Range<usize>,
    kind: SemanticKind,
}

fn semantic_token_ranges(analysis: &Analysis, file: FileId, text: &str) -> Vec<SemanticRange> {
    let mut ranges = Vec::new();

    if let Ok(imports) = analysis.imports(file) {
        for import in imports {
            if let Some(path_range) = import.path_range {
                ranges.push(SemanticRange {
                    range: path_range,
                    kind: SemanticKind::String,
                });
            }
        }
    }

    if let Ok(symbols) = analysis.document_symbols(file) {
        ranges.extend(symbols.into_iter().map(|symbol| SemanticRange {
            range: symbol.selection_range,
            kind: match symbol.kind {
                SymbolKind::Document | SymbolKind::Import => SemanticKind::Class,
                SymbolKind::Function => SemanticKind::Function,
                SymbolKind::Command => SemanticKind::Method,
                SymbolKind::Parameter => SemanticKind::Parameter,
                SymbolKind::Let => SemanticKind::Variable,
            },
        }));
    }

    for token in lex_for_semantics(text) {
        if ranges
            .iter()
            .any(|range| ranges_overlap(&range.range, &token.range))
        {
            continue;
        }
        ranges.push(token);
    }

    ranges.retain(|range| range.range.start < range.range.end);
    ranges.sort_by(|left, right| {
        left.range
            .start
            .cmp(&right.range.start)
            .then(left.range.end.cmp(&right.range.end))
    });

    let mut non_overlapping: Vec<SemanticRange> = Vec::new();
    for range in ranges {
        if non_overlapping
            .last()
            .map(|last| last.range.end <= range.range.start)
            .unwrap_or(true)
        {
            non_overlapping.push(range);
        }
    }
    non_overlapping
}

fn lex_for_semantics(text: &str) -> Vec<SemanticRange> {
    let mut ranges = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        let byte = bytes[index];
        if byte == b'#' {
            let end = text[index..]
                .find('\n')
                .map(|relative| index + relative)
                .unwrap_or(text.len());
            ranges.push(SemanticRange {
                range: index..end,
                kind: if looks_like_color(&text[index..end]) {
                    SemanticKind::String
                } else {
                    SemanticKind::Comment
                },
            });
            index = end;
        } else if byte == b'"' {
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index += 2;
                } else if bytes[index] == b'"' {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
            ranges.push(SemanticRange {
                range: start..index.min(text.len()),
                kind: SemanticKind::String,
            });
        } else if byte.is_ascii_digit() {
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_digit() || matches!(bytes[index], b'.' | b'_'))
            {
                index += 1;
            }
            if index < bytes.len() && bytes[index].is_ascii_alphabetic() {
                while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
                    index += 1;
                }
            }
            ranges.push(SemanticRange {
                range: start..index,
                kind: SemanticKind::Number,
            });
        } else if byte.is_ascii_alphabetic() || byte == b'_' {
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            let word = &text[start..index];
            if KEYWORD_COMPLETIONS.contains(&word) {
                ranges.push(SemanticRange {
                    range: start..index,
                    kind: SemanticKind::Keyword,
                });
            } else if TYPE_COMPLETIONS.contains(&word) {
                ranges.push(SemanticRange {
                    range: start..index,
                    kind: SemanticKind::Type,
                });
            }
        } else {
            index += 1;
            if matches!(byte, b':' | b'-' | b'{' | b'}' | b'[' | b']' | b',' | b'.') {
                ranges.push(SemanticRange {
                    range: start..index,
                    kind: SemanticKind::Operator,
                });
            }
        }
    }
    ranges
}

fn looks_like_color(text: &str) -> bool {
    let color = text.trim();
    color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn encode_semantic_tokens(
    line_index: &LineIndex,
    text: &str,
    ranges: Vec<SemanticRange>,
) -> SemanticTokens {
    let mut absolute = Vec::new();
    for token in ranges {
        let mut start = token.range.start;
        while start < token.range.end {
            let position = line_index.position(start);
            let line = position.line as usize;
            let line_end = line_index.line_end(line).min(token.range.end);
            if start < line_end {
                let length = text[start..line_end].encode_utf16().count() as u32;
                absolute.push((position.line, position.character, length, token.kind as u32));
            }
            if line_end >= token.range.end {
                break;
            }
            start = line_index
                .line_starts
                .get(line + 1)
                .copied()
                .unwrap_or(token.range.end);
        }
    }

    absolute.sort();
    let mut previous_line = 0;
    let mut previous_start = 0;
    let data = absolute
        .into_iter()
        .map(|(line, start, length, token_type)| {
            let delta_line = line - previous_line;
            let delta_start = if delta_line == 0 {
                start - previous_start
            } else {
                start
            };
            previous_line = line;
            previous_start = start;
            SemanticToken {
                delta_line,
                delta_start,
                length,
                token_type,
                token_modifiers_bitset: 0,
            }
        })
        .collect();

    SemanticTokens {
        result_id: None,
        data,
    }
}

fn range_contains(range: &Range<usize>, offset: usize) -> bool {
    range.start <= offset && offset <= range.end
}

fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

fn completion_item(label: &str, kind: CompletionItemKind) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        ..CompletionItem::default()
    }
}

fn symbol_completion_item(symbol: dawn_analysis::DocumentSymbol) -> CompletionItem {
    completion_item(
        &symbol.name,
        match symbol.kind {
            SymbolKind::Document => CompletionItemKind::CLASS,
            SymbolKind::Import => CompletionItemKind::MODULE,
            SymbolKind::Function => CompletionItemKind::FUNCTION,
            SymbolKind::Command => CompletionItemKind::METHOD,
            SymbolKind::Parameter | SymbolKind::Let => CompletionItemKind::VARIABLE,
        },
    )
}

fn symbol_hover_label(kind: SymbolKind, name: &str) -> String {
    let kind = match kind {
        SymbolKind::Document => "document",
        SymbolKind::Import => "import",
        SymbolKind::Function => "function",
        SymbolKind::Parameter => "parameter",
        SymbolKind::Let => "let",
        SymbolKind::Command => "command",
    };
    format!("{kind} `{name}`")
}

fn markdown_hover(contents: String, range: Option<LspRange>) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: contents,
        }),
        range,
    }
}

#[derive(Debug, Clone)]
struct WordToken {
    text: String,
}

fn token_at(text: &str, offset: usize) -> Option<WordToken> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            let range = start..index;
            if range_contains(&range, offset) {
                return Some(WordToken {
                    text: text[range.clone()].to_string(),
                });
            }
        } else {
            index += 1;
        }
    }
    None
}

fn relative_label(base: &Path, path: &Path) -> String {
    if let Ok(stripped) = path.strip_prefix(base) {
        return stripped.to_string_lossy().replace('\\', "/");
    }
    path.to_string_lossy().replace('\\', "/")
}

fn semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_LEGEND
            .iter()
            .map(|token| SemanticTokenType::new(token))
            .collect(),
        token_modifiers: Vec::new(),
    }
}

fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        document_symbol_provider: Some(OneOf::Left(true)),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                work_done_progress_options: Default::default(),
                legend: semantic_tokens_legend(),
                range: None,
                full: Some(SemanticTokensFullOptions::Bool(true)),
            },
        )),
        completion_provider: Some(CompletionOptions::default()),
        hover_provider: Some(tower_lsp::lsp_types::HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        ..ServerCapabilities::default()
    }
}

pub struct Backend {
    client: Client,
    state: Arc<Mutex<LspState>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            state: Arc::new(Mutex::new(LspState::new())),
        }
    }

    async fn publish_diagnostics(
        &self,
        uri: Url,
        diagnostics: Vec<tower_lsp::lsp_types::Diagnostic>,
    ) {
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }
}

#[async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> JsonRpcResult<InitializeResult> {
        self.state.lock().await.initialize_workspace(&params);

        Ok(InitializeResult {
            capabilities: server_capabilities(),
            server_info: Some(ServerInfo {
                name: "dawn-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Dawn language server initialized")
            .await;
    }

    async fn shutdown(&self) -> JsonRpcResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let diagnostics = self.state.lock().await.did_open(
            uri.clone(),
            params.text_document.version,
            params.text_document.text,
        );
        self.publish_diagnostics(uri, diagnostics).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let Some(change) = params
            .content_changes
            .into_iter()
            .rev()
            .find(|change| change.range.is_none())
        else {
            return;
        };

        let diagnostics = self
            .state
            .lock()
            .await
            .did_change(&uri, version, change.text);
        self.publish_diagnostics(uri, diagnostics).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.state.lock().await.did_close(&params.text_document.uri);
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> JsonRpcResult<Option<DocumentSymbolResponse>> {
        let symbols = self
            .state
            .lock()
            .await
            .document_symbols(&params.text_document.uri);
        Ok(symbols.map(DocumentSymbolResponse::Nested))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> JsonRpcResult<Option<SemanticTokensResult>> {
        let tokens = self
            .state
            .lock()
            .await
            .semantic_tokens(&params.text_document.uri);
        Ok(tokens.map(SemanticTokensResult::Tokens))
    }

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> JsonRpcResult<Option<CompletionResponse>> {
        let completion = self.state.lock().await.completion(
            &params.text_document_position.text_document.uri,
            params.text_document_position.position,
        );
        Ok(completion)
    }

    async fn hover(&self, params: HoverParams) -> JsonRpcResult<Option<Hover>> {
        let hover = self.state.lock().await.hover(
            &params.text_document_position_params.text_document.uri,
            params.text_document_position_params.position,
        );
        Ok(hover)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> JsonRpcResult<Option<GotoDefinitionResponse>> {
        let definition = self.state.lock().await.definition(
            &params.text_document_position_params.text_document.uri,
            params.text_document_position_params.position,
        );
        Ok(definition)
    }
}

fn workspace_roots(params: &InitializeParams) -> Vec<PathBuf> {
    if let Some(folders) = &params.workspace_folders {
        let roots = folders
            .iter()
            .filter_map(|folder| uri_to_normalized_path(&folder.uri))
            .collect::<Vec<_>>();
        if !roots.is_empty() {
            return roots;
        }
    }

    params
        .root_uri
        .as_ref()
        .and_then(uri_to_normalized_path)
        .into_iter()
        .collect()
}

fn uri_to_normalized_path(uri: &Url) -> Option<PathBuf> {
    uri.to_file_path().ok().map(normalize_path)
}

fn is_ignored_dir(entry: &DirEntry) -> bool {
    entry.file_type().is_dir()
        && entry
            .file_name()
            .to_str()
            .map(|name| IGNORED_DIRS.contains(&name))
            .unwrap_or(false)
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
    use dawn_analysis::DiagnosticSource;
    use tower_lsp::lsp_types::{DiagnosticSeverity as LspDiagnosticSeverity, NumberOrString};

    fn file_uri(path: &Path) -> Url {
        Url::from_file_path(path).unwrap()
    }

    #[test]
    fn line_index_converts_ascii_ranges() {
        let index = LineIndex::new("effect Main {}");
        assert_eq!(
            index.range(7..11),
            LspRange::new(Position::new(0, 7), Position::new(0, 11))
        );
    }

    #[test]
    fn line_index_converts_multiline_ranges() {
        let index = LineIndex::new("one\ntwo\nthree");
        assert_eq!(
            index.range(4..13),
            LspRange::new(Position::new(1, 0), Position::new(2, 5))
        );
    }

    #[test]
    fn line_index_converts_crlf_input() {
        let index = LineIndex::new("one\r\ntwo");
        assert_eq!(index.position(5), Position::new(1, 0));
        assert_eq!(index.line_end(0), 3);
    }

    #[test]
    fn line_index_converts_utf8_to_utf16_positions() {
        let text = format!("a{}b\n{}z", '\u{1F600}', '\u{00E9}');
        let index = LineIndex::new(&text);
        assert_eq!(index.position("a\u{1F600}".len()), Position::new(0, 3));
        assert_eq!(
            index.position("a\u{1F600}b\n\u{00E9}".len()),
            Position::new(1, 1)
        );
    }

    #[test]
    fn line_index_converts_lsp_positions_to_offsets() {
        let text = "one\r\na😀b\nlast";
        let index = LineIndex::new(text);

        assert_eq!(index.offset(Position::new(1, 0)), Some(5));
        assert_eq!(index.offset(Position::new(1, 1)), Some(6));
        assert_eq!(index.offset(Position::new(1, 3)), Some(10));
        assert_eq!(index.offset(Position::new(1, 2)), None);
    }

    #[test]
    fn diagnostic_conversion_uses_fallback_range() {
        let diagnostic = AnalysisDiagnostic {
            file: FileIdForTest::id(),
            message: "missing document".to_string(),
            severity: DiagnosticSeverity::Error,
            range: None,
            source: DiagnosticSource::Analysis,
            code: DiagnosticCode::InvalidDocument,
        };

        let converted = convert_diagnostic(&LineIndex::new(""), diagnostic);
        assert_eq!(
            converted.range,
            LspRange::new(Position::new(0, 0), Position::new(0, 0))
        );
        assert_eq!(
            converted.code,
            Some(NumberOrString::String("invalid-document".to_string()))
        );
    }

    #[test]
    fn diagnostic_conversion_maps_severity_and_codes() {
        let cases = [
            (
                DiagnosticSeverity::Error,
                LspDiagnosticSeverity::ERROR,
                DiagnosticCode::UnresolvedImport,
                "unresolved-import",
            ),
            (
                DiagnosticSeverity::Warning,
                LspDiagnosticSeverity::WARNING,
                DiagnosticCode::InvalidImportPath,
                "invalid-import-path",
            ),
            (
                DiagnosticSeverity::Info,
                LspDiagnosticSeverity::INFORMATION,
                DiagnosticCode::InvalidDocument,
                "invalid-document",
            ),
        ];

        for (severity, expected_severity, code, expected_code) in cases {
            let converted = convert_diagnostic(
                &LineIndex::new("abc"),
                AnalysisDiagnostic {
                    file: FileIdForTest::id(),
                    message: "message".to_string(),
                    severity,
                    range: Some(0..1),
                    source: DiagnosticSource::Analysis,
                    code,
                },
            );
            assert_eq!(converted.severity, Some(expected_severity));
            assert_eq!(
                converted.code,
                Some(NumberOrString::String(expected_code.to_string()))
            );
            assert_eq!(converted.source, Some("dawn".to_string()));
        }
    }

    #[test]
    fn workspace_scan_registers_dawn_files_and_skips_ignored_dirs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir_all(root.join("effects")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(
            root.join("main.effect.dawn"),
            "import effect Pulse from <effects/pulse.effect.dawn>;\neffect Main {}",
        )
        .unwrap();
        std::fs::write(root.join("effects/pulse.effect.dawn"), "effect Pulse {}").unwrap();
        std::fs::write(root.join("target/ignored.effect.dawn"), "").unwrap();

        let uri = file_uri(&root.join("main.effect.dawn"));
        let mut state = LspState::new();
        state.scan_workspace_root(root);
        state.did_open(
            uri.clone(),
            1,
            std::fs::read_to_string(root.join("main.effect.dawn")).unwrap(),
        );

        let file = state.file_for_uri(&uri).unwrap();
        assert!(state.imports_resolve(file));
        assert!(state.diagnostics_for_file(file).is_empty());
    }

    #[test]
    fn open_change_and_symbols_flow_updates_state() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("project.dawn");
        let uri = file_uri(&path);
        let mut state = LspState::new();

        let invalid = state.did_open(uri.clone(), 1, "club:\n  type: project\n".to_string());
        assert!(!invalid.is_empty());

        let valid_text = "club:\n  type: project\n  name: club\n  display:\n    import: main.display.dawn::main\n".to_string();
        state
            .analysis
            .set_file(temp.path().join("main.display.dawn"), "main:\n  type: display\n  name: main\n  layout:\n    import: stage.layout.dawn::stage\n  patch:\n    import: house.patch.dawn::house\n");
        let valid = state.did_change(&uri, 2, valid_text);
        assert!(valid.is_empty());

        let symbols = state.document_symbols(&uri).unwrap();
        assert!(symbols.iter().any(|symbol| symbol.name == "club"));

        state.did_close(&uri);
        assert_eq!(state.documents.get(&uri).unwrap().version, 2);
        assert!(!state.documents.get(&uri).unwrap().open);
    }

    #[test]
    fn semantic_tokens_map_roles_and_split_multiline_comments() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("project.dawn");
        let uri = file_uri(&path);
        let text = "# one\nclub:\n  type: project\n  name: club\n  display:\n    import: displays/main.display.dawn::main\n";
        let mut state = LspState::new();
        state.did_open(uri.clone(), 1, text.to_string());

        let tokens = state.semantic_tokens(&uri).unwrap();
        assert!(tokens
            .data
            .iter()
            .any(|token| token.token_type == SemanticKind::Comment as u32));
        assert!(tokens
            .data
            .iter()
            .any(|token| token.token_type == SemanticKind::Class as u32));
    }

    #[test]
    fn semantic_tokens_treat_import_path_as_single_string_range() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("project.dawn");
        let uri = file_uri(&path);
        let text = "club:\n  type: project\n  name: club\n  display:\n    import: displays/main.display.dawn::main\n";
        let mut state = LspState::new();
        state.did_open(uri.clone(), 1, text.to_string());

        let path_start = text.find("displays").unwrap();
        let path_position = LineIndex::new(text).position(path_start);
        let tokens = state.semantic_tokens(&uri).unwrap();
        let mut line = 0;
        let mut start = 0;
        let mut string_tokens = 0;
        for token in tokens.data {
            line += token.delta_line;
            start = if token.delta_line == 0 {
                start + token.delta_start
            } else {
                token.delta_start
            };
            if line == path_position.line && start == path_position.character {
                assert_eq!(token.token_type, SemanticKind::String as u32);
                string_tokens += 1;
            }
        }

        assert_eq!(string_tokens, 1);
    }

    #[test]
    fn completion_returns_keywords_symbols_and_import_paths() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir_all(root.join("displays")).unwrap();
        let main_path = root.join("project.dawn");
        let target_path = root.join("displays/main.display.dawn");
        std::fs::write(&target_path, "main:\n  type: display\n  name: main\n  layout:\n    import: ../layouts/stage.layout.dawn::stage\n  patch:\n    import: ../patches/house.patch.dawn::house\n").unwrap();
        let text = "club:\n  type: project\n  name: club\n  display:\n    import: displays/main.display.dawn::main\n";
        let uri = file_uri(&main_path);
        let mut state = LspState::new();
        state.scan_workspace_root(root);
        state.did_open(uri.clone(), 1, text.to_string());

        let top = state
            .completion(&uri, Position::new(2, 8))
            .and_then(|response| match response {
                CompletionResponse::Array(items) => Some(items),
                _ => None,
            })
            .unwrap();
        assert!(top.iter().any(|item| item.label == "project"));
        assert!(top.iter().any(|item| item.label == "club"));

        let path_items = state
            .completion(&uri, Position::new(4, 12))
            .and_then(|response| match response {
                CompletionResponse::Array(items) => Some(items),
                _ => None,
            })
            .unwrap();
        assert!(path_items
            .iter()
            .any(|item| item.label == "displays/main.display.dawn"));
    }

    #[test]
    fn hover_describes_symbols_and_import_paths() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir_all(root.join("displays")).unwrap();
        std::fs::write(root.join("displays/main.display.dawn"), "main:\n  type: display\n  name: main\n  layout:\n    import: ../layouts/stage.layout.dawn::stage\n  patch:\n    import: ../patches/house.patch.dawn::house\n").unwrap();
        let main_path = root.join("project.dawn");
        let uri = file_uri(&main_path);
        let text = "club:\n  type: project\n  name: club\n  display:\n    import: displays/main.display.dawn::main\n";
        let mut state = LspState::new();
        state.scan_workspace_root(root);
        state.did_open(uri.clone(), 1, text.to_string());

        let symbol_hover = state.hover(&uri, Position::new(2, 8)).unwrap();
        assert!(hover_text(&symbol_hover).contains("document `club`"));

        let import_hover = state.hover(&uri, Position::new(4, 12)).unwrap();
        assert!(hover_text(&import_hover).contains("import target"));
    }

    #[test]
    fn definition_resolves_imports_declarations_and_name_refs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir_all(root.join("displays")).unwrap();
        let target_path = root.join("displays/main.display.dawn");
        std::fs::write(&target_path, "main:\n  type: display\n  name: main\n  layout:\n    import: ../layouts/stage.layout.dawn::stage\n  patch:\n    import: ../patches/house.patch.dawn::house\n").unwrap();
        let main_path = root.join("project.dawn");
        let uri = file_uri(&main_path);
        let text = "club:\n  type: project\n  name: club\n  display:\n    import: displays/main.display.dawn::main\n";
        let mut state = LspState::new();
        state.scan_workspace_root(root);
        state.did_open(uri.clone(), 1, text.to_string());

        let import_definition = state.definition(&uri, Position::new(4, 12)).unwrap();
        let GotoDefinitionResponse::Scalar(import_location) = import_definition else {
            panic!("expected scalar definition");
        };
        assert_eq!(import_location.uri, file_uri(&target_path));

        let declaration_definition = state.definition(&uri, Position::new(2, 8)).unwrap();
        let GotoDefinitionResponse::Scalar(declaration_location) = declaration_definition else {
            panic!("expected scalar definition");
        };
        assert_eq!(declaration_location.uri, uri);
        assert_eq!(declaration_location.range.start, Position::new(0, 0));
    }

    #[test]
    fn initialize_capability_helpers_expose_semantic_legend() {
        let capabilities = server_capabilities();
        assert_eq!(
            capabilities.text_document_sync,
            Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL))
        );
        assert!(capabilities.completion_provider.is_some());
        assert!(capabilities.hover_provider.is_some());
        assert_eq!(capabilities.definition_provider, Some(OneOf::Left(true)));
        assert_eq!(
            capabilities.document_symbol_provider,
            Some(OneOf::Left(true))
        );
        assert!(capabilities.semantic_tokens_provider.is_some());

        let legend = semantic_tokens_legend();
        assert_eq!(
            legend.token_types,
            TOKEN_LEGEND
                .iter()
                .map(|token| SemanticTokenType::new(token))
                .collect::<Vec<_>>()
        );
    }

    fn hover_text(hover: &Hover) -> &str {
        match &hover.contents {
            HoverContents::Markup(markup) => &markup.value,
            _ => "",
        }
    }

    struct FileIdForTest;

    impl FileIdForTest {
        fn id() -> FileId {
            let mut analysis = Analysis::new();
            analysis.set_file("test.effect.dawn", "effect Test {}")
        }
    }
}
