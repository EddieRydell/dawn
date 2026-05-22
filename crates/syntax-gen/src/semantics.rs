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
        Validator::new(grammar, self)?.validate()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HirDecl {
    pub name: String,
    pub kind: HirDeclKind,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub fields: Vec<HirField>,
    #[serde(default)]
    pub variants: Vec<HirVariant>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
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
    pub dispatch: Option<String>,
    #[serde(default)]
    pub fields: Vec<LowerField>,
    #[serde(default)]
    pub variants: Vec<LowerVariant>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LowerField {
    pub name: String,
    pub accessor: String,
    #[serde(default)]
    pub scalar: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LowerVariant {
    pub syntax: String,
    pub target: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum TypeRef {
    Named(String),
    Option(Box<TypeRef>),
    Vec(Box<TypeRef>),
    Box(Box<TypeRef>),
}

impl TypeRef {
    fn parse(text: &str) -> Result<Self> {
        let text = text.trim();
        for wrapper in ["Option", "Vec", "Box"] {
            let prefix = format!("{wrapper}<");
            if let Some(inner) = text
                .strip_prefix(&prefix)
                .and_then(|ty| ty.strip_suffix('>'))
            {
                let inner = Box::new(Self::parse(inner)?);
                return Ok(match wrapper {
                    "Option" => Self::Option(inner),
                    "Vec" => Self::Vec(inner),
                    "Box" => Self::Box(inner),
                    _ => unreachable!(),
                });
            }
        }
        if !matches!(text, "String" | "bool") {
            ensure_rust_type("HIR type reference", text)?;
        }
        Ok(Self::Named(text.to_string()))
    }

    fn named(&self) -> Option<&str> {
        match self {
            Self::Named(name) => Some(name),
            Self::Box(inner) => inner.named(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AccessorShape {
    One(String),
    Many(String),
    Scalar(String),
}

impl AccessorShape {
    fn target(&self) -> &str {
        match self {
            Self::One(target) | Self::Many(target) | Self::Scalar(target) => target,
        }
    }
}

#[derive(Debug, Clone)]
struct FieldPlan {
    name: String,
    ty: TypeRef,
    accessor: Vec<String>,
    scalar: Option<String>,
}

struct Validator<'a> {
    grammar: &'a Grammar,
    semantics: &'a SemanticsSpec,
    hir: BTreeMap<&'a str, &'a HirDecl>,
    syntax_nodes: BTreeSet<&'a str>,
    accessors: BTreeMap<String, BTreeMap<String, AccessorShape>>,
}

impl<'a> Validator<'a> {
    fn new(grammar: &'a Grammar, semantics: &'a SemanticsSpec) -> Result<Self> {
        let hir = semantics
            .hir
            .iter()
            .map(|decl| (decl.name.as_str(), decl))
            .collect();
        Ok(Self {
            grammar,
            semantics,
            hir,
            syntax_nodes: syntax_nodes(grammar),
            accessors: syntax_accessors(grammar),
        })
    }

    fn validate(&self) -> Result<()> {
        if self.semantics.version != 1 {
            bail!("semantic spec version must be 1");
        }
        ensure_unique(
            "HIR declaration",
            self.semantics.hir.iter().map(|decl| decl.name.as_str()),
        )?;
        if !self.hir.contains_key(self.semantics.root.as_str()) {
            bail!("semantic root '{}' is not declared", self.semantics.root);
        }

        for decl in &self.semantics.hir {
            self.validate_hir_decl(decl)?;
        }

        ensure_unique(
            "lowering syntax rule",
            self.semantics.lower.iter().map(|rule| rule.syntax.as_str()),
        )?;
        for rule in &self.semantics.lower {
            self.validate_lower_rule(rule)?;
        }
        self.validate_operator_mappings()?;
        Ok(())
    }

    fn validate_hir_decl(&self, decl: &HirDecl) -> Result<()> {
        ensure_rust_type("HIR declaration", &decl.name)?;
        if let Some(source) = &decl.source {
            if source != "SyntaxNode" && !self.syntax_nodes.contains(source.as_str()) {
                bail!(
                    "HIR declaration '{}' references unknown source syntax '{}'",
                    decl.name,
                    source
                );
            }
        }
        match decl.kind {
            HirDeclKind::Struct => {
                if !decl.variants.is_empty() {
                    bail!("HIR struct '{}' cannot declare variants", decl.name);
                }
                ensure_unique(
                    "HIR field",
                    decl.fields.iter().map(|field| field.name.as_str()),
                )?;
                for field in &decl.fields {
                    ensure_rust_value("HIR field", &field.name)?;
                    self.validate_type_ref(&TypeRef::parse(&field.ty)?)?;
                }
            }
            HirDeclKind::Enum => {
                if !decl.fields.is_empty() {
                    bail!("HIR enum '{}' cannot declare fields", decl.name);
                }
                ensure_unique(
                    "HIR enum variant",
                    decl.variants.iter().map(|variant| variant.name.as_str()),
                )?;
                for variant in &decl.variants {
                    ensure_rust_type("HIR enum variant", &variant.name)?;
                    if let Some(ty) = &variant.ty {
                        self.validate_type_ref(&TypeRef::parse(ty)?)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_type_ref(&self, ty: &TypeRef) -> Result<()> {
        match ty {
            TypeRef::Named(name) => {
                if matches!(name.as_str(), "String" | "bool")
                    || self.hir.contains_key(name.as_str())
                {
                    Ok(())
                } else {
                    bail!("HIR type reference '{name}' is not declared")
                }
            }
            TypeRef::Option(inner) | TypeRef::Vec(inner) | TypeRef::Box(inner) => {
                self.validate_type_ref(inner)
            }
        }
    }

    fn validate_lower_rule(&self, rule: &LowerRule) -> Result<()> {
        if !self.syntax_nodes.contains(rule.syntax.as_str()) {
            bail!(
                "semantic lowering references unknown syntax node '{}'",
                rule.syntax
            );
        }
        let Some(decl) = self.hir.get(rule.hir.as_str()).copied() else {
            bail!(
                "semantic lowering references unknown HIR type '{}'",
                rule.hir
            );
        };
        match decl.kind {
            HirDeclKind::Struct => self.validate_struct_lower(rule, decl),
            HirDeclKind::Enum => self.validate_enum_lower(rule, decl),
        }
    }

    fn validate_struct_lower(&self, rule: &LowerRule, decl: &HirDecl) -> Result<()> {
        if rule.dispatch.is_some() || !rule.variants.is_empty() {
            bail!(
                "struct lowering rule '{} -> {}' cannot dispatch variants",
                rule.syntax,
                rule.hir
            );
        }
        ensure_unique(
            "lowering field",
            rule.fields.iter().map(|field| field.name.as_str()),
        )?;
        let declared = decl
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<BTreeSet<_>>();
        let lowered = rule
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<BTreeSet<_>>();
        for field in declared.difference(&lowered) {
            bail!(
                "lowering rule '{} -> {}' does not populate HIR field '{}'",
                rule.syntax,
                rule.hir,
                field
            );
        }
        for field in lowered.difference(&declared) {
            bail!(
                "lowering rule '{} -> {}' populates unknown HIR field '{}'",
                rule.syntax,
                rule.hir,
                field
            );
        }
        for field in &rule.fields {
            ensure_rust_value("lowering field", &field.name)?;
            let hir_field = decl
                .fields
                .iter()
                .find(|candidate| candidate.name == field.name)
                .expect("checked above");
            let shape = self.validate_accessor_path(&rule.syntax, &field.accessor)?;
            self.validate_lower_field_type(rule, field, &TypeRef::parse(&hir_field.ty)?, &shape)?;
        }
        Ok(())
    }

    fn validate_enum_lower(&self, rule: &LowerRule, decl: &HirDecl) -> Result<()> {
        if !rule.fields.is_empty() {
            bail!(
                "enum lowering rule '{} -> {}' cannot declare fields",
                rule.syntax,
                rule.hir
            );
        }
        let dispatch = rule.dispatch.as_deref().unwrap_or("kind");
        let shape = self.validate_accessor_path(&rule.syntax, dispatch)?;
        if !matches!(shape, AccessorShape::Scalar(_)) {
            bail!(
                "enum lowering dispatch '{}.{}' must be a scalar enum accessor",
                rule.syntax,
                dispatch
            );
        }
        ensure_unique(
            "lowering enum variant",
            rule.variants.iter().map(|variant| variant.syntax.as_str()),
        )?;
        for variant in &rule.variants {
            if !self.syntax_nodes.contains(variant.syntax.as_str()) {
                bail!(
                    "enum lowering target references unknown syntax node '{}'",
                    variant.syntax
                );
            }
            self.validate_target_path(&decl.name, &variant.target)?;
        }
        Ok(())
    }

    fn validate_lower_field_type(
        &self,
        rule: &LowerRule,
        field: &LowerField,
        ty: &TypeRef,
        shape: &AccessorShape,
    ) -> Result<()> {
        if let Some(scalar) = &field.scalar {
            match scalar.as_str() {
                "text" | "raw_text" => {
                    if ty.named() != Some("String") {
                        bail!(
                            "scalar '{}' for '{}.{}' requires a String field",
                            scalar,
                            rule.hir,
                            field.name
                        );
                    }
                }
                "bool" => {
                    if ty.named() != Some("bool") {
                        bail!(
                            "scalar 'bool' for '{}.{}' requires a bool field",
                            rule.hir,
                            field.name
                        );
                    }
                }
                "operator(prefix)" | "operator(binary)" => {
                    self.require_operator_field_ty(ty, &rule.hir, &field.name)?
                }
                _ => bail!("unknown lowering scalar '{}'", scalar),
            }
            return Ok(());
        }

        match ty {
            TypeRef::Vec(inner) => {
                if !matches!(shape, AccessorShape::Many(_)) {
                    bail!(
                        "lowering field '{}.{}' expects a repeated accessor",
                        rule.hir,
                        field.name
                    );
                }
                self.require_lowerable_type(inner, shape.target())
            }
            TypeRef::Option(inner) => self.require_lowerable_type(inner, shape.target()),
            TypeRef::Box(_) | TypeRef::Named(_) => {
                if !matches!(shape, AccessorShape::One(_)) {
                    bail!(
                        "lowering field '{}.{}' expects a single accessor",
                        rule.hir,
                        field.name
                    );
                }
                self.require_lowerable_type(ty, shape.target())
            }
        }
    }

    fn require_operator_field_ty(&self, ty: &TypeRef, hir: &str, field: &str) -> Result<()> {
        let Some(name) = ty.named() else {
            bail!("operator scalar for '{hir}.{field}' requires a named HIR enum type");
        };
        let Some(decl) = self.hir.get(name).copied() else {
            bail!("operator scalar for '{hir}.{field}' references unknown HIR type '{name}'");
        };
        if decl.kind == HirDeclKind::Enum
            && decl.variants.iter().all(|variant| variant.ty.is_none())
        {
            Ok(())
        } else {
            bail!("operator scalar for '{hir}.{field}' requires a payload-free HIR enum type")
        }
    }

    fn require_lowerable_type(&self, ty: &TypeRef, syntax_target: &str) -> Result<()> {
        let Some(hir_name) = ty.named() else {
            bail!("nested containers are not supported in lowering fields");
        };
        if matches!(hir_name, "String" | "bool") {
            bail!("scalar HIR type '{hir_name}' needs an explicit scalar lowering");
        }
        let Some(rule) = self
            .semantics
            .lower
            .iter()
            .find(|rule| rule.syntax == syntax_target)
        else {
            bail!("no lowering rule declared for syntax node '{syntax_target}'");
        };
        if rule.hir != hir_name {
            bail!(
                "syntax node '{}' lowers to '{}', not '{}'",
                syntax_target,
                rule.hir,
                hir_name
            );
        }
        Ok(())
    }

    fn validate_accessor_path(&self, syntax: &str, accessor: &str) -> Result<AccessorShape> {
        let mut current = syntax.to_string();
        let mut final_shape = None;
        for part in accessor.split('.') {
            ensure_rust_value("accessor", part)?;
            let Some(shape) = self
                .accessors
                .get(&current)
                .and_then(|items| items.get(part))
                .cloned()
            else {
                bail!("semantic lowering references unknown accessor '{current}.{part}'");
            };
            current = shape.target().to_string();
            final_shape = Some(shape);
        }
        final_shape.with_context(|| "accessor path must not be empty")
    }

    fn validate_target_path(&self, root: &str, target: &str) -> Result<()> {
        let mut current = root;
        for part in target.split("::") {
            ensure_rust_type("enum target path", part)?;
            let Some(decl) = self.hir.get(current).copied() else {
                bail!(
                    "enum target path '{}' references unknown HIR '{}'",
                    target,
                    current
                );
            };
            if decl.kind != HirDeclKind::Enum {
                bail!(
                    "enum target path '{}' passes through non-enum '{}'",
                    target,
                    current
                );
            }
            let Some(variant) = decl.variants.iter().find(|variant| variant.name == part) else {
                bail!(
                    "enum target path '{}' references unknown variant '{}::{}'",
                    target,
                    current,
                    part
                );
            };
            current = variant.ty.as_deref().unwrap_or("");
        }
        if current.is_empty() {
            bail!("enum target path '{}' ends at payload-free variant", target);
        }
        Ok(())
    }

    fn validate_operator_mappings(&self) -> Result<()> {
        let prefix_tokens = self
            .grammar
            .expressions
            .items
            .iter()
            .flat_map(|expr| expr.prefix.iter().map(|op| op.token.as_str()))
            .collect::<BTreeSet<_>>();
        let binary_tokens = self
            .grammar
            .expressions
            .items
            .iter()
            .flat_map(|expr| expr.infix.iter().map(|op| op.token.as_str()))
            .collect::<BTreeSet<_>>();

        let prefix_ty = self.operator_type("prefix")?;
        let binary_ty = self.operator_type("binary")?;
        self.validate_operator_group(
            "prefix",
            &prefix_ty,
            &prefix_tokens,
            &self.semantics.operators.prefix,
        )?;
        self.validate_operator_group(
            "binary",
            &binary_ty,
            &binary_tokens,
            &self.semantics.operators.binary,
        )?;
        Ok(())
    }

    fn operator_type(&self, label: &str) -> Result<String> {
        let scalar = format!("operator({label})");
        let mut found = BTreeSet::new();
        for rule in &self.semantics.lower {
            let Some(decl) = self.hir.get(rule.hir.as_str()).copied() else {
                continue;
            };
            for field in &rule.fields {
                if field.scalar.as_deref() == Some(scalar.as_str()) {
                    let hir_field = decl
                        .fields
                        .iter()
                        .find(|candidate| candidate.name == field.name)
                        .expect("validated");
                    let ty = TypeRef::parse(&hir_field.ty)?;
                    let Some(named) = ty.named() else {
                        bail!(
                            "{scalar} field '{}.{}' must use a named HIR enum type",
                            rule.hir,
                            field.name
                        );
                    };
                    found.insert(named.to_string());
                }
            }
        }
        match found.len() {
            1 => Ok(found.into_iter().next().expect("len checked")),
            0 => bail!("no HIR field uses scalar '{scalar}'"),
            _ => bail!("scalar '{scalar}' is used with multiple HIR types: {found:?}"),
        }
    }

    fn validate_operator_group(
        &self,
        label: &str,
        hir_enum: &str,
        expected: &BTreeSet<&str>,
        mappings: &[OperatorMapping],
    ) -> Result<()> {
        ensure_unique(
            &format!("{label} operator mapping"),
            mappings.iter().map(|mapping| mapping.token.as_str()),
        )?;
        let Some(decl) = self.hir.get(hir_enum).copied() else {
            bail!("{label} operator mappings require HIR enum '{hir_enum}'");
        };
        let variants = decl
            .variants
            .iter()
            .map(|variant| variant.name.as_str())
            .collect::<BTreeSet<_>>();
        for mapping in mappings {
            ensure_rust_type("operator variant", &mapping.variant)?;
            if !expected.contains(mapping.token.as_str()) {
                bail!(
                    "{label} operator mapping references unknown token '{}'",
                    mapping.token
                );
            }
            if !variants.contains(mapping.variant.as_str()) {
                bail!(
                    "{label} operator mapping references unknown variant '{}::{}'",
                    hir_enum,
                    mapping.variant
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
}

fn syntax_nodes(grammar: &Grammar) -> BTreeSet<&str> {
    let mut nodes = grammar
        .syntax
        .nodes
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for expr in &grammar.expressions.items {
        nodes.insert(expr.root_node.as_str());
        nodes.extend(expr.prefix.iter().map(|item| item.node.as_str()));
        nodes.extend(expr.infix.iter().map(|item| item.node.as_str()));
        nodes.extend(expr.postfix.iter().map(|item| item.node.as_str()));
    }
    nodes
}

fn syntax_accessors(grammar: &Grammar) -> BTreeMap<String, BTreeMap<String, AccessorShape>> {
    let mut accessors = BTreeMap::new();
    for rule in &grammar.syntax.rules {
        let mut node_accessors = BTreeMap::new();
        collect_accessors_for_items(&rule.pattern, &mut node_accessors, false);
        if emits_text_convenience(&rule.pattern) {
            node_accessors.insert(
                "text".to_string(),
                AccessorShape::Scalar("String".to_string()),
            );
        }
        if emits_raw_text_convenience(&rule.pattern) {
            node_accessors.insert(
                "raw_text".to_string(),
                AccessorShape::Scalar("String".to_string()),
            );
        }
        node_accessors.insert(
            "syntax".to_string(),
            AccessorShape::Scalar("SyntaxNode".to_string()),
        );
        accessors.insert(rule.name.clone(), node_accessors);
    }
    for expression in &grammar.expressions.items {
        accessors
            .entry(expression.root_node.clone())
            .or_default()
            .insert(
                "kind".to_string(),
                AccessorShape::Scalar(format!("{}Kind", expression.name)),
            );
        for prefix in &expression.prefix {
            let entry = accessors.entry(prefix.node.clone()).or_default();
            entry.insert(
                "op".to_string(),
                AccessorShape::Scalar("SyntaxToken".to_string()),
            );
            entry.insert(
                "expr".to_string(),
                AccessorShape::One(expression.name.clone()),
            );
        }
        for infix in &expression.infix {
            let entry = accessors.entry(infix.node.clone()).or_default();
            entry.insert(
                "left".to_string(),
                AccessorShape::One(expression.name.clone()),
            );
            entry.insert(
                "right".to_string(),
                AccessorShape::One(expression.name.clone()),
            );
            entry.insert(
                "op".to_string(),
                AccessorShape::Scalar("SyntaxToken".to_string()),
            );
        }
        for postfix in &expression.postfix {
            let mut nested_many = Vec::new();
            for item in &postfix.pattern {
                if let Some(node) = &item.node {
                    if let Some(nested) = accessors.get(node).cloned() {
                        for (name, shape) in nested {
                            if matches!(shape, AccessorShape::Many(_)) {
                                nested_many.push((name, shape));
                            }
                        }
                    }
                }
            }
            let entry = accessors.entry(postfix.node.clone()).or_default();
            entry.insert(
                "callee".to_string(),
                AccessorShape::One(expression.name.clone()),
            );
            for (name, shape) in nested_many {
                entry.insert(name, shape);
            }
        }
    }
    accessors
}

fn collect_accessors_for_items(
    items: &[PatternItem],
    accessors: &mut BTreeMap<String, AccessorShape>,
    repeated: bool,
) {
    for item in items {
        if let Some(label) = &item.label {
            if let Some(target) = item.node.as_ref().or(item.expr.as_ref()) {
                let shape = if repeated {
                    AccessorShape::Many(target.clone())
                } else {
                    AccessorShape::One(target.clone())
                };
                accessors.insert(label.clone(), shape);
            } else if item.token.is_some()
                || item
                    .choice
                    .as_ref()
                    .is_some_and(|choice| choice_is_tokens(choice))
            {
                accessors.insert(
                    label.clone(),
                    AccessorShape::Scalar("SyntaxToken".to_string()),
                );
            } else if let Some(repeat) = &item.repeat {
                if let Some(target) = repeated_target(repeat) {
                    accessors.insert(label.clone(), AccessorShape::Many(target.to_string()));
                } else if repeated_token_set(repeat).is_some() {
                    accessors.insert(
                        label.clone(),
                        AccessorShape::Many("SyntaxToken".to_string()),
                    );
                }
            } else if let Some(optional) = &item.optional {
                if let Some(target) = optional_target(optional) {
                    accessors.insert(label.clone(), AccessorShape::One(target.to_string()));
                }
            } else if let (Some(enum_name), Some(_)) = (&item.r#enum, &item.choice) {
                accessors.insert(label.clone(), AccessorShape::Scalar(enum_name.clone()));
            }
        }
        if let Some(repeat) = &item.repeat {
            collect_accessors_for_items(repeat, accessors, true);
        }
        if let Some(optional) = &item.optional {
            collect_accessors_for_items(optional, accessors, repeated);
        }
        if let Some(choice) = &item.choice {
            collect_accessors_for_items(choice, accessors, repeated);
        }
    }
}

fn repeated_target(items: &[PatternItem]) -> Option<&str> {
    items.iter().find_map(|item| {
        item.node
            .as_deref()
            .or(item.expr.as_deref())
            .or_else(|| item.repeat.as_deref().and_then(repeated_target))
    })
}

fn optional_target(items: &[PatternItem]) -> Option<&str> {
    items
        .iter()
        .find_map(|item| item.node.as_deref().or(item.expr.as_deref()))
}

fn repeated_token_set(items: &[PatternItem]) -> Option<&str> {
    items.iter().find_map(|item| item.token_set.as_deref())
}

fn choice_is_tokens(choice: &[PatternItem]) -> bool {
    choice.iter().all(|item| {
        item.token.is_some()
            || item
                .choice
                .as_ref()
                .is_some_and(|choice| choice_is_tokens(choice))
    })
}

fn emits_text_convenience(pattern: &[PatternItem]) -> bool {
    pattern.len() == 1 && pattern_item_is_only_tokens(&pattern[0])
}

fn pattern_item_is_only_tokens(item: &PatternItem) -> bool {
    item.token.is_some()
        || item
            .choice
            .as_ref()
            .is_some_and(|choice| choice.iter().all(pattern_item_is_only_tokens))
}

fn emits_raw_text_convenience(pattern: &[PatternItem]) -> bool {
    pattern
        .iter()
        .any(|item| item.label.as_deref() == Some("tokens") && item.repeat.is_some())
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
    grammar: &Grammar,
    semantics: &SemanticsSpec,
    header: &str,
) -> Vec<(&'static str, String)> {
    vec![
        ("src/generated/mod.rs", gen_mod(header)),
        ("src/generated/diagnostic.rs", gen_diagnostic(header)),
        ("src/generated/hir.rs", gen_hir(header, semantics)),
        (
            "src/generated/lower.rs",
            gen_lower(header, grammar, semantics),
        ),
    ]
}

fn gen_mod(header: &str) -> String {
    format!("{header}pub mod diagnostic;\npub mod hir;\npub mod lower;\n")
}

fn gen_diagnostic(header: &str) -> String {
    format!(
        r#"{header}use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LowerDiagnostic {{
    pub kind: LowerDiagnosticKind,
    pub range: Option<Range<usize>>,
}}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerDiagnosticKind {{
    MissingRequiredSyntax {{ parent: &'static str, field: &'static str }},
    UnknownOperator {{ operator: String }},
}}

impl LowerDiagnostic {{
    pub fn missing_required(parent: &'static str, field: &'static str, range: Range<usize>) -> Self {{
        Self {{
            kind: LowerDiagnosticKind::MissingRequiredSyntax {{ parent, field }},
            range: Some(range),
        }}
    }}

    pub fn unknown_operator(operator: impl Into<String>, range: Range<usize>) -> Self {{
        Self {{
            kind: LowerDiagnosticKind::UnknownOperator {{
                operator: operator.into(),
            }},
            range: Some(range),
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

fn gen_hir(header: &str, semantics: &SemanticsSpec) -> String {
    let mut out = format!("{header}use dawn_syntax::ast;\n\n");
    for decl in &semantics.hir {
        let copy = decl.kind == HirDeclKind::Enum
            && decl.variants.iter().all(|variant| variant.ty.is_none());
        if copy {
            out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
        } else {
            out.push_str("#[derive(Debug, Clone, PartialEq, Eq)]\n");
        }
        match decl.kind {
            HirDeclKind::Struct => {
                out.push_str(&format!("pub struct {} {{\n", decl.name));
                for field in &decl.fields {
                    out.push_str(&format!("    pub {}: {},\n", field.name, field.ty));
                }
                if let Some(source) = &decl.source {
                    let ty = if source == "SyntaxNode" {
                        "dawn_syntax::SyntaxNode".to_string()
                    } else {
                        format!("ast::{source}")
                    };
                    out.push_str(&format!("    pub syntax: {ty},\n"));
                }
                out.push_str("}\n\n");
            }
            HirDeclKind::Enum => {
                out.push_str(&format!("pub enum {} {{\n", decl.name));
                for variant in &decl.variants {
                    if let Some(ty) = &variant.ty {
                        out.push_str(&format!("    {}({}),\n", variant.name, ty));
                    } else {
                        out.push_str(&format!("    {},\n", variant.name));
                    }
                }
                out.push_str("}\n\n");
            }
        }
    }
    out
}

fn gen_lower(header: &str, grammar: &Grammar, semantics: &SemanticsSpec) -> String {
    let accessors = syntax_accessors(grammar);
    let hir = semantics
        .hir
        .iter()
        .map(|decl| (decl.name.as_str(), decl))
        .collect::<BTreeMap<_, _>>();
    let mut out = format!(
        r#"{header}use dawn_syntax::ast;
use dawn_syntax::ast::AstNode;
use dawn_syntax::SyntaxKind;
use dawn_syntax::SyntaxNode;
use dawn_syntax::SyntaxToken;

use super::diagnostic::LowerDiagnostic;
use super::hir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweredSourceFile {{
    pub root: Option<hir::{root}>,
    pub diagnostics: Vec<LowerDiagnostic>,
}}

pub fn lower_parse(parse: &dawn_syntax::Parse) -> LoweredSourceFile {{
    let mut ctx = LowerCtx::default();
    let root = ast::{root_syntax}::cast(parse.syntax_node()).and_then(|source| ctx.{root_method}(source));
    LoweredSourceFile {{
        root,
        diagnostics: ctx.diagnostics,
    }}
}}

pub fn lower_source_file(source: ast::{root_syntax}) -> LoweredSourceFile {{
    let mut ctx = LowerCtx::default();
    let root = ctx.{root_method}(source);
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
"#,
        root = semantics.root,
        root_syntax = root_syntax(semantics),
        root_method = to_snake(root_syntax(semantics)),
    );

    for rule in &semantics.lower {
        let decl = hir.get(rule.hir.as_str()).expect("validated");
        match decl.kind {
            HirDeclKind::Struct => gen_struct_lower(&mut out, rule, decl, &accessors),
            HirDeclKind::Enum => gen_enum_lower(&mut out, rule, decl, &hir),
        }
    }

    let prefix_ty = operator_hir_type(semantics, &hir, "prefix");
    let binary_ty = operator_hir_type(semantics, &hir, "binary");
    gen_operator_lower(&mut out, "prefix", &prefix_ty, &semantics.operators.prefix);
    gen_operator_lower(&mut out, "binary", &binary_ty, &semantics.operators.binary);
    out.push_str(
        r#"    fn missing(&mut self, parent: &'static str, field: &'static str, range: std::ops::Range<usize>) {
        self.diagnostics
            .push(LowerDiagnostic::missing_required(parent, field, range));
    }
}

fn node_range(node: &SyntaxNode) -> std::ops::Range<usize> {
    let range = node.text_range();
    u32::from(range.start()) as usize..u32::from(range.end()) as usize
}

fn token_range(token: &SyntaxToken) -> std::ops::Range<usize> {
    let range = token.text_range();
    u32::from(range.start()) as usize..u32::from(range.end()) as usize
}
"#,
    );
    out
}

fn root_syntax(semantics: &SemanticsSpec) -> &str {
    semantics
        .lower
        .iter()
        .find(|rule| rule.hir == semantics.root)
        .map(|rule| rule.syntax.as_str())
        .expect("validated")
}

fn operator_hir_type(
    semantics: &SemanticsSpec,
    hir: &BTreeMap<&str, &HirDecl>,
    label: &str,
) -> String {
    let scalar = format!("operator({label})");
    for rule in &semantics.lower {
        let decl = hir.get(rule.hir.as_str()).expect("validated");
        for field in &rule.fields {
            if field.scalar.as_deref() == Some(scalar.as_str()) {
                return decl
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == field.name)
                    .map(|field| field.ty.clone())
                    .expect("validated");
            }
        }
    }
    unreachable!("validated")
}

fn gen_struct_lower(
    out: &mut String,
    rule: &LowerRule,
    decl: &HirDecl,
    accessors: &BTreeMap<String, BTreeMap<String, AccessorShape>>,
) {
    out.push_str(&format!(
        "    fn {}(&mut self, syntax: ast::{}) -> Option<hir::{}> {{\n",
        to_snake(&rule.syntax),
        rule.syntax,
        rule.hir
    ));
    let plans = rule
        .fields
        .iter()
        .map(|field| {
            let hir_field = decl
                .fields
                .iter()
                .find(|candidate| candidate.name == field.name)
                .expect("validated");
            FieldPlan {
                name: field.name.clone(),
                ty: TypeRef::parse(&hir_field.ty).expect("validated"),
                accessor: field.accessor.split('.').map(str::to_string).collect(),
                scalar: field.scalar.clone(),
            }
        })
        .collect::<Vec<_>>();
    for plan in &plans {
        let shape = accessor_path_shape(accessors, &rule.syntax, &plan.accessor);
        let expr = lower_field_expr(rule, plan, &shape);
        out.push_str(&format!("        let {} = {};\n", plan.name, expr));
    }
    out.push_str(&format!("        Some(hir::{} {{\n", rule.hir));
    for field in &decl.fields {
        out.push_str(&format!("            {},\n", field.name));
    }
    if decl.source.is_some() {
        out.push_str("            syntax,\n");
    }
    out.push_str("        })\n    }\n\n");
}

fn lower_field_expr(rule: &LowerRule, plan: &FieldPlan, shape: &AccessorShape) -> String {
    if let Some(scalar) = &plan.scalar {
        return scalar_expr(rule, plan, scalar);
    }
    match &plan.ty {
        TypeRef::Vec(_inner) => {
            let item_method = to_snake(shape.target());
            if plan.accessor.len() == 1 {
                format!(
                    "{access}.into_iter().filter_map(|item| self.{item_method}(item)).collect()",
                    access = access_expr("syntax", &plan.accessor, false, true)
                )
            } else {
                let (prefix, final_part) = plan.accessor.split_at(plan.accessor.len() - 1);
                format!(
                    "{{ let Some(value) = {prefix} else {{ self.missing(\"{parent}\", \"{field}\", node_range(syntax.syntax())); return None; }}; value.{final_part}().into_iter().filter_map(|item| self.{item_method}(item)).collect() }}",
                    prefix = access_expr("syntax", prefix, true, false),
                    parent = rule.syntax,
                    field = plan.name,
                    final_part = final_part[0]
                )
            }
        }
        TypeRef::Option(inner) => {
            let method = to_snake(shape.target());
            let lowered = if matches!(**inner, TypeRef::Box(_)) {
                format!(".and_then(|value| self.{method}(value).map(Box::new))")
            } else {
                format!(".and_then(|value| self.{method}(value))")
            };
            format!(
                "{}{lowered}",
                access_expr("syntax", &plan.accessor, true, false)
            )
        }
        TypeRef::Box(_) => {
            let method = to_snake(shape.target());
            format!(
                "{{ let value = {}; Box::new(value) }}",
                required_lower_expr(rule, plan, &method)
            )
        }
        TypeRef::Named(_) => {
            let method = to_snake(shape.target());
            required_lower_expr(rule, plan, &method)
        }
    }
}

fn scalar_expr(rule: &LowerRule, plan: &FieldPlan, scalar: &str) -> String {
    match scalar {
        "text" | "raw_text" => format!(
            "{{ let Some(value) = {access} else {{ self.missing(\"{parent}\", \"{field}\", node_range(syntax.syntax())); return None; }}; value }}",
            access = access_expr("syntax", &plan.accessor, true, false),
            parent = rule.syntax,
            field = plan.name
        ),
        "bool" => format!(
            "{{ let Some(value) = {access} else {{ self.missing(\"{parent}\", \"{field}\", node_range(syntax.syntax())); return None; }}; value == \"true\" }}",
            access = access_expr("syntax", &plan.accessor, true, false),
            parent = rule.syntax,
            field = plan.name
        ),
        "operator(prefix)" => operator_field_expr(rule, plan, "prefix_op"),
        "operator(binary)" => operator_field_expr(rule, plan, "binary_op"),
        _ => unreachable!("validated"),
    }
}

fn operator_field_expr(rule: &LowerRule, plan: &FieldPlan, method: &str) -> String {
    format!(
        "{{ let Some(op_token) = {access} else {{ self.missing(\"{parent}\", \"{field}\", node_range(syntax.syntax())); return None; }}; self.{method}(op_token.kind(), op_token.text(), token_range(&op_token))? }}",
        access = access_expr("syntax", &plan.accessor, true, false),
        parent = rule.syntax,
        field = plan.name
    )
}

fn required_lower_expr(rule: &LowerRule, plan: &FieldPlan, method: &str) -> String {
    format!(
        "{{ let Some(value) = {access} else {{ self.missing(\"{parent}\", \"{field}\", node_range(syntax.syntax())); return None; }}; self.{method}(value)? }}",
        access = access_expr("syntax", &plan.accessor, true, false),
        parent = rule.syntax,
        field = plan.name
    )
}

fn access_expr(base: &str, parts: &[String], optional_chain: bool, final_many: bool) -> String {
    let mut expr = base.to_string();
    for (index, part) in parts.iter().enumerate() {
        let last = index == parts.len() - 1;
        if index == 0 {
            expr = format!("{expr}.{part}()");
        } else if last && final_many {
            expr = format!("{expr}.map(|value| value.{part}()).unwrap_or_default()");
        } else if optional_chain {
            expr = format!("{expr}.and_then(|value| value.{part}())");
        } else {
            expr = format!("{expr}.and_then(|value| value.{part}())");
        }
    }
    expr
}

fn accessor_path_shape(
    accessors: &BTreeMap<String, BTreeMap<String, AccessorShape>>,
    syntax: &str,
    parts: &[String],
) -> AccessorShape {
    let mut current = syntax.to_string();
    let mut shape = None;
    for part in parts {
        let item = accessors
            .get(&current)
            .and_then(|items| items.get(part))
            .expect("validated")
            .clone();
        current = item.target().to_string();
        shape = Some(item);
    }
    shape.expect("validated")
}

fn gen_enum_lower(
    out: &mut String,
    rule: &LowerRule,
    decl: &HirDecl,
    hir: &BTreeMap<&str, &HirDecl>,
) {
    let dispatch = rule.dispatch.as_deref().unwrap_or("kind");
    out.push_str(&format!(
        "    fn {}(&mut self, syntax: ast::{}) -> Option<hir::{}> {{\n",
        to_snake(&rule.syntax),
        rule.syntax,
        rule.hir
    ));
    out.push_str(&format!("        match syntax.{dispatch}()? {{\n"));
    for variant in &rule.variants {
        let lower_method = to_snake(&variant.syntax);
        let maps = wrapper_chain(&decl.name, &variant.target, hir);
        out.push_str(&format!(
            "            ast::{}Kind::{}(node) => self.{lower_method}(node){}{},\n",
            rule.syntax,
            variant.syntax,
            maps.iter()
                .map(|wrapper| format!(".map(hir::{wrapper})"))
                .collect::<Vec<_>>()
                .join(""),
            if maps.is_empty() { "" } else { "" }
        ));
    }
    out.push_str("        }\n    }\n\n");
}

fn wrapper_chain(root: &str, target: &str, hir: &BTreeMap<&str, &HirDecl>) -> Vec<String> {
    let mut current = root.to_string();
    let mut wrappers = Vec::new();
    for part in target.split("::") {
        wrappers.push(format!("{current}::{part}"));
        let decl = hir.get(current.as_str()).expect("validated");
        let variant = decl
            .variants
            .iter()
            .find(|variant| variant.name == part)
            .expect("validated");
        current = variant.ty.clone().expect("validated");
    }
    wrappers.reverse();
    wrappers
}

fn gen_operator_lower(out: &mut String, label: &str, hir_enum: &str, mappings: &[OperatorMapping]) {
    out.push_str(&format!(
        "    fn {label}_op(&mut self, kind: SyntaxKind, text: &str, range: std::ops::Range<usize>) -> Option<hir::{hir_enum}> {{\n"
    ));
    out.push_str("        match kind {\n");
    for mapping in mappings {
        out.push_str(&format!(
            "            SyntaxKind::{} => Some(hir::{}::{}),\n",
            mapping.token, hir_enum, mapping.variant
        ));
    }
    out.push_str(
        r#"            _ => {
                self.diagnostics.push(LowerDiagnostic::unknown_operator(text, range));
                None
            }
        }
    }

"#,
    );
}

fn to_snake(name: &str) -> String {
    let mut out = String::new();
    for (index, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
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
                            (name: "File", pattern: [(label: "name", node: "Name"), (label: "expr", expr: "Expr")]),
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
            root: "Program",
            hir: [
                (name: "Program", kind: struct, source: "File", fields: [(name: "name", ty: "Ident"), (name: "expr", ty: "Expr")]),
                (name: "Ident", kind: struct, source: "Name", fields: [(name: "text", ty: "String")]),
                (name: "Expr", kind: enum, variants: [
                    (name: "Ident", ty: "Ident"),
                    (name: "Prefix", ty: "PrefixExpr"),
                    (name: "Binary", ty: "BinaryExpr"),
                ]),
                (name: "PrefixExpr", kind: struct, source: "PrefixExpr", fields: [(name: "op", ty: "PrefixOp"), (name: "expr", ty: "Box<Expr>")]),
                (name: "BinaryExpr", kind: struct, source: "BinaryExpr", fields: [(name: "left", ty: "Box<Expr>"), (name: "op", ty: "BinaryOp"), (name: "right", ty: "Box<Expr>")]),
                (name: "PrefixOp", kind: enum, variants: [(name: "Neg")]),
                (name: "BinaryOp", kind: enum, variants: [(name: "Add")]),
            ],
            lower: [
                (syntax: "File", hir: "Program", fields: [(name: "name", accessor: "name"), (name: "expr", accessor: "expr")]),
                (syntax: "Name", hir: "Ident", fields: [(name: "text", accessor: "text", scalar: "text")]),
                (syntax: "Expr", hir: "Expr", dispatch: "kind", variants: [
                    (syntax: "Name", target: "Ident"),
                    (syntax: "PrefixExpr", target: "Prefix"),
                    (syntax: "BinaryExpr", target: "Binary"),
                ]),
                (syntax: "PrefixExpr", hir: "PrefixExpr", fields: [(name: "op", accessor: "op", scalar: "operator(prefix)"), (name: "expr", accessor: "expr")]),
                (syntax: "BinaryExpr", hir: "BinaryExpr", fields: [(name: "left", accessor: "left"), (name: "op", accessor: "op", scalar: "operator(binary)"), (name: "right", accessor: "right")]),
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
            "(name: \"Ident\", kind: struct, source: \"Name\", fields: [(name: \"text\", ty: \"String\")]),",
            "(name: \"Ident\", kind: struct, source: \"Name\", fields: [(name: \"text\", ty: \"String\")]),\n                (name: \"Ident\", kind: struct, fields: []),",
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

    #[test]
    fn rejects_missing_hir_field_population() {
        let grammar = grammar();
        let spec = valid_semantics().replace(
            "(syntax: \"File\", hir: \"Program\", fields: [(name: \"name\", accessor: \"name\"), (name: \"expr\", accessor: \"expr\")])",
            "(syntax: \"File\", hir: \"Program\", fields: [(name: \"name\", accessor: \"name\")])",
        );
        let error = semantics(&spec).validate(&grammar).unwrap_err().to_string();
        assert!(error.contains("does not populate HIR field"));
    }

    #[test]
    fn rejects_extra_lower_field() {
        let grammar = grammar();
        let spec = valid_semantics().replace(
            "(syntax: \"Name\", hir: \"Ident\", fields: [(name: \"text\", accessor: \"text\", scalar: \"text\")])",
            "(syntax: \"Name\", hir: \"Ident\", fields: [(name: \"text\", accessor: \"text\", scalar: \"text\"), (name: \"extra\", accessor: \"text\", scalar: \"text\")])",
        );
        let error = semantics(&spec).validate(&grammar).unwrap_err().to_string();
        assert!(error.contains("unknown HIR field"));
    }

    #[test]
    fn rejects_invalid_enum_target_path() {
        let grammar = grammar();
        let spec = valid_semantics().replace("target: \"Binary\"", "target: \"Missing\"");
        let error = semantics(&spec).validate(&grammar).unwrap_err().to_string();
        assert!(error.contains("unknown variant"));
    }

    #[test]
    fn rejects_invalid_operator_variant() {
        let grammar = grammar();
        let spec = valid_semantics().replace("variant: \"Add\"", "variant: \"Missing\"");
        let error = semantics(&spec).validate(&grammar).unwrap_err().to_string();
        assert!(error.contains("unknown variant"));
    }

    #[test]
    fn generated_hir_uses_toy_names() {
        let spec = semantics(&valid_semantics());
        let hir = gen_hir("", &spec);
        assert!(hir.contains("pub struct Program"));
        assert!(hir.contains("pub enum PrefixOp"));
        assert!(!hir.contains("SourceFile"));
    }

    #[test]
    fn generated_lower_changes_with_rules() {
        let grammar = grammar();
        let mut spec = semantics(&valid_semantics());
        let lower = gen_lower("", &grammar, &spec);
        assert!(lower.contains("fn file"));
        spec.lower[0].fields[0].accessor = "expr".to_string();
        let changed = gen_lower("", &grammar, &spec);
        assert_ne!(lower, changed);
    }
}
