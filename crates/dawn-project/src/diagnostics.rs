use crate::path::Utf8PathBuf;

#[derive(Debug, Clone)]
pub struct ProjectLoadResult {
    pub project: Option<crate::model::DawnProject>,
    pub diagnostics: Vec<ProjectDiagnostic>,
}

#[derive(Debug, Clone)]
pub struct ProjectDiagnostic {
    pub severity: DiagnosticSeverity,
    pub file: Utf8PathBuf,
    pub range: Option<TextRange>,
    pub message: String,
    pub kind: ProjectDiagnosticKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectDiagnosticKind {
    Io,
    DawnSyntax,
    DawnSchema,
    Import,
    Lower,
    EffectScript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}
