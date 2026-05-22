use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use ron::extensions::Extensions;
use serde::Deserialize;

#[derive(Debug)]
pub struct Grammar {
    pub tokens: TokensSpec,
    pub syntax: SyntaxSpec,
    pub ast: AstSpec,
    pub precedence: PrecedenceSpec,
    pub diagnostics: DiagnosticsSpec,
}

impl Grammar {
    pub fn load(path: &Path) -> Result<Self> {
        Ok(Self {
            tokens: load_ron(&path.join("tokens.ron"))?,
            syntax: load_ron(&path.join("syntax.ron"))?,
            ast: load_ron(&path.join("ast.ron"))?,
            precedence: load_ron(&path.join("precedence.ron"))?,
            diagnostics: load_ron(&path.join("diagnostics.ron"))?,
        })
    }

    pub fn syntax_kinds(&self) -> Vec<&str> {
        let mut kinds = Vec::new();
        kinds.extend(self.tokens.trivia.iter().map(|item| item.name.as_str()));
        kinds.extend(self.tokens.keywords.iter().map(|(name, _)| name.as_str()));
        kinds.extend(self.tokens.literals.iter().map(|item| item.name.as_str()));
        kinds.extend(self.tokens.punctuation.iter().map(|(name, _)| name.as_str()));
        kinds.extend(self.syntax.nodes.iter().map(String::as_str));
        kinds
    }

    pub fn token_names(&self) -> BTreeSet<&str> {
        self.tokens
            .trivia
            .iter()
            .map(|item| item.name.as_str())
            .chain(self.tokens.keywords.iter().map(|(name, _)| name.as_str()))
            .chain(self.tokens.literals.iter().map(|item| item.name.as_str()))
            .chain(self.tokens.punctuation.iter().map(|(name, _)| name.as_str()))
            .collect()
    }

