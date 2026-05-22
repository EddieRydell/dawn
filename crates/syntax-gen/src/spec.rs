use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use regex::Regex;
use ron::extensions::Extensions;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grammar {
    pub version: u32,
    pub language: LanguageSpec,
    pub diagnostics: DiagnosticsSpec,
    pub tokens: TokensSpec,
    pub syntax: SyntaxSpec,
    pub expressions: ExpressionsSpec,
    pub ast: AstSpec,
}

impl Grammar {
    pub fn load(path: &Path) -> Result<Self> {
        load_ron(path)
    }

    pub fn syntax_kinds(&self) -> Vec<&str> {
        let mut kinds = Vec::new();
        kinds.extend(self.tokens.rules.iter().map(|item| item.name.as_str()));
        kinds.extend(self.syntax.nodes.iter().map(String::as_str));
        for expression in &self.expressions.items {
            kinds.extend(expression.node_kinds());
        }
        let mut seen = BTreeSet::new();
        kinds
            .into_iter()
            .filter(|kind| seen.insert(*kind))
            .collect()
    }

    pub fn token_names(&self) -> BTreeSet<&str> {
        self.tokens
            .rules
            .iter()
            .map(|item| item.name.as_str())
            .collect()
    }

    pub fn trivia_names(&self) -> BTreeSet<&str> {
        self.tokens
            .rules
            .iter()
            .filter(|item| matches!(item.trivia, TriviaPolicy::Emit | TriviaPolicy::Skip))
            .map(|item| item.name.as_str())
            .collect()
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 2 {
            bail!("syntax grammar version must be 2");
        }
        ensure_rust_type("language.type_name", &self.language.type_name)?;
        ensure_rust_type("language.syntax_kind", &self.language.syntax_kind)?;
        ensure_unique("syntax kind", self.syntax_kinds())?;
        ensure_unique(
            "token",
            self.tokens.rules.iter().map(|kind| kind.name.as_str()),
        )?;
        ensure_unique("node", self.syntax.nodes.iter().map(String::as_str))?;
        ensure_unique(
            "diagnostic",
            self.diagnostics.kinds.iter().map(|kind| kind.name.as_str()),
        )?;
        ensure_unique(
            "expression",
            self.expressions.items.iter().map(|item| item.name.as_str()),
        )?;

        let tokens = self.token_names();
        let nodes = self
            .syntax
            .nodes
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let diagnostics = self
            .diagnostics
            .kinds
            .iter()
            .map(|kind| kind.name.as_str())
            .collect::<BTreeSet<_>>();
        let expressions = self
            .expressions
            .items
            .iter()
            .map(|item| item.name.as_str())
            .collect::<BTreeSet<_>>();

        require_diagnostic(&diagnostics, &self.diagnostics.invalid_token)?;
        require_diagnostic(&diagnostics, &self.diagnostics.unexpected_token)?;
        require_diagnostic(&diagnostics, &self.diagnostics.unexpected_eof)?;

        if !nodes.contains(self.syntax.entry.as_str()) {
            bail!("syntax entry node '{}' is not declared", self.syntax.entry);
        }

        for diagnostic in &self.diagnostics.kinds {
            ensure_rust_type("diagnostic", &diagnostic.name)?;
        }

        for token in &self.tokens.rules {
            ensure_rust_type("token", &token.name)?;
            token.validate_shape()?;
            if let Some(pattern) = &token.regex {
                validate_regex("token", &token.name, pattern)?;
            }
            if let Some(diagnostic) = &token.diagnostic {
                require_diagnostic(&diagnostics, diagnostic)?;
            }
        }
        validate_unreachable_token_rules(&self.tokens.rules)?;

        let token_sets = self
            .syntax
            .token_sets
            .iter()
            .map(|set| set.name.as_str())
            .collect::<BTreeSet<_>>();
        ensure_unique(
            "token set",
            self.syntax.token_sets.iter().map(|set| set.name.as_str()),
        )?;

        for node in &self.syntax.nodes {
            ensure_rust_type("syntax node", node)?;
        }
        for rule in &self.syntax.rules {
            ensure_rust_type("rule", &rule.name)?;
            if !nodes.contains(rule.name.as_str()) {
                bail!("rule '{}' references undeclared node", rule.name);
            }
            validate_pattern(&rule.pattern, &tokens, &nodes, &token_sets, &expressions)?;
        }
        self.validate_generated_accessors()?;
        for node in &self.syntax.nodes {
            if !self.syntax.rules.iter().any(|rule| rule.name == *node) {
                bail!("syntax node '{node}' has no rule");
            }
        }

        for set in &self.syntax.token_sets {
            ensure_rust_type("token set", &set.name)?;
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

        self.validate_expressions(&tokens, &nodes)?;
        self.validate_ast(&nodes)?;
        self.validate_ll(&tokens)?;
        Ok(())
    }

    fn validate_generated_accessors(&self) -> Result<()> {
        let mut enum_names = BTreeSet::new();
        for rule in &self.syntax.rules {
            let mut labels = BTreeSet::from(["syntax".to_string()]);
            let mut variable_targets = BTreeSet::new();
            validate_accessor_items(
                &rule.pattern,
                &rule.name,
                &mut labels,
                &mut variable_targets,
                &mut enum_names,
                false,
            )?;
        }
        Ok(())
    }

    fn validate_ast(&self, nodes: &BTreeSet<&str>) -> Result<()> {
        if !nodes.contains(self.ast.root.as_str()) {
            bail!(
                "AST root '{}' is not declared as a syntax node",
                self.ast.root
            );
        }
        for node in &self.ast.nodes {
            if !nodes.contains(node.as_str()) {
                bail!("AST metadata references undeclared syntax node '{node}'");
            }
        }
        Ok(())
    }

    fn validate_expressions(&self, tokens: &BTreeSet<&str>, nodes: &BTreeSet<&str>) -> Result<()> {
        let expressions = self
            .expressions
            .items
            .iter()
            .map(|item| item.name.as_str())
            .collect::<BTreeSet<_>>();
        for expression in &self.expressions.items {
            ensure_rust_type("expression", &expression.name)?;
            if expression.atoms.is_empty() {
                bail!(
                    "expression '{}' must declare at least one atom",
                    expression.name
                );
            }
            for atom in &expression.atoms {
                atom.validate_shape()?;
                if let Some(token) = &atom.token {
                    require_token(tokens, token)?;
                }
                if let Some(node) = &atom.node {
                    if !nodes.contains(node.as_str()) {
                        bail!("expression atom references undeclared node '{node}'");
                    }
                }
            }
            for prefix in &expression.prefix {
                ensure_rust_type("prefix expression node", &prefix.node)?;
                require_token(tokens, &prefix.token)?;
            }
            let mut precedence_names = BTreeSet::new();
            for infix in &expression.infix {
                ensure_rust_type("infix expression node", &infix.node)?;
                require_token(tokens, &infix.token)?;
                if infix.associativity != "left" && infix.associativity != "right" {
                    bail!(
                        "infix operator '{}' has invalid associativity '{}'",
                        infix.token,
                        infix.associativity
                    );
                }
                if !precedence_names.insert(infix.precedence.as_str()) {
                    let _ = &infix.precedence;
                }
            }
            for postfix in &expression.postfix {
                ensure_rust_type("postfix expression node", &postfix.node)?;
                if postfix.pattern.is_empty() {
                    bail!(
                        "postfix expression node '{}' has empty pattern",
                        postfix.node
                    );
                }
                validate_pattern(
                    &postfix.pattern,
                    tokens,
                    nodes,
                    &BTreeSet::new(),
                    &expressions,
                )?;
                if pattern_nullable(&postfix.pattern) {
                    bail!(
                        "postfix expression node '{}' pattern is non-consuming",
                        postfix.node
                    );
                }
            }
        }
        Ok(())
    }

    fn validate_ll(&self, tokens: &BTreeSet<&str>) -> Result<()> {
        let sets = GrammarAnalysis::new(self);
        for cycle in sets.left_recursive_cycles() {
            bail!("left-recursive syntax rule cycle: {}", cycle.join(" -> "));
        }
        for cycle in sets.nullable_cycles() {
            bail!("nullable syntax rule cycle: {}", cycle.join(" -> "));
        }
        for (rule_index, rule) in self.syntax.rules.iter().enumerate() {
            for (item_index, item) in rule.pattern.iter().enumerate() {
                if let Some(repeat) = &item.repeat {
                    let first = first_of_sequence(
                        repeat,
                        &sets.first,
                        &sets.nullable_rules,
                        &sets.rule_indices,
                        &sets.token_sets,
                        &sets.token_set_indices,
                        &sets.expression_indices,
                        &sets.expression_first,
                    );
                    if first.is_empty() {
                        bail!("repeat item in rule '{}' cannot consume input", rule.name);
                    }
                    if pattern_nullable_with(
                        repeat,
                        &sets.nullable_rules,
                        &sets.rule_indices,
                        &sets.token_set_indices,
                        &sets.expression_indices,
                        &sets.expression_first,
                    ) {
                        bail!("repeat item in rule '{}' is nullable", rule.name);
                    }
                    let rest = &rule.pattern[item_index + 1..];
                    let mut item_follow = first_of_sequence(
                        rest,
                        &sets.first,
                        &sets.nullable_rules,
                        &sets.rule_indices,
                        &sets.token_sets,
                        &sets.token_set_indices,
                        &sets.expression_indices,
                        &sets.expression_first,
                    );
                    if pattern_nullable_with(
                        rest,
                        &sets.nullable_rules,
                        &sets.rule_indices,
                        &sets.token_set_indices,
                        &sets.expression_indices,
                        &sets.expression_first,
                    ) {
                        item_follow.extend(sets.follow[rule_index].iter().cloned());
                    }
                    let overlap = first
                        .intersection(&item_follow)
                        .cloned()
                        .collect::<Vec<_>>();
                    if !overlap.is_empty() {
                        bail!(
                            "repeat item in rule '{}' has unsafe FIRST/FOLLOW conflict on {:?}",
                            rule.name,
                            overlap
                        );
                    }
                }
                validate_choice_conflicts(item, &sets, &rule.name)?;
            }
        }
        for rule in sets.unreachable_rules() {
            bail!("unreachable syntax rule '{rule}'");
        }
        for set in sets.unused_token_sets() {
            bail!("unused token set '{set}'");
        }
        for token in tokens {
            let _ = token;
        }
        Ok(())
    }
}

fn load_ron<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    ron::Options::default()
        .with_default_extension(Extensions::IMPLICIT_SOME)
        .from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))
}

