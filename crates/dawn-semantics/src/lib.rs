pub mod generated;

pub use generated::diagnostic::{LowerDiagnostic, LowerDiagnosticKind};
pub use generated::hir;
pub use generated::lower::{lower_parse, lower_source_file, LoweredSourceFile};

#[cfg(test)]
mod tests {
    use dawn_syntax::parse;

    use super::*;

    fn lower(source: &str) -> LoweredSourceFile {
        let parse = parse(source);
        assert_eq!(parse.diagnostics(), []);
        lower_parse(&parse)
    }

    fn range_text<'a>(source: &'a str, range: &std::ops::Range<usize>) -> &'a str {
        &source[range.clone()]
    }

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn lowers_import_and_document() {
        let source =
            "import display MainDisplay from <displays/main.display.dawn>;\neffect Pulse {}";
        let lowered = lower(source);
        assert_eq!(lowered.diagnostics, []);

        let root = lowered.root.unwrap();
        assert_eq!(range_text(source, &root.range), source);
        assert_eq!(root.imports[0].kind.text, "display");
        assert_eq!(root.imports[0].name.text, "MainDisplay");
        assert_eq!(root.imports[0].path.raw_text, "displays/main.display.dawn");
        assert_eq!(range_text(source, &root.imports[0].kind.range), "display");
        assert_eq!(
            range_text(source, &root.imports[0].name.range),
            "MainDisplay"
        );
        assert_eq!(
            range_text(source, root.imports[0].path.inner_range.as_ref().unwrap()),
            "displays/main.display.dawn"
        );
        assert_eq!(root.document.kind.text, "effect");
        assert_eq!(root.document.name.text, "Pulse");
        assert_eq!(range_text(source, &root.document.kind.range), "effect");
        assert_eq!(range_text(source, &root.document.name.range), "Pulse");
        assert!(root.document.block.items.is_empty());
    }

    #[test]
    fn lowers_functions_params_lets_and_commands() {
        let source = r#"effect Pulse {
  fn sample(t float) color {
    let phase float = 1.0;
    color base = accent {}
  }
}"#;
        let lowered = lower(source);
        assert_eq!(lowered.diagnostics, []);

        let root = lowered.root.unwrap();
        let hir::Item::FnDecl(function) = &root.document.block.items[0] else {
            panic!("expected function");
        };
        assert_eq!(function.name.text, "sample");
        assert!(range_text(source, &function.range).starts_with("fn sample"));
        assert_eq!(range_text(source, &function.name.range), "sample");
        assert_eq!(function.params[0].name.text, "t");
        assert_eq!(function.params[0].ty.as_ref().unwrap().name.text, "float");
        assert_eq!(range_text(source, &function.params[0].range), "t float");
        assert_eq!(range_text(source, &function.params[0].name.range), "t");
        assert_eq!(function.return_type.as_ref().unwrap().name.text, "color");

        let hir::Item::LetStmt(let_stmt) = &function.body.items[0] else {
            panic!("expected let");
        };
        assert_eq!(let_stmt.name.text, "phase");
        assert!(range_text(source, &let_stmt.range).starts_with("let phase"));
        assert_eq!(range_text(source, &let_stmt.name.range), "phase");
        assert!(let_stmt.ty.is_some());
        assert!(matches!(
            let_stmt.value.as_ref().unwrap(),
            hir::Expr::Literal(hir::Literal::Float(_))
        ));

        let hir::Item::Command(command) = &function.body.items[1] else {
            panic!("expected command");
        };
        assert_eq!(command.name.text, "color");
        assert!(range_text(source, &command.range).starts_with("color base"));
        assert_eq!(range_text(source, &command.name.range), "color");
        assert_eq!(command.args.len(), 1);
        assert!(command.initializer.is_some());
        assert!(command.body.is_some());
    }

    #[test]
    fn lowers_expression_shapes_and_operators() {
        let lowered = lower("effect Pulse { color -sin(a ** b ** c, [true, #fff, 45s])..10; }");
        assert_eq!(lowered.diagnostics, []);

        let root = lowered.root.unwrap();
        let hir::Item::Command(command) = &root.document.block.items[0] else {
            panic!("expected command");
        };
        let hir::Expr::Binary(range) = &command.args[0] else {
            panic!("expected range");
        };
        assert_eq!(range.op, hir::BinaryOp::Range);
        assert!(matches!(*range.left, hir::Expr::Prefix(_)));
        assert!(matches!(
            *range.right,
            hir::Expr::Literal(hir::Literal::Int(_))
        ));
    }

    #[test]
    fn missing_required_syntax_reports_diagnostic() {
        let parse = parse("");
        let lowered = lower_parse(&parse);
        assert!(lowered.root.is_none());
        assert!(lowered.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.kind,
                LowerDiagnosticKind::MissingRequiredSyntax {
                    parent: "SourceFile",
                    field: "document"
                }
            )
        }));
    }

    #[test]
    fn lowered_source_file_is_send_sync() {
        assert_send_sync::<LoweredSourceFile>();
    }
}
