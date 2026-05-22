pub mod diagnostic;
pub mod generated;

use rowan::GreenNode;

pub use diagnostic::{Diagnostic, DiagnosticKind};
pub use generated::ast;
pub use generated::ast::AstNode;
pub use generated::kind::{DawnLanguage, SyntaxKind};
pub use generated::lexer::{lex, LexToken};
pub use generated::parser::parse_green;

pub type SyntaxNode = rowan::SyntaxNode<DawnLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<DawnLanguage>;
pub type SyntaxElement = rowan::SyntaxElement<DawnLanguage>;

#[derive(Debug, Clone)]
pub struct Parse {
    green: GreenNode,
    diagnostics: Vec<Diagnostic>,
}

impl Parse {
    pub fn syntax_node(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    pub fn root(&self) -> SyntaxNode {
        self.syntax_node()
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn source_file(&self) -> Option<ast::SourceFile> {
        ast::SourceFile::cast(self.syntax_node())
    }
}

pub fn parse(source: &str) -> Parse {
    let (tokens, mut diagnostics) = lex(source);
    let (green, mut parse_diagnostics) = parse_green(tokens);
    diagnostics.append(&mut parse_diagnostics);
    Parse { green, diagnostics }
}