    pub fn validate(&self) -> Result<()> {
        ensure_unique("syntax kind", self.syntax_kinds())?;
        ensure_unique("node", self.syntax.nodes.iter().map(String::as_str))?;
        ensure_unique(
            "diagnostic",
            self.diagnostics.kinds.iter().map(|kind| kind.name.as_str()),
        )?;

        let tokens = self.token_names();
        let nodes = self.syntax.nodes.iter().map(String::as_str).collect::<BTreeSet<_>>();

        if !nodes.contains(self.syntax.entry.as_str()) {
            bail!("syntax entry node '{}' is not declared", self.syntax.entry);
        }

        for (_, token) in &self.syntax.doc_kinds {
            require_token(&tokens, token)?;
        }

        for rule in &self.syntax.rules {
            if !nodes.contains(rule.name.as_str()) {
                bail!("rule '{}' references undeclared node", rule.name);
            }
            let token_sets = self
                .syntax
                .token_sets
                .iter()
                .map(|set| set.name.as_str())
                .collect::<BTreeSet<_>>();
            validate_pattern(&rule.pattern, &tokens, &nodes, &token_sets)?;
        }

        for set in &self.syntax.token_sets {
            ensure_name("token set", &set.name)?;
            if let Some(include) = &set.include {
                if include != "all_tokens" {
                    bail!("token set '{}' has invalid include '{}'", set.name, include);
                }
            }
            for token in &set.tokens {
                require_token(&tokens, token)?;
            }
            for token in &set.exclude {
                require_token(&tokens, token)?;
            }
        }

        for recovery in &self.syntax.recovery {
            if !nodes.contains(recovery.rule.as_str()) {
                bail!("recovery references undeclared rule '{}'", recovery.rule);
            }
            ensure_name("recovery strategy", &recovery.strategy)?;
        }

        let diagnostics = self
            .diagnostics
            .kinds
            .iter()
            .map(|kind| kind.name.as_str())
            .collect::<BTreeSet<_>>();
        for trivia in &self.tokens.trivia {
            ensure_name("trivia", &trivia.name)?;
            if trivia.pattern.is_none() && (trivia.start.is_none() || trivia.end.is_none()) {
                bail!("trivia '{}' needs pattern or start/end delimiters", trivia.name);
            }
            if trivia.error.is_some() && trivia.end.is_none() {
                bail!("trivia '{}' declares an error without an end delimiter", trivia.name);
            }
            if let Some(error) = &trivia.error {
                require_diagnostic(&diagnostics, error)?;
            }
            let _ = trivia.skip;
        }
        for literal in &self.tokens.literals {
            ensure_name("literal", &literal.name)?;
            if literal.pattern.is_none() && literal.patterns.is_empty() {
                bail!("literal '{}' needs pattern or patterns", literal.name);
            }
            if literal.normalize.as_deref().is_some_and(|value| value != "remove_underscores") {
                bail!("literal '{}' has unsupported normalize mode", literal.name);
            }
            if let Some(error) = &literal.error {
                require_diagnostic(&diagnostics, error)?;
            }
            let _ = &literal.value;
        }
        for error in &self.tokens.errors {
            require_diagnostic(&diagnostics, error)?;
        }

        if !nodes.contains(self.ast.root.as_str()) {
            bail!("AST root '{}' is not declared as a syntax node", self.ast.root);
        }
        let ast_nodes = self
            .ast
            .nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect::<BTreeSet<_>>();
        for ast_node in &self.ast.nodes {
            if !nodes.contains(ast_node.syntax.as_str()) {
                bail!(
                    "AST node '{}' references undeclared syntax node '{}'",
                    ast_node.name,
                    ast_node.syntax
                );
            }
            for accessor in &ast_node.accessors {
                match accessor.kind.as_str() {
                    "child" | "children" => {
                        let Some(node) = &accessor.node else {
                            bail!("AST accessor '{}.{}' is missing node", ast_node.name, accessor.name);
                        };
                        if !ast_nodes.contains(node.as_str()) {
                            bail!("AST accessor '{}.{}' references undeclared AST node '{}'", ast_node.name, accessor.name, node);
                        }
                    }
                    "token" => {
                        let Some(token) = &accessor.token else {
                            bail!("AST accessor '{}.{}' is missing token", ast_node.name, accessor.name);
                        };
                        require_token(&tokens, token)?;
                    }
                    "first_token" => {}
                    "text_between" => {
                        require_token(&tokens, accessor.start.as_deref().unwrap_or(""))?;
                        require_token(&tokens, accessor.end.as_deref().unwrap_or(""))?;
                    }
                    other => bail!("AST accessor '{}.{}' has invalid kind '{}'", ast_node.name, accessor.name, other),
                }
            }
        }

        for ast_enum in &self.ast.enums {
            if !nodes.contains(ast_enum.from_node.as_str()) {
                bail!("AST enum '{}' references undeclared node '{}'", ast_enum.name, ast_enum.from_node);
            }
            for (_, token) in &ast_enum.variants {
                require_token(&tokens, token)?;
            }
        }

        for tier in &self.precedence.tiers {
            ensure_name("precedence tier", &tier.name)?;
            if tier.associativity != "left" && tier.associativity != "right" {
                bail!("precedence tier '{}' has invalid associativity '{}'", tier.name, tier.associativity);
            }
            for token in &tier.operators {
                require_token(&tokens, token)?;
            }
        }
        for (_, token) in &self.precedence.prefix {
            require_token(&tokens, token)?;
        }
        for postfix in &self.precedence.postfix {
            ensure_name("postfix operator", postfix)?;
        }

        Ok(())
    }
}

fn load_ron<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    ron::Options::default()
        .with_default_extension(Extensions::IMPLICIT_SOME)
        .from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))
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

fn ensure_name(label: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{label} name cannot be empty");
    }
    Ok(())
}

fn require_token(tokens: &BTreeSet<&str>, token: &str) -> Result<()> {
    if !tokens.contains(token) {
        bail!("undeclared token '{token}'");
    }
    Ok(())
}