fn validate_regex(label: &str, name: &str, pattern: &str) -> Result<()> {
    let regex = Regex::new(&format!("^(?:{pattern})"))
        .with_context(|| format!("{label} '{name}' has invalid regex pattern {pattern:?}"))?;
    if regex.find("").is_some_and(|matched| matched.end() == 0) {
        bail!("{label} '{name}' regex pattern must not match an empty string");
    }
    Ok(())
}

fn validate_unreachable_token_rules(rules: &[TokenRuleSpec]) -> Result<()> {
    for (index, rule) in rules.iter().enumerate() {
        if let Some(literal) = &rule.literal {
            if rules[..index].iter().any(|previous| {
                previous
                    .literal
                    .as_ref()
                    .is_some_and(|other| other == literal)
            }) {
                bail!(
                    "token rule '{}' is unreachable duplicate literal",
                    rule.name
                );
            }
        }
    }
    Ok(())
}

fn validate_pattern(
    pattern: &[PatternItem],
    tokens: &BTreeSet<&str>,
    nodes: &BTreeSet<&str>,
    token_sets: &BTreeSet<&str>,
    expressions: &BTreeSet<&str>,
) -> Result<()> {
    for item in pattern {
        item.validate_shape()?;
        if let Some(label) = &item.label {
            ensure_rust_value("pattern label", label)?;
        }
        if let Some(enum_name) = &item.r#enum {
            ensure_rust_type("pattern enum", enum_name)?;
            if item.choice.is_none() {
                bail!("pattern enum '{enum_name}' can only be attached to a choice item");
            }
        }
        if let Some(token) = &item.token {
            require_token(tokens, token)?;
        }
        if let Some(node) = &item.node {
            if !nodes.contains(node.as_str()) {
                bail!("undeclared node '{node}'");
            }
        }
        if let Some(repeat) = &item.repeat {
            validate_pattern(repeat, tokens, nodes, token_sets, expressions)?;
        }
        if let Some(optional) = &item.optional {
            validate_pattern(optional, tokens, nodes, token_sets, expressions)?;
        }
        if let Some(choice) = &item.choice {
            validate_pattern(choice, tokens, nodes, token_sets, expressions)?;
        }
        if let Some(token_set) = &item.token_set {
            if !token_sets.contains(token_set.as_str()) {
                bail!("undeclared token set '{token_set}'");
            }
        }
        if let Some(expr) = &item.expr {
            if !expressions.contains(expr.as_str()) {
                bail!("undeclared expression '{expr}'");
            }
        }
    }
    Ok(())
}

