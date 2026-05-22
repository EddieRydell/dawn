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

    #[test]
    fn lowers_import_and_document() {
        let lowered =
            lower("import display MainDisplay from <displays/main.display.dawn>;\neffect Pulse {}");
        assert_eq!(lowered.diagnostics, []);

        let root = lowered.root.unwrap();
        assert_eq!(root.imports[0].kind.text, "display");
        assert_eq!(root.imports[0].name.text, "MainDisplay");
        assert_eq!(root.imports[0].path.raw_text, "displays/main.display.dawn");
        assert_eq!(root.document.kind.text, "effect");
        assert_eq!(root.document.name.text, "Pulse");
        assert!(root.document.block.items.is_empty());
    }

    #[test]
    fn lowers_functions_params_lets_and_commands() {
        let lowered = lower(
            r#"effect Pulse {
  fn sample(t float) color {
    let phase float = 1.0;
    color base = accent {}
  }
}"#,
        );
        assert_eq!(lowered.diagnostics, []);

        let root = lowered.root.unwrap();
        let hir::Item::FnDecl(function) = &root.document.block.items[0] else {
            panic!("expected function");
        };
        assert_eq!(function.name.text, "sample");
        assert_eq!(function.params[0].name.text, "t");
        assert_eq!(function.params[0].ty.as_ref().unwrap().name.text, "float");
        assert_eq!(function.return_type.as_ref().unwrap().name.text, "color");

        let hir::Item::LetStmt(let_stmt) = &function.body.items[0] else {
            panic!("expected let");
        };
        assert_eq!(let_stmt.name.text, "phase");
        assert!(let_stmt.ty.is_some());
        assert!(matches!(
            let_stmt.value.as_ref().unwrap(),
            hir::Expr::Literal(hir::Literal::Float(_))
        ));

        let hir::Item::Command(command) = &function.body.items[1] else {
            panic!("expected command");
        };
        assert_eq!(command.name.text, "color");
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
}
