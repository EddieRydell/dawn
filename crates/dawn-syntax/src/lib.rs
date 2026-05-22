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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;

    fn token_kinds(source: &str) -> Vec<SyntaxKind> {
        let (tokens, diagnostics) = lex(source);
        assert_eq!(diagnostics, []);
        tokens.into_iter().map(|token| token.kind).collect()
    }

    fn dawn_files(root: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(path) = pending.pop() {
            for entry in fs::read_dir(&path).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else if path
                    .extension()
                    .is_some_and(|extension| extension == "dawn")
                {
                    files.push(path);
                }
            }
        }
        files.sort();
        files
    }

    #[test]
    fn domain_words_lex_as_identifiers() {
        assert_eq!(
            token_kinds("project display hardware fixture duration color sequence 45s"),
            vec![
                SyntaxKind::Ident,
                SyntaxKind::Ident,
                SyntaxKind::Ident,
                SyntaxKind::Ident,
                SyntaxKind::Ident,
                SyntaxKind::Ident,
                SyntaxKind::Ident,
                SyntaxKind::Int,
                SyntaxKind::Ident,
            ]
        );
    }

    #[test]
    fn import_and_from_remain_hard_keywords() {
        assert_eq!(
            token_kinds("import display MainDisplay from <displays/main.display.dawn>;"),
            vec![
                SyntaxKind::ImportKw,
                SyntaxKind::Ident,
                SyntaxKind::Ident,
                SyntaxKind::FromKw,
                SyntaxKind::Lt,
                SyntaxKind::Ident,
                SyntaxKind::Slash,
                SyntaxKind::Ident,
                SyntaxKind::Dot,
                SyntaxKind::Ident,
                SyntaxKind::Dot,
                SyntaxKind::Ident,
                SyntaxKind::Gt,
                SyntaxKind::Semicolon,
            ]
        );
    }

    #[test]
    fn club_rig_examples_parse_without_diagnostics() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/club-rig");
        let files = dawn_files(&root);
        assert!(!files.is_empty());

        for file in files {
            let source = fs::read_to_string(&file).unwrap();
            let parse = parse(&source);
            assert_eq!(parse.diagnostics(), [], "{}", file.display());
        }
    }

    #[test]
    fn effect_function_body_parses_as_generic_statements() {
        let source = r#"effect Pulse {
  fn sample(t float, fixture Fixture) {
    let phase = (sin(t * speed) + 1.0) / 2.0;
    color mix(base, accent, phase);
  }
}"#;

        let parse = parse(source);
        assert_eq!(parse.diagnostics(), []);

        let file = parse.source_file().unwrap();
        let document = file.document().unwrap();
        assert_eq!(document.kind().unwrap().text().as_deref(), Some("effect"));

        let fn_stmt = document
            .body()
            .unwrap()
            .items()
            .into_iter()
            .find_map(|item| item.stmt())
            .unwrap();
        assert_eq!(fn_stmt.head().unwrap().text().as_deref(), Some("fn"));

        let nested_heads = fn_stmt
            .body()
            .unwrap()
            .items()
            .into_iter()
            .filter_map(|item| item.stmt())
            .map(|stmt| stmt.head().unwrap().text().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(nested_heads, ["let", "color"]);
    }
}
