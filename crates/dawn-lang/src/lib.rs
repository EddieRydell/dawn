pub mod project;

use std::marker::PhantomData;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn merge(self, other: Self) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRange {
    pub file_id: FileId,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticCode {
    Lex,
    Parse,
    Resolve,
    Type,
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub code: DiagnosticCode,
    pub path: PathBuf,
    pub span: Option<Span>,
    pub message: String,
}

impl Diagnostic {
    pub fn error(
        path: impl Into<PathBuf>,
        code: DiagnosticCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: DiagnosticSeverity::Error,
            code,
            path: path.into(),
            span: None,
            message: message.into(),
        }
    }

    pub fn at(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Name<TNamespace> {
    raw: String,
    _namespace: PhantomData<TNamespace>,
}

impl<TNamespace> Name<TNamespace> {
    pub fn new(raw: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
            _namespace: PhantomData,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SymbolRef<TNamespace> {
    raw: String,
    _namespace: PhantomData<TNamespace>,
}

impl<TNamespace> SymbolRef<TNamespace> {
    pub fn new(raw: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
            _namespace: PhantomData,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int,
    Float,
    Bool,
    String,
    Color,
    Duration,
    Array(Box<Type>),
    Record(RecordType),
    Enum(EnumType),
    Ref(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordType {
    pub fields: Vec<(String, Type)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumType {
    pub variants: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayType {
    pub item: Box<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RefType {
    pub namespace: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Color(u8, u8, u8),
    DurationSeconds(f64),
    Array(Vec<Value>),
    Record(Vec<(String, Value)>),
    EnumVariant(String),
    Ref(String),
}

pub mod language_service {
    use super::{Diagnostic, Span};

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct DocumentSymbol {
        pub name: String,
        pub kind: String,
        pub span: Span,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct TextEdit {
        pub span: Span,
        pub replacement: String,
    }

    #[derive(Debug, Clone, Default)]
    pub struct Analysis {
        pub diagnostics: Vec<Diagnostic>,
        pub symbols: Vec<DocumentSymbol>,
    }

    pub fn diagnostics(analysis: &Analysis) -> &[Diagnostic] {
        &analysis.diagnostics
    }

    pub fn document_symbols(analysis: &Analysis) -> &[DocumentSymbol] {
        &analysis.symbols
    }

    pub fn declaration_lookup(_analysis: &Analysis, _offset: usize) -> Option<Span> {
        None
    }

    pub fn references(_analysis: &Analysis, _offset: usize) -> Vec<Span> {
        Vec::new()
    }

    pub fn rename_edits(_analysis: &Analysis, _offset: usize, _new_name: &str) -> Vec<TextEdit> {
        Vec::new()
    }

    pub fn completions(_analysis: &Analysis, _offset: usize) -> Vec<String> {
        Vec::new()
    }

    pub fn format_document(source: &str) -> String {
        source.to_string()
    }
}
