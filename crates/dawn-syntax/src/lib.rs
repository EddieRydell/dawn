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
                SyntaxKind::Duration,
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
    fn duration_and_range_tokens_are_distinct() {
        assert_eq!(
            token_kinds("45s 1..510"),
            vec![
                SyntaxKind::Duration,
                SyntaxKind::Int,
                SyntaxKind::DotDot,
                SyntaxKind::Int,
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
    fn effect_function_body_parses_as_generic_items_and_expressions() {
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

        let fn_decl = document
            .block()
            .unwrap()
            .items()
            .into_iter()
            .find_map(|item| match item.kind()? {
                ast::ItemKind::FnDecl(fn_decl) => Some(fn_decl),
                _ => None,
            })
            .unwrap();

        let nested = fn_decl
            .body()
            .unwrap()
            .items()
            .into_iter()
            .filter_map(|item| item.kind())
            .collect::<Vec<_>>();
        assert!(matches!(nested[0], ast::ItemKind::LetStmt(_)));
        assert!(matches!(nested[1], ast::ItemKind::Command(_)));

        let let_stmt = match &nested[0] {
            ast::ItemKind::LetStmt(let_stmt) => let_stmt,
            _ => unreachable!(),
        };
        let value = let_stmt.value().unwrap();
        let ast::ExprKind::BinaryExpr(binary) = value.kind().unwrap() else {
            panic!("expected binary expression");
        };
        assert_eq!(binary.op().unwrap().kind(), SyntaxKind::Slash);
        assert!(binary.left().is_some());
        assert!(binary.right().is_some());

        let command = match &nested[1] {
            ast::ItemKind::Command(command) => command,
            _ => unreachable!(),
        };
        assert_eq!(command.head().unwrap().text().as_deref(), Some("color"));
    }

    #[test]
    fn labeled_name_accessors_are_position_aware() {
        let source =
            "import display MainDisplay from <displays/main.display.dawn>;\neffect Pulse {}";
        let parse = parse(source);
        assert_eq!(parse.diagnostics(), []);

        let file = parse.source_file().unwrap();
        let import = file.imports().into_iter().next().unwrap();
        assert_eq!(import.kind().unwrap().text().as_deref(), Some("display"));
        assert_eq!(
            import.name().unwrap().text().as_deref(),
            Some("MainDisplay")
        );

        let document = file.document().unwrap();
        assert_eq!(document.kind().unwrap().text().as_deref(), Some("effect"));
        assert_eq!(document.name().unwrap().text().as_deref(), Some("Pulse"));
    }

    #[test]
    fn params_keep_name_and_type_distinct() {
        let parse = parse("effect Pulse { fn sample(t float) {} }");
        assert_eq!(parse.diagnostics(), []);

        let document = parse.source_file().unwrap().document().unwrap();
        let fn_decl = document
            .block()
            .unwrap()
            .items()
            .into_iter()
            .find_map(|item| match item.kind()? {
                ast::ItemKind::FnDecl(fn_decl) => Some(fn_decl),
                _ => None,
            })
            .unwrap();
        let param = fn_decl
            .params()
            .unwrap()
            .params()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(param.name().unwrap().text().as_deref(), Some("t"));
        assert_eq!(
            param.ty().unwrap().name().unwrap().text().as_deref(),
            Some("float")
        );
    }

    #[test]
    fn generated_literal_and_path_accessors_return_content() {
        let parse = parse("import display MainDisplay from <displays/main.display.dawn>;\neffect Pulse { color true false; }");
        assert_eq!(parse.diagnostics(), []);

        let file = parse.source_file().unwrap();
        let path = file.imports().into_iter().next().unwrap().path().unwrap();
        assert_eq!(
            path.raw_text().as_deref(),
            Some("displays/main.display.dawn")
        );
        assert_eq!(
            path.tokens()
                .into_iter()
                .map(|token| token.text().to_string())
                .collect::<String>(),
            "displays/main.display.dawn"
        );

        let command = match file
            .document()
            .unwrap()
            .block()
            .unwrap()
            .items()
            .into_iter()
            .next()
            .unwrap()
            .kind()
            .unwrap()
        {
            ast::ItemKind::Command(command) => command,
            _ => unreachable!(),
        };
        let args = command.args();
        let ast::ExprKind::BoolLit(first) = args[0].kind().unwrap() else {
            panic!("expected bool literal");
        };
        let ast::ExprKind::BoolLit(second) = args[1].kind().unwrap() else {
            panic!("expected bool literal");
        };
        assert_eq!(first.token().unwrap().kind(), SyntaxKind::TrueKw);
        assert_eq!(first.text().as_deref(), Some("true"));
        assert_eq!(second.token().unwrap().kind(), SyntaxKind::FalseKw);
    }

    #[test]
    fn command_requires_block_or_semicolon_terminator() {
        assert!(!parse("effect Pulse { color true }")
            .diagnostics()
            .is_empty());
        assert_eq!(parse("effect Pulse { color true; }").diagnostics(), []);
        assert_eq!(parse("effect Pulse { color true {} }").diagnostics(), []);
    }

    #[test]
    fn expression_tree_preserves_precedence_calls_prefix_and_power_associativity() {
        fn first_arg(source: &str) -> ast::Expr {
            let parse = parse(source);
            assert_eq!(parse.diagnostics(), []);
            let command = match parse
                .source_file()
                .unwrap()
                .document()
                .unwrap()
                .block()
                .unwrap()
                .items()
                .into_iter()
                .next()
                .unwrap()
                .kind()
                .unwrap()
            {
                ast::ItemKind::Command(command) => command,
                _ => unreachable!(),
            };
            command.args().into_iter().next().unwrap()
        }

        let expr = first_arg("effect Pulse { color a + b * c; }");
        let ast::ExprKind::BinaryExpr(add) = expr.kind().unwrap() else {
            panic!("expected add");
        };
        assert_eq!(add.op().unwrap().kind(), SyntaxKind::Plus);
        assert!(matches!(
            add.right().unwrap().kind().unwrap(),
            ast::ExprKind::BinaryExpr(_)
        ));

        let expr = first_arg("effect Pulse { color (a + b) * c; }");
        let ast::ExprKind::BinaryExpr(mul) = expr.kind().unwrap() else {
            panic!("expected mul");
        };
        assert_eq!(mul.op().unwrap().kind(), SyntaxKind::Star);
        assert!(matches!(
            mul.left().unwrap().kind().unwrap(),
            ast::ExprKind::ParenExpr(_)
        ));

        let expr = first_arg("effect Pulse { color 1..510; }");
        let ast::ExprKind::BinaryExpr(range) = expr.kind().unwrap() else {
            panic!("expected range");
        };
        assert_eq!(range.op().unwrap().kind(), SyntaxKind::DotDot);

        let expr = first_arg("effect Pulse { color sin(t * speed); }");
        let ast::ExprKind::CallExpr(call) = expr.kind().unwrap() else {
            panic!("expected call");
        };
        assert!(matches!(
            call.callee().unwrap().kind().unwrap(),
            ast::ExprKind::NameRef(_)
        ));
        assert!(matches!(
            call.args()[0].kind().unwrap(),
            ast::ExprKind::BinaryExpr(_)
        ));

        let expr = first_arg("effect Pulse { color -sin(t); }");
        let ast::ExprKind::PrefixExpr(prefix) = expr.kind().unwrap() else {
            panic!("expected prefix");
        };
        assert_eq!(prefix.op().unwrap().kind(), SyntaxKind::Minus);
        assert!(matches!(
            prefix.expr().unwrap().kind().unwrap(),
            ast::ExprKind::CallExpr(_)
        ));

        let expr = first_arg("effect Pulse { color a ** b ** c; }");
        let ast::ExprKind::BinaryExpr(power) = expr.kind().unwrap() else {
            panic!("expected power");
        };
        assert_eq!(power.op().unwrap().kind(), SyntaxKind::StarStar);
        assert!(matches!(
            power.right().unwrap().kind().unwrap(),
            ast::ExprKind::BinaryExpr(_)
        ));
    }
}