fn validate_accessor_items(
    pattern: &[PatternItem],
    rule_name: &str,
    labels: &mut BTreeSet<String>,
    variable_targets: &mut BTreeSet<String>,
    enum_names: &mut BTreeSet<String>,
    in_variable: bool,
) -> Result<()> {
    for item in pattern {
        if let Some(enum_name) = &item.r#enum {
            if !enum_names.insert(enum_name.clone()) {
                bail!("duplicate generated enum '{enum_name}'");
            }
        }
        if let Some(label) = &item.label {
            if !labels.insert(label.clone()) {
                bail!("duplicate accessor label '{}.{}'", rule_name, label);
            }
            if let Some(target) = accessor_child_target(item) {
                if !in_variable && variable_targets.contains(target) {
                    bail!(
                        "accessor '{}.{}' is ambiguous after variable-length '{}' children",
                        rule_name,
                        label,
                        target
                    );
                }
            }
        }
        if let Some(repeat) = &item.repeat {
            collect_accessor_targets(repeat, variable_targets);
            let mut repeat_labels = BTreeSet::new();
            validate_accessor_items(
                repeat,
                rule_name,
                &mut repeat_labels,
                &mut BTreeSet::new(),
                enum_names,
                true,
            )?;
        }
        if let Some(optional) = &item.optional {
            validate_accessor_items(
                optional,
                rule_name,
                labels,
                variable_targets,
                enum_names,
                in_variable,
            )?;
        }
        if let Some(choice) = &item.choice {
            validate_accessor_items(
                choice,
                rule_name,
                labels,
                variable_targets,
                enum_names,
                in_variable,
            )?;
        }
    }
    Ok(())
}

fn accessor_child_target(item: &PatternItem) -> Option<&str> {
    item.node
        .as_deref()
        .or(item.expr.as_deref())
        .or_else(|| item.optional.as_deref().and_then(optional_accessor_target))
}

fn optional_accessor_target(items: &[PatternItem]) -> Option<&str> {
    items
        .iter()
        .find_map(|item| item.node.as_deref().or(item.expr.as_deref()))
}

