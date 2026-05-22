use std::ops::Range;

pub use crate::generated::diagnostic::DiagnosticKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub range: Range<usize>,
    pub message: String,
}

impl Diagnostic {
    pub fn new(kind: DiagnosticKind, range: Range<usize>, message: impl Into<String>) -> Self {
        Self {
            kind,
            range,
            message: message.into(),
        }
    }
}