fn require_diagnostic(diagnostics: &BTreeSet<&str>, diagnostic: &str) -> Result<()> {
    if !diagnostics.contains(diagnostic) {
        bail!("undeclared diagnostic '{diagnostic}'");
    }
    Ok(())
}

fn validate_pattern(
    pattern: &[PatternItem],
    tokens: &BTreeSet<&str>,
    nodes: &BTreeSet<&str>,
    token_sets: &BTreeSet<&str>,
) -> Result<()> {
    for item in pattern {
        item.validate_shape()?;
        if let Some(token) = &item.token {
            require_token(tokens, token)?;
        }
        if let Some(node) = &item.node {
            if !nodes.contains(node.as_str()) {
                bail!("undeclared node '{node}'");
            }
        }
        if let Some(repeat) = &item.repeat {
            if !nodes.contains(repeat.as_str()) && !token_sets.contains(repeat.as_str()) {
                bail!("repeat references undeclared node or token set '{repeat}'");
            }
        }
        if let Some(choice) = &item.choice {
            validate_pattern(choice, tokens, nodes, token_sets)?;
        }
        if let Some(token_set) = &item.token_set {
            if !token_sets.contains(token_set.as_str()) {
                bail!("undeclared token set '{token_set}'");
            }
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct TokensSpec {
    pub trivia: Vec<TriviaSpec>,
    pub keywords: Vec<(String, String)>,
    pub literals: Vec<LiteralSpec>,
    pub punctuation: Vec<(String, String)>,
    pub errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct DiagnosticsSpec {
    pub kinds: Vec<DiagnosticSpec>,
}

#[derive(Debug, Deserialize)]
pub struct DiagnosticSpec {
    pub name: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct TriviaSpec {
    pub name: String,
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
    #[serde(default)]
    pub skip: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LiteralSpec {
    pub name: String,
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub normalize: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SyntaxSpec {
    pub entry: String,
    pub doc_kinds: Vec<(String, String)>,
    pub nodes: Vec<String>,
    pub rules: Vec<RuleSpec>,
    pub token_sets: Vec<TokenSetSpec>,
    pub recovery: Vec<RecoverySpec>,
}

#[derive(Debug, Deserialize)]
pub struct RuleSpec {
    pub name: String,
    pub pattern: Vec<PatternItem>,
}

#[derive(Debug, Deserialize)]
pub struct PatternItem {
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub node: Option<String>,
    #[serde(default)]
    pub repeat: Option<String>,
    #[serde(default)]
    pub choice: Option<Vec<PatternItem>>,
    #[serde(default)]
    pub token_set: Option<String>,
}

impl PatternItem {
    fn validate_shape(&self) -> Result<()> {
        let fields = [
            self.token.is_some(),
            self.node.is_some(),
            self.repeat.is_some(),
            self.choice.is_some(),
            self.token_set.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count();
        if fields != 1 {
            bail!("pattern item must have exactly one field");
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct TokenSetSpec {
    pub name: String,
    #[serde(default)]
    pub tokens: Vec<String>,
    #[serde(default)]
    pub include: Option<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RecoverySpec {
    pub rule: String,
    pub strategy: String,
}

#[derive(Debug, Deserialize)]
pub struct AstSpec {
    pub root: String,
    pub nodes: Vec<AstNodeSpec>,
    pub enums: Vec<AstEnumSpec>,
}

#[derive(Debug, Deserialize)]
pub struct AstNodeSpec {
    pub name: String,
    pub syntax: String,
    pub accessors: Vec<AccessorSpec>,
}

#[derive(Debug, Deserialize)]
pub struct AccessorSpec {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub node: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AstEnumSpec {
    pub name: String,
    pub from_node: String,
    pub variants: Vec<(String, String)>,
}

#[derive(Debug, Deserialize)]
pub struct PrecedenceSpec {
    pub tiers: Vec<PrecedenceTier>,
    pub prefix: Vec<(String, String)>,
    pub postfix: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct PrecedenceTier {
    pub name: String,
    pub associativity: String,
    pub operators: Vec<String>,
}
