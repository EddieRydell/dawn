use super::ast::Span;

/// A compilation error with source location.
#[derive(Debug, Clone)]
pub struct CompileError {
    pub message: String,
    pub span: Span,
    pub kind: ErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Lexer,
    Parser,
    Type,
    Compiler,
}

impl CompileError {
    pub fn lexer(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            kind: ErrorKind::Lexer,
        }
    }

    pub fn parser(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            kind: ErrorKind::Parser,
        }
    }

    pub fn type_error(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            kind: ErrorKind::Type,
        }
    }

    pub fn compiler(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            kind: ErrorKind::Compiler,
        }
    }

    /// Format the error with source context.
    pub fn format_with_source(&self, source: &str) -> String {
        let (line, col) = offset_to_line_col(source, self.span.start);
        format!(
            "[{}] line {}:{}: {}",
            match self.kind {
                ErrorKind::Lexer => "lexer",
                ErrorKind::Parser => "parser",
                ErrorKind::Type => "type",
                ErrorKind::Compiler => "compiler",
            },
            line,
            col,
            self.message,
        )
    }
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CompileError {}

fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}
