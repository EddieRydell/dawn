use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use ron::extensions::Extensions;
use serde::Deserialize;

use crate::spec::{Grammar, PatternItem};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticsSpec {
    pub version: u32,
    pub root: String,
    pub hir: Vec<HirDecl>,
    pub lower: Vec<LowerRule>,
    pub operators: OperatorSpec,
}

impl SemanticsSpec {
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        ron::Options::default()
            .with_default_extension(Extensions::IMPLICIT_SOME)
            .from_str(&text)
            .with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn validate(&self, grammar: &Grammar) -> Result<()> {
        if self.version != 1 {
            bail!("semantic spec version must be 1");
        }

        ensure_unique(
            "HIR declaration",
            self.hir.iter().map(|decl| decl.name.as_str()),
        )?;
        let hir_names = self
            .hir
            .iter()
            .map(|decl| decl.name.as_str())
            .collect::<BTreeSet<_>>();
        if !hir_names.contains(self.root.as_str()) {
            bail!("semantic root '{}' is not declared", self.root);
        }

        for decl in &self.hir {
            ensure_rust_type("HIR declaration", &decl.name)?;
            match decl {
                HirDecl {
                    kind: HirDeclKind::Struct,
                    fields,
                    ..
                } => {
                    ensure_unique("HIR field", fields.iter().map(|field| field.name.as_str()))?;
                    for field in fields {
                        ensure_rust_value("HIR field", &field.name)?;
                        validate_type_ref(&field.ty, &hir_names)?;
                    }
                }
                HirDecl {
                    kind: HirDeclKind::Enum,
                    variants,
                    ..
                } => {
                    ensure_unique("HIR enum variant", variants.iter().map(|v| v.name.as_str()))?;
                    for variant in variants {
                        ensure_rust_type("HIR enum variant", &variant.name)?;
                        if let Some(ty) = &variant.ty {
                            validate_type_ref(ty, &hir_names)?;
                        }
                    }
                }
            }
        }

        let syntax_nodes = grammar
            .syntax
            .nodes
            .iter()
            .map(String::as_str)
            .chain(
                grammar
                    .expressions
                    .items
                    .iter()
                    .map(|expr| expr.root_node.as_str()),
            )
            .chain(grammar.expressions.items.iter().flat_map(|expr| {
                expr.prefix
                    .iter()
                    .map(|op| op.node.as_str())
                    .chain(expr.infix.iter().map(|op| op.node.as_str()))
                    .chain(expr.postfix.iter().map(|op| op.node.as_str()))
            }))
            .collect::<BTreeSet<_>>();
        let accessors = syntax_accessors(grammar);

        for rule in &self.lower {
            if !syntax_nodes.contains(rule.syntax.as_str()) {
                bail!(
                    "semantic lowering references unknown syntax node '{}'",
                    rule.syntax
                );
            }
            if !hir_names.contains(rule.hir.as_str()) {
                bail!(
                    "semantic lowering references unknown HIR type '{}'",
                    rule.hir
                );
            }
            let node_accessors = accessors.get(rule.syntax.as_str());
            for field in &rule.fields {
                ensure_rust_value("lowering field", &field.name)?;
                if let Some(accessor) = &field.accessor {
                    let valid = node_accessors
                        .is_some_and(|items| items.contains(accessor.as_str()))
                        || special_accessor(&rule.syntax, accessor);
                    if !valid {
                        bail!(
                            "semantic lowering references unknown accessor '{}.{}'",
                            rule.syntax,
                            accessor
                        );
                    }
                }
            }
        }

        validate_operator_mappings(grammar, &self.operators)?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HirDecl {
    pub name: String,
    pub kind: HirDeclKind,
    #[serde(default)]
    pub fields: Vec<HirField>,
    #[serde(default)]
    pub variants: Vec<HirVariant>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HirDeclKind {
    Struct,
    Enum,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HirField {
    pub name: String,
    pub ty: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HirVariant {
    pub name: String,
    pub ty: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LowerRule {
    pub syntax: String,
    pub hir: String,
    #[serde(default)]
    pub fields: Vec<LowerField>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LowerField {
    pub name: String,
    pub accessor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorSpec {
    pub prefix: Vec<OperatorMapping>,
    pub binary: Vec<OperatorMapping>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorMapping {
    pub token: String,
    pub variant: String,
}

fn validate_operator_mappings(grammar: &Grammar, operators: &OperatorSpec) -> Result<()> {
    let prefix_tokens = grammar
        .expressions
        .items
        .iter()
        .flat_map(|expr| expr.prefix.iter().map(|op| op.token.as_str()))
        .collect::<BTreeSet<_>>();
    let binary_tokens = grammar
        .expressions
        .items
        .iter()
        .flat_map(|expr| expr.infix.iter().map(|op| op.token.as_str()))
        .collect::<BTreeSet<_>>();

    validate_operator_group("prefix", &prefix_tokens, &operators.prefix)?;
    validate_operator_group("binary", &binary_tokens, &operators.binary)?;
    Ok(())
}

fn validate_operator_group(
    label: &str,
    expected: &BTreeSet<&str>,
    mappings: &[OperatorMapping],
) -> Result<()> {
    ensure_unique(
        &format!("{label} operator mapping"),
        mappings.iter().map(|mapping| mapping.token.as_str()),
    )?;
    for mapping in mappings {
        ensure_rust_type("operator variant", &mapping.variant)?;
        if !expected.contains(mapping.token.as_str()) {
            bail!(
                "{label} operator mapping references unknown token '{}'",
                mapping.token
            );
        }
    }
    let actual = mappings
        .iter()
        .map(|mapping| mapping.token.as_str())
        .collect::<BTreeSet<_>>();
    for token in expected {
        if !actual.contains(token) {
            bail!("missing {label} operator mapping for token '{token}'");
        }
    }
    Ok(())
}

fn validate_type_ref(ty: &str, hir_names: &BTreeSet<&str>) -> Result<()> {
    let inner = ty
        .strip_prefix("Vec<")
        .and_then(|ty| ty.strip_suffix('>'))
        .or_else(|| {
            ty.strip_prefix("Option<")
                .and_then(|ty| ty.strip_suffix('>'))
        })
        .unwrap_or(ty);
    if matches!(inner, "String" | "bool") || hir_names.contains(inner) {
        return Ok(());
    }
    bail!("HIR type reference '{ty}' is not declared")
}

fn syntax_accessors(grammar: &Grammar) -> BTreeMap<&str, BTreeSet<&str>> {
    let mut accessors = BTreeMap::new();
    for rule in &grammar.syntax.rules {
        let mut names = BTreeSet::new();
        collect_labels(&rule.pattern, &mut names);
        names.insert("syntax");
        accessors.insert(rule.name.as_str(), names);
    }
    accessors
}

fn collect_labels<'a>(items: &'a [PatternItem], labels: &mut BTreeSet<&'a str>) {
    for item in items {
        if let Some(label) = &item.label {
            labels.insert(label.as_str());
        }
        if let Some(repeat) = &item.repeat {
            collect_labels(repeat, labels);
        }
        if let Some(optional) = &item.optional {
            collect_labels(optional, labels);
        }
        if let Some(choice) = &item.choice {
            collect_labels(choice, labels);
        }
    }
}

fn special_accessor(syntax: &str, accessor: &str) -> bool {
    matches!(
        (syntax, accessor),
        ("Name", "text")
            | ("PathLit", "raw_text")
            | ("CommandHead", "text")
            | ("Expr", "kind")
            | ("PrefixExpr", "op")
            | ("PrefixExpr", "expr")
            | ("BinaryExpr", "left")
            | ("BinaryExpr", "right")
            | ("BinaryExpr", "op")
            | ("CallExpr", "callee")
            | ("CallExpr", "args")
    )
}

fn ensure_unique<'a>(label: &str, values: impl IntoIterator<Item = &'a str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            bail!("duplicate {label} '{value}'");
        }
    }
    Ok(())
}

fn ensure_rust_type(label: &str, value: &str) -> Result<()> {
    ensure_identifier(label, value, true)
}

fn ensure_rust_value(label: &str, value: &str) -> Result<()> {
    ensure_identifier(label, value, false)
}

fn ensure_identifier(label: &str, value: &str, upper: bool) -> Result<()> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        bail!("{label} must not be empty");
    };
    let valid_first = if upper {
        first == '_' || first.is_ascii_uppercase()
    } else {
        first == '_' || first.is_ascii_lowercase()
    };
    if !valid_first || !chars.all(|c| c == '_' || c.is_ascii_alphanumeric()) {
        bail!("{label} '{value}' is not a valid Rust identifier");
    }
    Ok(())
}

pub fn generate(
    _grammar: &Grammar,
    _semantics: &SemanticsSpec,
    header: &str,
) -> Vec<(&'static str, String)> {
    vec![
        ("src/generated/mod.rs", gen_mod(header)),
        ("src/generated/diagnostic.rs", gen_diagnostic(header)),
        ("src/generated/hir.rs", gen_hir(header)),
        ("src/generated/lower.rs", gen_lower(header)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grammar() -> Grammar {
        ron::Options::default()
            .with_default_extension(Extensions::IMPLICIT_SOME)
            .from_str(
                r#"(
                    version: 2,
                    language: (type_name: "ToyLanguage", syntax_kind: "ToyKind"),
                    diagnostics: (
                        invalid_token: "InvalidToken",
                        unexpected_token: "UnexpectedToken",
                        unexpected_eof: "UnexpectedEof",
                        kinds: [
                            (name: "InvalidToken", message: "invalid token"),
                            (name: "UnexpectedToken", message: "unexpected token"),
                            (name: "UnexpectedEof", message: "unexpected eof"),
                        ],
                    ),
                    tokens: (
                        rules: [
                            (name: "Whitespace", regex: "[ \n]+", trivia: skip),
                            (name: "Ident", regex: "[A-Za-z]+"),
                            (name: "Int", regex: "[0-9]+"),
                            (name: "Minus", literal: "-"),
                            (name: "Plus", literal: "+"),
                        ],
                    ),
                    syntax: (
                        entry: "File",
                        nodes: ["File", "Name"],
                        rules: [
                            (name: "File", pattern: [(label: "name", node: "Name"), (expr: "Expr")]),
                            (name: "Name", pattern: [(label: "token", token: "Ident")]),
                        ],
                        token_sets: [],
                    ),
                    expressions: (
                        items: [(
                            name: "Expr",
                            root_node: "Expr",
                            atoms: [(token: "Int"), (node: "Name")],
                            prefix: [(token: "Minus", node: "PrefixExpr")],
                            infix: [(token: "Plus", node: "BinaryExpr", precedence: "Add", associativity: "left")],
                        )],
                    ),
                    ast: (root: "File", nodes: []),
                )"#,
            )
            .unwrap()
    }

    fn semantics(text: &str) -> SemanticsSpec {
        ron::Options::default()
            .with_default_extension(Extensions::IMPLICIT_SOME)
            .from_str(text)
            .unwrap()
    }

    fn valid_semantics() -> String {
        r#"(
            version: 1,
            root: "File",
            hir: [
                (name: "File", kind: struct, fields: [(name: "name", ty: "Ident")]),
                (name: "Ident", kind: struct, fields: [(name: "text", ty: "String")]),
            ],
            lower: [
                (syntax: "File", hir: "File", fields: [(name: "name", accessor: "name")]),
                (syntax: "Name", hir: "Ident", fields: [(name: "text", accessor: "text")]),
            ],
            operators: (
                prefix: [(token: "Minus", variant: "Neg")],
                binary: [(token: "Plus", variant: "Add")],
            ),
        )"#
        .to_string()
    }

    #[test]
    fn validates_semantics_spec() {
        let grammar = grammar();
        grammar.validate().unwrap();
        semantics(&valid_semantics()).validate(&grammar).unwrap();
    }

    #[test]
    fn rejects_unknown_syntax_accessor() {
        let grammar = grammar();
        let spec = valid_semantics().replace("accessor: \"name\"", "accessor: \"missing\"");
        let error = semantics(&spec).validate(&grammar).unwrap_err().to_string();
        assert!(error.contains("unknown accessor"));
    }

    #[test]
    fn rejects_missing_operator_mapping() {
        let grammar = grammar();
        let spec = valid_semantics().replace("(token: \"Plus\", variant: \"Add\")", "");
        let error = semantics(&spec).validate(&grammar).unwrap_err().to_string();
        assert!(error.contains("missing binary operator mapping"));
    }

    #[test]
    fn rejects_duplicate_hir_names() {
        let grammar = grammar();
        let spec = valid_semantics().replace(
            "(name: \"Ident\", kind: struct, fields: [(name: \"text\", ty: \"String\")]),",
            "(name: \"Ident\", kind: struct, fields: [(name: \"text\", ty: \"String\")]),\n                (name: \"Ident\", kind: struct, fields: []),",
        );
        let error = semantics(&spec).validate(&grammar).unwrap_err().to_string();
        assert!(error.contains("duplicate HIR declaration"));
    }

    #[test]
    fn rejects_invalid_hir_type_reference() {
        let grammar = grammar();
        let spec = valid_semantics().replace("ty: \"Ident\"", "ty: \"Missing\"");
        let error = semantics(&spec).validate(&grammar).unwrap_err().to_string();
        assert!(error.contains("not declared"));
    }
}

fn gen_mod(header: &str) -> String {
    format!("{header}pub mod diagnostic;\npub mod hir;\npub mod lower;\n")
}

fn gen_diagnostic(header: &str) -> String {
    format!(
        r#"{header}#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LowerDiagnostic {{
    pub kind: LowerDiagnosticKind,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerDiagnosticKind {{
    MissingRequiredSyntax {{ parent: &'static str, field: &'static str }},
    UnknownOperator {{ operator: String }},
}}

impl LowerDiagnostic {{
    pub fn missing_required(parent: &'static str, field: &'static str) -> Self {{
        Self {{
            kind: LowerDiagnosticKind::MissingRequiredSyntax {{ parent, field }},
        }}
    }}

    pub fn unknown_operator(operator: impl Into<String>) -> Self {{
        Self {{
            kind: LowerDiagnosticKind::UnknownOperator {{
                operator: operator.into(),
            }},
        }}
    }}

    pub fn message(&self) -> String {{
        match &self.kind {{
            LowerDiagnosticKind::MissingRequiredSyntax {{ parent, field }} => {{
                format!("missing required syntax field {{parent}}.{{field}}")
            }}
            LowerDiagnosticKind::UnknownOperator {{ operator }} => {{
                format!("unknown semantic operator mapping for '{{operator}}'")
            }}
        }}
    }}
}}
"#
    )
}

fn gen_hir(header: &str) -> String {
    format!(
        r#"{header}use dawn_syntax::ast;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {{
    pub imports: Vec<Import>,
    pub document: Document,
    pub syntax: ast::SourceFile,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {{
    pub kind: Ident,
    pub name: Ident,
    pub path: PathLiteral,
    pub syntax: ast::ImportDecl,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {{
    pub kind: Ident,
    pub name: Ident,
    pub block: Block,
    pub syntax: ast::Document,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {{
    pub items: Vec<Item>,
    pub syntax: ast::Block,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {{
    FnDecl(FnDecl),
    LetStmt(LetStmt),
    Command(Command),
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnDecl {{
    pub name: Ident,
    pub params: Vec<Param>,
    pub return_type: Option<TypeRef>,
    pub body: Block,
    pub syntax: ast::FnDecl,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {{
    pub name: Ident,
    pub ty: Option<TypeRef>,
    pub syntax: ast::Param,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetStmt {{
    pub name: Ident,
    pub ty: Option<TypeRef>,
    pub value: Option<Expr>,
    pub syntax: ast::LetStmt,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {{
    pub name: Ident,
    pub args: Vec<Expr>,
    pub initializer: Option<Expr>,
    pub body: Option<Block>,
    pub syntax: ast::Command,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {{
    pub name: Ident,
    pub syntax: ast::TypeRef,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {{
    pub text: String,
    pub syntax: ast::Name,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathLiteral {{
    pub raw_text: String,
    pub syntax: ast::PathLit,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {{
    NameRef(NameRef),
    Literal(Literal),
    List(List),
    Paren(Paren),
    Call(Call),
    Prefix(Prefix),
    Binary(Binary),
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameRef {{
    pub name: Ident,
    pub syntax: ast::NameRef,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Literal {{
    String(RawLiteral<ast::StringLit>),
    Int(RawLiteral<ast::IntLit>),
    Float(RawLiteral<ast::FloatLit>),
    Bool(BoolLiteral),
    Color(RawLiteral<ast::ColorLit>),
    Duration(RawLiteral<ast::DurationLit>),
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawLiteral<Syntax> {{
    pub raw_text: String,
    pub syntax: Syntax,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoolLiteral {{
    pub value: bool,
    pub syntax: ast::BoolLit,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List {{
    pub items: Vec<Expr>,
    pub syntax: ast::ListExpr,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paren {{
    pub expr: Box<Expr>,
    pub syntax: ast::ParenExpr,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {{
    pub callee: Box<Expr>,
    pub args: Vec<Expr>,
    pub syntax: ast::CallExpr,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefix {{
    pub op: PrefixOp,
    pub expr: Box<Expr>,
    pub syntax: ast::PrefixExpr,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binary {{
    pub left: Box<Expr>,
    pub op: BinaryOp,
    pub right: Box<Expr>,
    pub syntax: ast::BinaryExpr,
}}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixOp {{
    Neg,
    Not,
}}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {{
    LogicalOr,
    LogicalAnd,
    BitOr,
    BitXor,
    BitAnd,
    Eq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Range,
    Shl,
    Shr,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
}}
"#
    )
}

fn gen_lower(header: &str) -> String {
    format!(
        r#"{header}use dawn_syntax::ast;
use dawn_syntax::SyntaxKind;

use super::diagnostic::LowerDiagnostic;
use super::hir;

macro_rules! required {{
    ($self:ident, $parent:literal, $field:literal, $value:expr, $lower:ident) => {{{{
        let Some(value) = $value else {{
            $self.missing($parent, $field);
            return None;
        }};
        let Some(value) = $self.$lower(value) else {{
            return None;
        }};
        value
    }}}};
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweredSourceFile {{
    pub root: Option<hir::SourceFile>,
    pub diagnostics: Vec<LowerDiagnostic>,
}}

pub fn lower_parse(parse: &dawn_syntax::Parse) -> LoweredSourceFile {{
    let mut ctx = LowerCtx::default();
    let root = parse.source_file().and_then(|source| ctx.source_file(source));
    LoweredSourceFile {{
        root,
        diagnostics: ctx.diagnostics,
    }}
}}

pub fn lower_source_file(source: ast::SourceFile) -> LoweredSourceFile {{
    let mut ctx = LowerCtx::default();
    let root = ctx.source_file(source);
    LoweredSourceFile {{
        root,
        diagnostics: ctx.diagnostics,
    }}
}}

#[derive(Default)]
struct LowerCtx {{
    diagnostics: Vec<LowerDiagnostic>,
}}

impl LowerCtx {{
    fn source_file(&mut self, syntax: ast::SourceFile) -> Option<hir::SourceFile> {{
        let imports = syntax
            .imports()
            .into_iter()
            .filter_map(|import| self.import(import))
            .collect();
        let document = required!(self, "SourceFile", "document", syntax.document(), document);
        Some(hir::SourceFile {{
            imports,
            document,
            syntax,
        }})
    }}

    fn import(&mut self, syntax: ast::ImportDecl) -> Option<hir::Import> {{
        let kind = required!(self, "ImportDecl", "kind", syntax.kind(), ident);
        let name = required!(self, "ImportDecl", "name", syntax.name(), ident);
        let path = required!(self, "ImportDecl", "path", syntax.path(), path_literal);
        Some(hir::Import {{
            kind,
            name,
            path,
            syntax,
        }})
    }}

    fn document(&mut self, syntax: ast::Document) -> Option<hir::Document> {{
        let kind = required!(self, "Document", "kind", syntax.kind(), ident);
        let name = required!(self, "Document", "name", syntax.name(), ident);
        let block = required!(self, "Document", "block", syntax.block(), block);
        Some(hir::Document {{
            kind,
            name,
            block,
            syntax,
        }})
    }}

    fn block(&mut self, syntax: ast::Block) -> Option<hir::Block> {{
        let items = syntax
            .items()
            .into_iter()
            .filter_map(|item| self.item(item))
            .collect();
        Some(hir::Block {{ items, syntax }})
    }}

    fn item(&mut self, syntax: ast::Item) -> Option<hir::Item> {{
        match syntax.kind()? {{
            ast::ItemKind::FnDecl(item) => self.fn_decl(item).map(hir::Item::FnDecl),
            ast::ItemKind::LetStmt(item) => self.let_stmt(item).map(hir::Item::LetStmt),
            ast::ItemKind::Command(item) => self.command(item).map(hir::Item::Command),
        }}
    }}

    fn fn_decl(&mut self, syntax: ast::FnDecl) -> Option<hir::FnDecl> {{
        let name = required!(self, "FnDecl", "name", syntax.name(), ident);
        let params = syntax
            .params()
            .map(|params| {{
                params
                    .params()
                    .into_iter()
                    .filter_map(|param| self.param(param))
                    .collect()
            }})
            .unwrap_or_else(|| {{
                self.missing("FnDecl", "params");
                Vec::new()
            }});
        let return_type = syntax.return_type().and_then(|ty| self.type_ref(ty));
        let body = required!(self, "FnDecl", "body", syntax.body(), block);
        Some(hir::FnDecl {{
            name,
            params,
            return_type,
            body,
            syntax,
        }})
    }}

    fn param(&mut self, syntax: ast::Param) -> Option<hir::Param> {{
        let name = required!(self, "Param", "name", syntax.name(), ident);
        let ty = syntax.ty().and_then(|ty| self.type_ref(ty));
        Some(hir::Param {{ name, ty, syntax }})
    }}

    fn let_stmt(&mut self, syntax: ast::LetStmt) -> Option<hir::LetStmt> {{
        let name = required!(self, "LetStmt", "name", syntax.name(), ident);
        let ty = syntax.ty().and_then(|ty| self.type_ref(ty));
        let value = syntax.value().and_then(|expr| self.expr(expr));
        Some(hir::LetStmt {{
            name,
            ty,
            value,
            syntax,
        }})
    }}

    fn command(&mut self, syntax: ast::Command) -> Option<hir::Command> {{
        let head = required!(self, "Command", "head", syntax.head(), command_head);
        let args = syntax
            .args()
            .into_iter()
            .filter_map(|arg| self.expr(arg))
            .collect();
        let initializer = syntax
            .initializer()
            .and_then(|initializer| initializer.value())
            .and_then(|expr| self.expr(expr));
        let body = syntax.body().and_then(|block| self.block(block));
        Some(hir::Command {{
            name: head,
            args,
            initializer,
            body,
            syntax,
        }})
    }}

    fn command_head(&mut self, syntax: ast::CommandHead) -> Option<hir::Ident> {{
        let name = required!(self, "CommandHead", "name", syntax.name(), ident);
        Some(name)
    }}

    fn type_ref(&mut self, syntax: ast::TypeRef) -> Option<hir::TypeRef> {{
        let name = required!(self, "TypeRef", "name", syntax.name(), ident);
        Some(hir::TypeRef {{ name, syntax }})
    }}

    fn ident(&mut self, syntax: ast::Name) -> Option<hir::Ident> {{
        let text = syntax.text().unwrap_or_else(|| {{
            self.missing("Name", "text");
            String::new()
        }});
        Some(hir::Ident {{ text, syntax }})
    }}

    fn path_literal(&mut self, syntax: ast::PathLit) -> Option<hir::PathLiteral> {{
        let raw_text = syntax.raw_text().unwrap_or_else(|| {{
            self.missing("PathLit", "raw_text");
            String::new()
        }});
        Some(hir::PathLiteral {{ raw_text, syntax }})
    }}

    fn expr(&mut self, syntax: ast::Expr) -> Option<hir::Expr> {{
        match syntax.kind()? {{
            ast::ExprKind::NameRef(expr) => self.name_ref(expr).map(hir::Expr::NameRef),
            ast::ExprKind::StringLit(expr) => self.string_lit(expr).map(hir::Literal::String).map(hir::Expr::Literal),
            ast::ExprKind::IntLit(expr) => self.int_lit(expr).map(hir::Literal::Int).map(hir::Expr::Literal),
            ast::ExprKind::FloatLit(expr) => self.float_lit(expr).map(hir::Literal::Float).map(hir::Expr::Literal),
            ast::ExprKind::BoolLit(expr) => self.bool_lit(expr).map(hir::Literal::Bool).map(hir::Expr::Literal),
            ast::ExprKind::ColorLit(expr) => self.color_lit(expr).map(hir::Literal::Color).map(hir::Expr::Literal),
            ast::ExprKind::DurationLit(expr) => self.duration_lit(expr).map(hir::Literal::Duration).map(hir::Expr::Literal),
            ast::ExprKind::ListExpr(expr) => Some(hir::Expr::List(hir::List {{
                items: expr.items().into_iter().filter_map(|item| self.expr(item)).collect(),
                syntax: expr,
            }})),
            ast::ExprKind::ParenExpr(expr) => {{
                let inner = required!(self, "ParenExpr", "expr", expr.expr(), expr);
                Some(hir::Expr::Paren(hir::Paren {{
                    expr: Box::new(inner),
                    syntax: expr,
                }}))
            }}
            ast::ExprKind::CallExpr(expr) => {{
                let callee = required!(self, "CallExpr", "callee", expr.callee(), expr);
                let args = expr.args().into_iter().filter_map(|arg| self.expr(arg)).collect();
                Some(hir::Expr::Call(hir::Call {{
                    callee: Box::new(callee),
                    args,
                    syntax: expr,
                }}))
            }}
            ast::ExprKind::PrefixExpr(expr) => {{
                let Some(op_token) = expr.op() else {{
                    self.missing("PrefixExpr", "op");
                    return None;
                }};
                let op = self.prefix_op(op_token.kind(), op_token.text())?;
                let inner = required!(self, "PrefixExpr", "expr", expr.expr(), expr);
                Some(hir::Expr::Prefix(hir::Prefix {{
                    op,
                    expr: Box::new(inner),
                    syntax: expr,
                }}))
            }}
            ast::ExprKind::BinaryExpr(expr) => {{
                let left = required!(self, "BinaryExpr", "left", expr.left(), expr);
                let Some(op_token) = expr.op() else {{
                    self.missing("BinaryExpr", "op");
                    return None;
                }};
                let op = self.binary_op(op_token.kind(), op_token.text())?;
                let right = required!(self, "BinaryExpr", "right", expr.right(), expr);
                Some(hir::Expr::Binary(hir::Binary {{
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                    syntax: expr,
                }}))
            }}
        }}
    }}

    fn name_ref(&mut self, syntax: ast::NameRef) -> Option<hir::NameRef> {{
        let name = required!(self, "NameRef", "name", syntax.name(), ident);
        Some(hir::NameRef {{ name, syntax }})
    }}

    fn string_lit(&mut self, syntax: ast::StringLit) -> Option<hir::RawLiteral<ast::StringLit>> {{
        self.raw_lit("StringLit", syntax.text(), syntax)
    }}

    fn int_lit(&mut self, syntax: ast::IntLit) -> Option<hir::RawLiteral<ast::IntLit>> {{
        self.raw_lit("IntLit", syntax.text(), syntax)
    }}

    fn float_lit(&mut self, syntax: ast::FloatLit) -> Option<hir::RawLiteral<ast::FloatLit>> {{
        self.raw_lit("FloatLit", syntax.text(), syntax)
    }}

    fn color_lit(&mut self, syntax: ast::ColorLit) -> Option<hir::RawLiteral<ast::ColorLit>> {{
        self.raw_lit("ColorLit", syntax.text(), syntax)
    }}

    fn duration_lit(&mut self, syntax: ast::DurationLit) -> Option<hir::RawLiteral<ast::DurationLit>> {{
        self.raw_lit("DurationLit", syntax.text(), syntax)
    }}

    fn raw_lit<Syntax>(&mut self, parent: &'static str, raw_text: Option<String>, syntax: Syntax) -> Option<hir::RawLiteral<Syntax>> {{
        let raw_text = raw_text.unwrap_or_else(|| {{
            self.missing(parent, "token");
            String::new()
        }});
        Some(hir::RawLiteral {{ raw_text, syntax }})
    }}

    fn bool_lit(&mut self, syntax: ast::BoolLit) -> Option<hir::BoolLiteral> {{
        let text = syntax.text().unwrap_or_else(|| {{
            self.missing("BoolLit", "token");
            String::new()
        }});
        Some(hir::BoolLiteral {{
            value: text == "true",
            syntax,
        }})
    }}

    fn prefix_op(&mut self, kind: SyntaxKind, text: &str) -> Option<hir::PrefixOp> {{
        match kind {{
            SyntaxKind::Minus => Some(hir::PrefixOp::Neg),
            SyntaxKind::Bang => Some(hir::PrefixOp::Not),
            _ => {{
                self.diagnostics.push(LowerDiagnostic::unknown_operator(text));
                None
            }}
        }}
    }}

    fn binary_op(&mut self, kind: SyntaxKind, text: &str) -> Option<hir::BinaryOp> {{
        match kind {{
            SyntaxKind::OrOr => Some(hir::BinaryOp::LogicalOr),
            SyntaxKind::AndAnd => Some(hir::BinaryOp::LogicalAnd),
            SyntaxKind::Pipe => Some(hir::BinaryOp::BitOr),
            SyntaxKind::Caret => Some(hir::BinaryOp::BitXor),
            SyntaxKind::Ampersand => Some(hir::BinaryOp::BitAnd),
            SyntaxKind::EqEq => Some(hir::BinaryOp::Eq),
            SyntaxKind::BangEq => Some(hir::BinaryOp::NotEq),
            SyntaxKind::Lt => Some(hir::BinaryOp::Lt),
            SyntaxKind::Le => Some(hir::BinaryOp::Le),
            SyntaxKind::Gt => Some(hir::BinaryOp::Gt),
            SyntaxKind::Ge => Some(hir::BinaryOp::Ge),
            SyntaxKind::DotDot => Some(hir::BinaryOp::Range),
            SyntaxKind::Shl => Some(hir::BinaryOp::Shl),
            SyntaxKind::Shr => Some(hir::BinaryOp::Shr),
            SyntaxKind::Plus => Some(hir::BinaryOp::Add),
            SyntaxKind::Minus => Some(hir::BinaryOp::Sub),
            SyntaxKind::Star => Some(hir::BinaryOp::Mul),
            SyntaxKind::Slash => Some(hir::BinaryOp::Div),
            SyntaxKind::Percent => Some(hir::BinaryOp::Rem),
            SyntaxKind::StarStar => Some(hir::BinaryOp::Pow),
            _ => {{
                self.diagnostics.push(LowerDiagnostic::unknown_operator(text));
                None
            }}
        }}
    }}

    fn missing(&mut self, parent: &'static str, field: &'static str) {{
        self.diagnostics
            .push(LowerDiagnostic::missing_required(parent, field));
    }}
}}
"#
    )
}