fn collect_accessor_targets(items: &[PatternItem], targets: &mut BTreeSet<String>) {
    for item in items {
        if let Some(node) = &item.node {
            targets.insert(node.clone());
        }
        if let Some(expr) = &item.expr {
            targets.insert(expr.clone());
        }
        if let Some(repeat) = &item.repeat {
            collect_accessor_targets(repeat, targets);
        }
        if let Some(optional) = &item.optional {
            collect_accessor_targets(optional, targets);
        }
        if let Some(choice) = &item.choice {
            collect_accessor_targets(choice, targets);
        }
    }
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
        bail!("{label} name cannot be empty");
    };
    if !first.is_ascii_alphabetic() || (upper && !first.is_ascii_uppercase()) {
        bail!("{label} '{value}' is not a valid generated Rust identifier");
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        bail!("{label} '{value}' is not a valid generated Rust identifier");
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageSpec {
    pub type_name: String,
    pub syntax_kind: String,
    #[serde(default)]
    pub modules: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsSpec {
    pub invalid_token: String,
    pub unexpected_token: String,
    pub unexpected_eof: String,
    pub kinds: Vec<DiagnosticSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticSpec {
    pub name: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokensSpec {
    pub rules: Vec<TokenRuleSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenRuleSpec {
    pub name: String,
    #[serde(default)]
    pub regex: Option<String>,
    #[serde(default)]
    pub literal: Option<String>,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
    #[serde(default)]
    pub trivia: TriviaPolicy,
    #[serde(default)]
    pub diagnostic: Option<String>,
}

impl TokenRuleSpec {
    fn validate_shape(&self) -> Result<()> {
        let fields = [
            self.regex.is_some(),
            self.literal.is_some(),
            self.start.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count();
        if fields != 1 {
            bail!("token rule '{}' must have exactly one matcher", self.name);
        }
        if self.start.is_some() != self.end.is_some() {
            bail!(
                "token rule '{}' needs both start and end delimiters",
                self.name
            );
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriviaPolicy {
    #[default]
    None,
    Emit,
    Skip,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyntaxSpec {
    pub entry: String,
    pub nodes: Vec<String>,
    pub rules: Vec<RuleSpec>,
    pub token_sets: Vec<TokenSetSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSpec {
    pub name: String,
    #[serde(default = "default_ast_enabled")]
    pub ast: bool,
    pub pattern: Vec<PatternItem>,
}

fn default_ast_enabled() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct PatternItem {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default, rename = "enum")]
    pub r#enum: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub node: Option<String>,
    #[serde(default)]
    pub repeat: Option<Vec<PatternItem>>,
    #[serde(default)]
    pub optional: Option<Vec<PatternItem>>,
    #[serde(default)]
    pub choice: Option<Vec<PatternItem>>,
    #[serde(default)]
    pub token_set: Option<String>,
    #[serde(default)]
    pub expr: Option<String>,
}

impl PatternItem {
    fn validate_shape(&self) -> Result<()> {
        let fields = [
            self.token.is_some(),
            self.node.is_some(),
            self.repeat.is_some(),
            self.optional.is_some(),
            self.choice.is_some(),
            self.token_set.is_some(),
            self.expr.is_some(),
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct ExpressionsSpec {
    #[serde(default)]
    pub items: Vec<ExpressionSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpressionSpec {
    pub name: String,
    pub root_node: String,
    pub atoms: Vec<ExpressionAtomSpec>,
    #[serde(default)]
    pub prefix: Vec<PrefixExpressionSpec>,
    #[serde(default)]
    pub infix: Vec<InfixExpressionSpec>,
    #[serde(default)]
    pub postfix: Vec<PostfixExpressionSpec>,
}

impl ExpressionSpec {
    fn node_kinds(&self) -> Vec<&str> {
        let mut kinds = vec![self.root_node.as_str()];
        kinds.extend(self.prefix.iter().map(|item| item.node.as_str()));
        kinds.extend(self.infix.iter().map(|item| item.node.as_str()));
        kinds.extend(self.postfix.iter().map(|item| item.node.as_str()));
        kinds
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpressionAtomSpec {
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub node: Option<String>,
}

impl ExpressionAtomSpec {
    fn validate_shape(&self) -> Result<()> {
        let fields = [self.token.is_some(), self.node.is_some()]
            .into_iter()
            .filter(|present| *present)
            .count();
        if fields != 1 {
            bail!("expression atom must have exactly one field");
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrefixExpressionSpec {
    pub token: String,
    pub node: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InfixExpressionSpec {
    pub token: String,
    pub node: String,
    pub precedence: String,
    pub associativity: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostfixExpressionSpec {
    pub node: String,
    pub pattern: Vec<PatternItem>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AstSpec {
    pub root: String,
    #[serde(default)]
    pub nodes: Vec<String>,
}

struct GrammarAnalysis<'a> {
    grammar: &'a Grammar,
    rule_indices: BTreeMap<String, usize>,
    token_set_indices: BTreeMap<String, usize>,
    expression_indices: BTreeMap<String, usize>,
    expression_first: Vec<BTreeSet<String>>,
    token_sets: Vec<BTreeSet<String>>,
    first: Vec<BTreeSet<String>>,
    follow: Vec<BTreeSet<String>>,
    nullable_rules: Vec<bool>,
}

impl<'a> GrammarAnalysis<'a> {
    fn new(grammar: &'a Grammar) -> Self {
        let rule_indices = grammar
            .syntax
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| (rule.name.clone(), index))
            .collect::<BTreeMap<_, _>>();
        let token_set_indices = grammar
            .syntax
            .token_sets
            .iter()
            .enumerate()
            .map(|(index, set)| (set.name.clone(), index))
            .collect::<BTreeMap<_, _>>();
        let expression_indices = grammar
            .expressions
            .items
            .iter()
            .enumerate()
            .map(|(index, expression)| (expression.name.clone(), index))
            .collect::<BTreeMap<_, _>>();
        let all_tokens = grammar
            .token_names()
            .into_iter()
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        let token_sets = grammar
            .syntax
            .token_sets
            .iter()
            .map(|set| {
                let mut tokens = if set.include.as_deref() == Some("all_tokens") {
                    all_tokens.clone()
                } else {
                    set.tokens.iter().cloned().collect::<BTreeSet<_>>()
                };
                for token in &set.exclude {
                    tokens.remove(token);
                }
                tokens
            })
            .collect::<Vec<_>>();
        let expression_first = grammar
            .expressions
            .items
            .iter()
            .map(|expression| expression_first(expression))
            .collect::<Vec<_>>();

        let mut nullable_rules = vec![false; grammar.syntax.rules.len()];
        loop {
            let mut changed = false;
            for (index, rule) in grammar.syntax.rules.iter().enumerate() {
                if !nullable_rules[index]
                    && pattern_nullable_with(
                        &rule.pattern,
                        &nullable_rules,
                        &rule_indices,
                        &token_set_indices,
                        &expression_indices,
                        &expression_first,
                    )
                {
                    nullable_rules[index] = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        let mut first = vec![BTreeSet::new(); grammar.syntax.rules.len()];
        loop {
            let mut changed = false;
            for (index, rule) in grammar.syntax.rules.iter().enumerate() {
                let item_first = first_of_sequence(
                    &rule.pattern,
                    &first,
                    &nullable_rules,
                    &rule_indices,
                    &token_sets,
                    &token_set_indices,
                    &expression_indices,
                    &expression_first,
                );
                changed |= extend_set(&mut first[index], item_first);
            }
            if !changed {
                break;
            }
        }

        let mut follow = vec![BTreeSet::new(); grammar.syntax.rules.len()];
        loop {
            let mut changed = false;
            for (rule_index, rule) in grammar.syntax.rules.iter().enumerate() {
                changed |= collect_follow(
                    &rule.pattern,
                    &follow[rule_index].clone(),
                    &mut follow,
                    &first,
                    &nullable_rules,
                    &rule_indices,
                    &token_sets,
                    &token_set_indices,
                    &expression_indices,
                    &expression_first,
                );
            }
            if !changed {
                break;
            }
        }

        Self {
            grammar,
            rule_indices,
            token_set_indices,
            expression_indices,
            expression_first,
            token_sets,
            first,
            follow,
            nullable_rules,
        }
    }

    fn left_recursive_cycles(&self) -> Vec<Vec<String>> {
        let graph = self.rule_prefix_graph();
        find_cycles(&self.ordered_rule_names(), &graph)
    }

    fn nullable_cycles(&self) -> Vec<Vec<String>> {
        let mut graph = BTreeMap::<String, BTreeSet<String>>::new();
        for rule in &self.grammar.syntax.rules {
            let mut deps = BTreeSet::new();
            collect_rule_deps(&rule.pattern, &self.rule_indices, &mut deps);
            graph.insert(rule.name.clone(), deps);
        }
        find_cycles(&self.ordered_rule_names(), &graph)
            .into_iter()
            .filter(|cycle| {
                cycle.iter().all(|name| {
                    self.nullable_rules[*self.rule_indices.get(name).expect("validated")]
                })
            })
            .collect()
    }

    fn unreachable_rules(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(self.grammar.syntax.entry.clone());
        for expression in &self.grammar.expressions.items {
            for atom in &expression.atoms {
                if let Some(node) = &atom.node {
                    queue.push_back(node.clone());
                }
            }
            for postfix in &expression.postfix {
                collect_rule_refs(&postfix.pattern, &self.rule_indices, &mut queue);
            }
        }
        while let Some(rule_name) = queue.pop_front() {
            if !seen.insert(rule_name.clone()) {
                continue;
            }
            let rule =
                &self.grammar.syntax.rules[*self.rule_indices.get(&rule_name).expect("validated")];
            collect_rule_refs(&rule.pattern, &self.rule_indices, &mut queue);
        }
        self.grammar
            .syntax
            .rules
            .iter()
            .map(|rule| rule.name.clone())
            .filter(|name| !seen.contains(name))
            .collect()
    }

    fn unused_token_sets(&self) -> Vec<String> {
        let mut used = BTreeSet::new();
        for rule in &self.grammar.syntax.rules {
            collect_token_set_refs(&rule.pattern, &mut used);
        }
        self.grammar
            .syntax
            .token_sets
            .iter()
            .map(|set| set.name.clone())
            .filter(|name| !used.contains(name))
            .collect()
    }

    fn ordered_rule_names(&self) -> Vec<String> {
        self.grammar
            .syntax
            .rules
            .iter()
            .map(|rule| rule.name.clone())
            .collect()
    }

    fn rule_prefix_graph(&self) -> BTreeMap<String, BTreeSet<String>> {
        let mut graph = BTreeMap::new();
        for rule in &self.grammar.syntax.rules {
            let mut deps = BTreeSet::new();
            for item in &rule.pattern {
                if let Some(node) = &item.node {
                    deps.insert(node.clone());
                } else if let Some(choice) = &item.choice {
                    for alt in choice {
                        collect_prefix_node(alt, &mut deps);
                    }
                }
                if !item_nullable_with(
                    item,
                    &self.nullable_rules,
                    &self.rule_indices,
                    &self.token_set_indices,
                    &self.expression_indices,
                    &self.expression_first,
                ) {
                    break;
                }
            }
            graph.insert(rule.name.clone(), deps);
        }
        graph
    }
}

fn validate_choice_conflicts(
    item: &PatternItem,
    sets: &GrammarAnalysis<'_>,
    rule_name: &str,
) -> Result<()> {
    if let Some(choice) = &item.choice {
        let mut seen = BTreeSet::new();
        for alt in choice {
            let first = first_of_item(
                alt,
                &sets.first,
                &sets.nullable_rules,
                &sets.rule_indices,
                &sets.token_sets,
                &sets.token_set_indices,
                &sets.expression_indices,
                &sets.expression_first,
            );
            let overlap = seen.intersection(&first).cloned().collect::<Vec<_>>();
            if !overlap.is_empty() {
                bail!(
                    "choice in rule '{}' has FIRST/FIRST conflict on {:?}",
                    rule_name,
                    overlap
                );
            }
            seen.extend(first);
            validate_choice_conflicts(alt, sets, rule_name)?;
        }
    }
    Ok(())
}

fn collect_rule_refs(
    pattern: &[PatternItem],
    rule_indices: &BTreeMap<String, usize>,
    queue: &mut VecDeque<String>,
) {
    for item in pattern {
        if let Some(node) = &item.node {
            if rule_indices.contains_key(node) {
                queue.push_back(node.clone());
            }
        }
        if let Some(repeat) = &item.repeat {
            collect_rule_refs(repeat, rule_indices, queue);
        }
        if let Some(optional) = &item.optional {
            collect_rule_refs(optional, rule_indices, queue);
        }
        if let Some(choice) = &item.choice {
            collect_rule_refs(choice, rule_indices, queue);
        }
    }
}

fn collect_rule_deps(
    pattern: &[PatternItem],
    rule_indices: &BTreeMap<String, usize>,
    deps: &mut BTreeSet<String>,
) {
    for item in pattern {
        if let Some(node) = &item.node {
            if rule_indices.contains_key(node) {
                deps.insert(node.clone());
            }
        }
        if let Some(repeat) = &item.repeat {
            collect_rule_deps(repeat, rule_indices, deps);
        }
        if let Some(optional) = &item.optional {
            collect_rule_deps(optional, rule_indices, deps);
        }
        if let Some(choice) = &item.choice {
            collect_rule_deps(choice, rule_indices, deps);
        }
    }
}

fn collect_token_set_refs(pattern: &[PatternItem], used: &mut BTreeSet<String>) {
    for item in pattern {
        if let Some(name) = &item.token_set {
            used.insert(name.clone());
        }
        if let Some(repeat) = &item.repeat {
            collect_token_set_refs(repeat, used);
        }
        if let Some(optional) = &item.optional {
            collect_token_set_refs(optional, used);
        }
        if let Some(choice) = &item.choice {
            collect_token_set_refs(choice, used);
        }
    }
}

fn collect_prefix_node(item: &PatternItem, deps: &mut BTreeSet<String>) {
    if let Some(node) = &item.node {
        deps.insert(node.clone());
    }
    if let Some(choice) = &item.choice {
        for alt in choice {
            collect_prefix_node(alt, deps);
        }
    }
    if let Some(repeat) = &item.repeat {
        for item in repeat {
            collect_prefix_node(item, deps);
        }
    }
    if let Some(optional) = &item.optional {
        for item in optional {
            collect_prefix_node(item, deps);
        }
    }
}

fn expression_first(expression: &ExpressionSpec) -> BTreeSet<String> {
    let mut first = expression
        .prefix
        .iter()
        .map(|prefix| prefix.token.clone())
        .collect::<BTreeSet<_>>();
    first.extend(
        expression
            .atoms
            .iter()
            .filter_map(|atom| atom.token.as_ref().cloned()),
    );
    first
}

fn find_cycles(names: &[String], graph: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
    let mut cycles = Vec::new();
    for start in names {
        let mut stack = Vec::new();
        dfs_cycle(start, start, graph, &mut stack, &mut cycles);
    }
    cycles
}

fn dfs_cycle(
    start: &str,
    current: &str,
    graph: &BTreeMap<String, BTreeSet<String>>,
    stack: &mut Vec<String>,
    cycles: &mut Vec<Vec<String>>,
) {
    if stack.iter().any(|item| item == current) {
        return;
    }
    stack.push(current.to_string());
    if let Some(nexts) = graph.get(current) {
        for next in nexts {
            if next == start {
                let mut cycle = stack.clone();
                cycle.push(start.to_string());
                cycles.push(cycle);
                break;
            }
            dfs_cycle(start, next, graph, stack, cycles);
        }
    }
    stack.pop();
}

fn extend_set(target: &mut BTreeSet<String>, source: BTreeSet<String>) -> bool {
    let before = target.len();
    target.extend(source);
    target.len() != before
}

fn first_of_sequence(
    items: &[PatternItem],
    first: &[BTreeSet<String>],
    nullable_rules: &[bool],
    rule_indices: &BTreeMap<String, usize>,
    token_sets: &[BTreeSet<String>],
    token_set_indices: &BTreeMap<String, usize>,
    expression_indices: &BTreeMap<String, usize>,
    expression_first: &[BTreeSet<String>],
) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    for item in items {
        result.extend(first_of_item(
            item,
            first,
            nullable_rules,
            rule_indices,
            token_sets,
            token_set_indices,
            expression_indices,
            expression_first,
        ));
        if !item_nullable_with(
            item,
            nullable_rules,
            rule_indices,
            token_set_indices,
            expression_indices,
            expression_first,
        ) {
            break;
        }
    }
    result
}

fn first_of_item(
    item: &PatternItem,
    first: &[BTreeSet<String>],
    nullable_rules: &[bool],
    rule_indices: &BTreeMap<String, usize>,
    token_sets: &[BTreeSet<String>],
    token_set_indices: &BTreeMap<String, usize>,
    expression_indices: &BTreeMap<String, usize>,
    expression_first: &[BTreeSet<String>],
) -> BTreeSet<String> {
    if let Some(token) = &item.token {
        return BTreeSet::from([token.clone()]);
    }
    if let Some(node) = &item.node {
        return first[*rule_indices.get(node).expect("validated")].clone();
    }
    if let Some(repeat) = &item.repeat {
        return first_of_sequence(
            repeat,
            first,
            nullable_rules,
            rule_indices,
            token_sets,
            token_set_indices,
            expression_indices,
            expression_first,
        );
    }
    if let Some(optional) = &item.optional {
        return first_of_sequence(
            optional,
            first,
            nullable_rules,
            rule_indices,
            token_sets,
            token_set_indices,
            expression_indices,
            expression_first,
        );
    }
    if let Some(choice) = &item.choice {
        return choice
            .iter()
            .flat_map(|alt| {
                first_of_item(
                    alt,
                    first,
                    nullable_rules,
                    rule_indices,
                    token_sets,
                    token_set_indices,
                    expression_indices,
                    expression_first,
                )
            })
            .collect();
    }
    if let Some(token_set) = &item.token_set {
        return token_sets[*token_set_indices.get(token_set).expect("validated")].clone();
    }
    if let Some(expr) = &item.expr {
        return expression_first[*expression_indices.get(expr).expect("validated")].clone();
    }
    BTreeSet::new()
}

fn pattern_nullable(pattern: &[PatternItem]) -> bool {
    pattern
        .iter()
        .all(|item| item.repeat.is_some() || item.optional.is_some())
}

fn pattern_nullable_with(
    pattern: &[PatternItem],
    nullable_rules: &[bool],
    rule_indices: &BTreeMap<String, usize>,
    token_set_indices: &BTreeMap<String, usize>,
    expression_indices: &BTreeMap<String, usize>,
    expression_first: &[BTreeSet<String>],
) -> bool {
    pattern.iter().all(|item| {
        item_nullable_with(
            item,
            nullable_rules,
            rule_indices,
            token_set_indices,
            expression_indices,
            expression_first,
        )
    })
}

fn item_nullable_with(
    item: &PatternItem,
    nullable_rules: &[bool],
    rule_indices: &BTreeMap<String, usize>,
    token_set_indices: &BTreeMap<String, usize>,
    expression_indices: &BTreeMap<String, usize>,
    expression_first: &[BTreeSet<String>],
) -> bool {
    if let Some(repeat) = &item.repeat {
        let _ = repeat;
        return true;
    }
    if let Some(optional) = &item.optional {
        let _ = optional;
        return true;
    }
    if let Some(node) = &item.node {
        return nullable_rules[*rule_indices.get(node).expect("validated")];
    }
    if let Some(choice) = &item.choice {
        return choice.iter().any(|alt| {
            item_nullable_with(
                alt,
                nullable_rules,
                rule_indices,
                token_set_indices,
                expression_indices,
                expression_first,
            )
        });
    }
    let _ = expression_indices;
    let _ = expression_first;
    false
}

fn collect_follow(
    items: &[PatternItem],
    parent_follow: &BTreeSet<String>,
    follow: &mut [BTreeSet<String>],
    first: &[BTreeSet<String>],
    nullable_rules: &[bool],
    rule_indices: &BTreeMap<String, usize>,
    token_sets: &[BTreeSet<String>],
    token_set_indices: &BTreeMap<String, usize>,
    expression_indices: &BTreeMap<String, usize>,
    expression_first: &[BTreeSet<String>],
) -> bool {
    let mut changed = false;
    for (index, item) in items.iter().enumerate() {
        let rest = &items[index + 1..];
        let mut item_follow = first_of_sequence(
            rest,
            first,
            nullable_rules,
            rule_indices,
            token_sets,
            token_set_indices,
            expression_indices,
            expression_first,
        );
        if pattern_nullable_with(
            rest,
            nullable_rules,
            rule_indices,
            token_set_indices,
            expression_indices,
            expression_first,
        ) {
            item_follow.extend(parent_follow.iter().cloned());
        }
        changed |= collect_follow_for_item(
            item,
            item_follow,
            follow,
            first,
            nullable_rules,
            rule_indices,
            token_sets,
            token_set_indices,
            expression_indices,
            expression_first,
        );
    }
    changed
}

fn collect_follow_for_item(
    item: &PatternItem,
    item_follow: BTreeSet<String>,
    follow: &mut [BTreeSet<String>],
    first: &[BTreeSet<String>],
    nullable_rules: &[bool],
    rule_indices: &BTreeMap<String, usize>,
    token_sets: &[BTreeSet<String>],
    token_set_indices: &BTreeMap<String, usize>,
    expression_indices: &BTreeMap<String, usize>,
    expression_first: &[BTreeSet<String>],
) -> bool {
    let mut changed = false;
    if let Some(node) = &item.node {
        changed |= extend_set(
            &mut follow[*rule_indices.get(node).expect("validated")],
            item_follow,
        );
    } else if let Some(repeat) = &item.repeat {
        let mut repeat_follow = item_follow;
        repeat_follow.extend(first_of_sequence(
            repeat,
            first,
            nullable_rules,
            rule_indices,
            token_sets,
            token_set_indices,
            expression_indices,
            expression_first,
        ));
        changed |= collect_follow(
            repeat,
            &repeat_follow,
            follow,
            first,
            nullable_rules,
            rule_indices,
            token_sets,
            token_set_indices,
            expression_indices,
            expression_first,
        );
    } else if let Some(optional) = &item.optional {
        changed |= collect_follow(
            optional,
            &item_follow,
            follow,
            first,
            nullable_rules,
            rule_indices,
            token_sets,
            token_set_indices,
            expression_indices,
            expression_first,
        );
    } else if let Some(choice) = &item.choice {
        for alt in choice {
            changed |= collect_follow_for_item(
                alt,
                item_follow.clone(),
                follow,
                first,
                nullable_rules,
                rule_indices,
                token_sets,
                token_set_indices,
                expression_indices,
                expression_first,
            );
        }
    }
    let _ = token_sets;
    let _ = token_set_indices;
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(text: &str) -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("syntax.ron");
        fs::write(&path, text)?;
        let grammar = Grammar::load(&path)?;
        grammar.validate()
    }

    fn valid_grammar() -> String {
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
                    (name: "LetKw", literal: "let"),
                    (name: "Ident", regex: "[A-Za-z]+"),
                    (name: "Eq", literal: "="),
                    (name: "Int", regex: "[0-9]+"),
                    (name: "Plus", literal: "+"),
                    (name: "Semicolon", literal: ";"),
                ],
            ),
            syntax: (
                entry: "File",
                nodes: ["File", "Stmt", "Name"],
                rules: [
                    (name: "File", pattern: [(repeat: [(node: "Stmt")])]),
                    (name: "Stmt", pattern: [(token: "LetKw"), (node: "Name"), (token: "Eq"), (expr: "Expr"), (token: "Semicolon")]),
                    (name: "Name", pattern: [(token: "Ident")]),
                ],
                token_sets: [],
            ),
            expressions: (
                items: [
                    (
                        name: "Expr",
                        root_node: "Expr",
                        atoms: [(token: "Int"), (node: "Name")],
                        infix: [(token: "Plus", node: "BinaryExpr", precedence: "Add", associativity: "left")],
                    ),
                ],
            ),
            ast: (
                root: "File",
                nodes: [],
            ),
        )"#.to_string()
    }

    #[test]
    fn accepts_non_dawn_grammar() {
        validate(&valid_grammar()).unwrap();
    }

    #[test]
    fn rejects_unknown_fields() {
        let grammar = valid_grammar().replace(
            "language: (type_name: \"ToyLanguage\", syntax_kind: \"ToyKind\")",
            "language: (type_name: \"ToyLanguage\", syntax_kind: \"ToyKind\", legacy: true)",
        );
        assert!(validate(&grammar).is_err());
    }

    #[test]
    fn rejects_missing_diagnostic_binding() {
        let grammar = valid_grammar().replace("unexpected_eof: \"UnexpectedEof\",", "");
        assert!(validate(&grammar).is_err());
    }

    #[test]
    fn rejects_ambiguous_choices() {
        let grammar = valid_grammar().replace(
            "(name: \"Name\", pattern: [(token: \"Ident\")])",
            "(name: \"Name\", pattern: [(choice: [(token: \"Ident\"), (token: \"Ident\")])])",
        );
        assert!(validate(&grammar).is_err());
    }

    #[test]
    fn rejects_left_recursion() {
        let grammar = valid_grammar().replace(
            "(name: \"Name\", pattern: [(token: \"Ident\")])",
            "(name: \"Name\", pattern: [(node: \"Name\"), (token: \"Ident\")])",
        );
        assert!(validate(&grammar).is_err());
    }

    #[test]
    fn rejects_invalid_regex() {
        let grammar = valid_grammar().replace("[A-Za-z]+", "[A-");
        assert!(validate(&grammar).is_err());
    }

    #[test]
    fn rejects_invalid_expression_spec() {
        let grammar =
            valid_grammar().replace("associativity: \"left\"", "associativity: \"sideways\"");
        assert!(validate(&grammar).is_err());
    }

    #[test]
    fn rejects_duplicate_accessor_labels() {
        let grammar = valid_grammar().replace(
            "(name: \"Name\", pattern: [(token: \"Ident\")])",
            "(name: \"Name\", pattern: [(label: \"text\", token: \"Ident\"), (label: \"text\", token: \"Ident\")])",
        );
        let error = validate(&grammar).unwrap_err().to_string();
        assert!(error.contains("duplicate accessor label"));
    }

    #[test]
    fn rejects_accessor_label_collisions_with_generated_methods() {
        let grammar = valid_grammar().replace(
            "(name: \"Name\", pattern: [(token: \"Ident\")])",
            "(name: \"Name\", pattern: [(label: \"syntax\", token: \"Ident\")])",
        );
        let error = validate(&grammar).unwrap_err().to_string();
        assert!(error.contains("duplicate accessor label"));
    }

    #[test]
    fn rejects_duplicate_generated_enum_names() {
        let grammar = valid_grammar().replace(
            "(name: \"Name\", pattern: [(token: \"Ident\")])",
            "(name: \"Name\", pattern: [(enum: \"Dup\", choice: [(token: \"Ident\")]), (enum: \"Dup\", choice: [(token: \"LetKw\")])])",
        );
        let error = validate(&grammar).unwrap_err().to_string();
        assert!(error.contains("duplicate generated enum"));
    }

    #[test]
    fn rejects_ambiguous_variable_length_label_layouts() {
        let grammar = valid_grammar().replace(
            "(name: \"File\", pattern: [(repeat: [(node: \"Stmt\")])])",
            "(name: \"File\", pattern: [(repeat: [(node: \"Stmt\")]), (label: \"tail\", node: \"Stmt\")])",
        );
        let error = validate(&grammar).unwrap_err().to_string();
        assert!(error.contains("ambiguous after variable-length"));
    }
}
