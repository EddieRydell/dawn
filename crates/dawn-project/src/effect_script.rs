use std::collections::{BTreeMap, HashMap};
use std::fmt;

use crate::model::{Color, Curve, CurveValue, EffectParam, Flags};

#[derive(Debug, Clone)]
pub struct ScriptDiagnostic {
    pub range: Option<SourceRange>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRange {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourcePosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledEffect {
    pub name: String,
    pub params: Vec<EffectParamSchema>,
    sample: Vec<Stmt>,
}

impl CompiledEffect {
    pub fn param(&self, name: &str) -> Option<&EffectParamSchema> {
        self.params.iter().find(|param| param.name == name)
    }

    pub fn sample(
        &self,
        progress: f64,
        seconds: f64,
        fixture: FixtureContext,
        pixel: PixelContext,
        params: &BTreeMap<String, RuntimeValue>,
    ) -> Result<Color, RuntimeError> {
        Vm::new(self, progress, seconds, fixture, pixel, params).run()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixtureContext {
    pub index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelContext {
    pub index: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectParamSchema {
    pub name: String,
    pub value_type: ScriptType,
    pub options: Vec<String>,
    pub default: Option<ParamDefault>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParamDefault {
    Value(RuntimeValue),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptType {
    Float,
    Int,
    Bool,
    Color,
    CurveFloat,
    CurveColor,
    Enum,
    Flags,
    Fixture,
    Pixel,
    Void,
}

impl ScriptType {
    pub fn matches_param(self, param: &EffectParam<crate::model::Resolved>) -> bool {
        match (self, param) {
            (Self::Float, EffectParam::Float { .. }) => true,
            (Self::Int, EffectParam::Integer { .. }) => true,
            (Self::Bool, EffectParam::Boolean { .. }) => true,
            (Self::Color, EffectParam::Color { .. }) => true,
            (Self::Enum, EffectParam::Enum { .. }) => true,
            (Self::Flags, EffectParam::Flags { .. }) => true,
            (Self::CurveFloat, EffectParam::Curve { curve }) => {
                curve.value_type == crate::model::CurveValueType::Float
            }
            (Self::CurveColor, EffectParam::Curve { curve }) => {
                curve.value_type == crate::model::CurveValueType::Color
            }
            _ => false,
        }
    }
}

fn is_float_compatible(value_type: ScriptType) -> bool {
    matches!(value_type, ScriptType::Float | ScriptType::Int)
}

fn is_assignable(expected: ScriptType, actual: ScriptType) -> bool {
    expected == actual || (expected == ScriptType::Float && actual == ScriptType::Int)
}

fn binary_result_type(left: ScriptType, op: BinaryOp, right: ScriptType) -> Option<ScriptType> {
    match (left, op, right) {
        (ScriptType::Float, _, ScriptType::Float)
        | (ScriptType::Float, _, ScriptType::Int)
        | (ScriptType::Int, _, ScriptType::Float) => Some(ScriptType::Float),
        (ScriptType::Int, _, ScriptType::Int) => Some(ScriptType::Int),
        (ScriptType::Color, BinaryOp::Multiply, factor)
        | (factor, BinaryOp::Multiply, ScriptType::Color)
            if is_float_compatible(factor) =>
        {
            Some(ScriptType::Color)
        }
        _ => None,
    }
}

impl fmt::Display for ScriptType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Float => "float",
            Self::Int => "int",
            Self::Bool => "bool",
            Self::Color => "color",
            Self::CurveFloat => "curve<float>",
            Self::CurveColor => "curve<color>",
            Self::Enum => "enum",
            Self::Flags => "flags",
            Self::Fixture => "Fixture",
            Self::Pixel => "Pixel",
            Self::Void => "void",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeValue {
    Float(f64),
    Int(i64),
    Bool(bool),
    Color(Color),
    Curve(Curve),
    Enum(String),
    Flags(Flags),
    Fixture(FixtureContext),
    Pixel(PixelContext),
}

impl RuntimeValue {
    fn value_type(&self) -> ScriptType {
        match self {
            Self::Float(_) => ScriptType::Float,
            Self::Int(_) => ScriptType::Int,
            Self::Bool(_) => ScriptType::Bool,
            Self::Color(_) => ScriptType::Color,
            Self::Curve(curve) => match curve.value_type {
                crate::model::CurveValueType::Float => ScriptType::CurveFloat,
                crate::model::CurveValueType::Color => ScriptType::CurveColor,
            },
            Self::Enum(_) => ScriptType::Enum,
            Self::Flags(_) => ScriptType::Flags,
            Self::Fixture(_) => ScriptType::Fixture,
            Self::Pixel(_) => ScriptType::Pixel,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub message: String,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RuntimeError {}

pub fn compile(text: &str) -> Result<CompiledEffect, Vec<ScriptDiagnostic>> {
    let tokens = lex(text)?;
    let effect = parse(&tokens)?;
    type_check(&effect)?;
    Ok(compile_ast(effect))
}

pub fn compile_ast(effect: EffectAst) -> CompiledEffect {
    CompiledEffect {
        name: effect.name,
        params: effect.params,
        sample: effect.sample,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectAst {
    pub name: String,
    pub params: Vec<EffectParamSchema>,
    sample: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
enum Stmt {
    Let {
        name: String,
        value_type: ScriptType,
        expr: Expr,
    },
    Return(Expr),
}

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Float(f64),
    Int(i64),
    Bool(bool),
    Color(Color),
    Ident(String),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnaryOp {
    Negate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    kind: TokenKind,
    range: SourceRange,
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Ident(String),
    Number(String),
    Color(String),
    String(String),
    Symbol(char),
    Eof,
}

pub fn lex(text: &str) -> Result<Vec<Token>, Vec<ScriptDiagnostic>> {
    let mut lexer = Lexer::new(text);
    lexer.lex();
    if lexer.errors.is_empty() {
        Ok(lexer.tokens)
    } else {
        Err(lexer.errors)
    }
}

struct Lexer<'a> {
    chars: Vec<char>,
    index: usize,
    line: u32,
    character: u32,
    tokens: Vec<Token>,
    errors: Vec<ScriptDiagnostic>,
    _text: &'a str,
}

impl<'a> Lexer<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            chars: text.chars().collect(),
            index: 0,
            line: 0,
            character: 0,
            tokens: Vec::new(),
            errors: Vec::new(),
            _text: text,
        }
    }

    fn lex(&mut self) {
        while let Some(character) = self.peek() {
            if character.is_whitespace() {
                self.bump();
            } else if character.is_ascii_alphabetic() || character == '_' {
                self.ident();
            } else if character.is_ascii_digit() {
                self.number();
            } else if character == '#' {
                self.color();
            } else if character == '"' {
                self.string();
            } else if "{}();,<>+-*/=".contains(character) {
                let start = self.position();
                self.bump();
                self.tokens.push(Token {
                    kind: TokenKind::Symbol(character),
                    range: SourceRange {
                        start,
                        end: self.position(),
                    },
                });
            } else {
                let range = self.single_char_range();
                self.errors.push(ScriptDiagnostic {
                    range: Some(range),
                    message: format!("unexpected character `{character}`"),
                });
            }
        }
        let position = self.position();
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            range: SourceRange {
                start: position,
                end: position,
            },
        });
    }

    fn ident(&mut self) {
        let start = self.position();
        let mut value = String::new();
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            if let Some(character) = self.bump() {
                value.push(character);
            }
        }
        self.tokens.push(Token {
            kind: TokenKind::Ident(value),
            range: SourceRange {
                start,
                end: self.position(),
            },
        });
    }

    fn number(&mut self) {
        let start = self.position();
        let mut value = String::new();
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_digit())
        {
            if let Some(character) = self.bump() {
                value.push(character);
            }
        }
        if self.peek() == Some('.') {
            if let Some(character) = self.bump() {
                value.push(character);
            }
            while self
                .peek()
                .is_some_and(|character| character.is_ascii_digit())
            {
                if let Some(character) = self.bump() {
                    value.push(character);
                }
            }
        }
        self.tokens.push(Token {
            kind: TokenKind::Number(value),
            range: SourceRange {
                start,
                end: self.position(),
            },
        });
    }

    fn color(&mut self) {
        let start = self.position();
        let mut value = String::new();
        if let Some(character) = self.bump() {
            value.push(character);
        }
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_hexdigit())
        {
            if let Some(character) = self.bump() {
                value.push(character);
            }
        }
        self.tokens.push(Token {
            kind: TokenKind::Color(value),
            range: SourceRange {
                start,
                end: self.position(),
            },
        });
    }

    fn string(&mut self) {
        let start = self.position();
        self.bump();
        let mut value = String::new();
        while let Some(character) = self.peek() {
            if character == '"' {
                self.bump();
                self.tokens.push(Token {
                    kind: TokenKind::String(value),
                    range: SourceRange {
                        start,
                        end: self.position(),
                    },
                });
                return;
            }
            if let Some(character) = self.bump() {
                value.push(character);
            }
        }
        self.errors.push(ScriptDiagnostic {
            range: Some(SourceRange {
                start,
                end: self.position(),
            }),
            message: "unterminated string literal".to_string(),
        });
    }

    fn single_char_range(&mut self) -> SourceRange {
        let start = self.position();
        self.bump();
        SourceRange {
            start,
            end: self.position(),
        }
    }

    fn position(&self) -> SourcePosition {
        SourcePosition {
            line: self.line,
            character: self.character,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.chars.get(self.index).copied()?;
        self.index += 1;
        if character == '\n' {
            self.line += 1;
            self.character = 0;
        } else {
            self.character += 1;
        }
        Some(character)
    }
}

pub fn parse(tokens: &[Token]) -> Result<EffectAst, Vec<ScriptDiagnostic>> {
    if tokens.is_empty() {
        return Err(vec![ScriptDiagnostic {
            range: None,
            message: "script did not produce tokens".to_string(),
        }]);
    }
    let mut parser = Parser {
        tokens,
        index: 0,
        errors: Vec::new(),
    };
    let parsed = parser.effect();
    if parser.errors.is_empty() {
        parsed.map_err(|diagnostic| vec![diagnostic])
    } else {
        Err(parser.errors)
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    index: usize,
    errors: Vec<ScriptDiagnostic>,
}

impl Parser<'_> {
    fn effect(&mut self) -> Result<EffectAst, ScriptDiagnostic> {
        self.keyword("effect")?;
        let name = self.identifier("effect name")?;
        self.symbol('{')?;
        let mut params = Vec::new();
        let mut sample = None;
        while !self.at_symbol('}') && !self.at_eof() {
            if self.at_keyword("param") {
                params.push(self.param()?);
            } else {
                sample = Some(self.sample()?);
            }
        }
        self.symbol('}')?;
        if !self.at_eof() {
            return Err(self.error_here("expected exactly one effect declaration per file"));
        }
        Ok(EffectAst {
            name,
            params,
            sample: sample.ok_or_else(|| self.error_here("missing sample entrypoint"))?,
        })
    }

    fn param(&mut self) -> Result<EffectParamSchema, ScriptDiagnostic> {
        self.keyword("param")?;
        let value_type = self.type_name()?;
        let name = self.identifier("parameter name")?;
        let options = if matches!(value_type, ScriptType::Enum | ScriptType::Flags) {
            self.option_list()?
        } else {
            Vec::new()
        };
        let default = if self.consume_symbol('=') {
            if self.at_keyword("import") {
                return Err(self.error_here("effect parameter defaults cannot import files"));
            } else {
                Some(ParamDefault::Value(
                    self.param_default_value(value_type, &options)?,
                ))
            }
        } else {
            None
        };
        if matches!(value_type, ScriptType::Enum | ScriptType::Flags) && options.is_empty() {
            return Err(self.error_here("enum and flags parameters must declare options"));
        }
        self.symbol(';')?;
        Ok(EffectParamSchema {
            name,
            value_type,
            options,
            default,
        })
    }

    fn option_list(&mut self) -> Result<Vec<String>, ScriptDiagnostic> {
        self.symbol('{')?;
        let mut options = Vec::new();
        while !self.at_symbol('}') && !self.at_eof() {
            let option = self.identifier("option")?;
            if options.contains(&option) {
                return Err(self.error_here(&format!("duplicate option `{option}`")));
            }
            options.push(option);
            if !self.consume_symbol(',') {
                break;
            }
        }
        self.symbol('}')?;
        Ok(options)
    }

    fn sample(&mut self) -> Result<Vec<Stmt>, ScriptDiagnostic> {
        let return_type = self.type_name()?;
        if return_type != ScriptType::Color {
            return Err(self.error_here("sample must return color"));
        }
        let name = self.identifier("function name")?;
        if name != "sample" {
            return Err(self.error_here("only sample entrypoint functions are supported"));
        }
        self.symbol('(')?;
        self.expect_arg("float", "progress")?;
        self.symbol(',')?;
        self.expect_arg("float", "seconds")?;
        self.symbol(',')?;
        self.expect_arg("Fixture", "fixture")?;
        self.symbol(',')?;
        self.expect_arg("Pixel", "pixel")?;
        self.symbol(')')?;
        self.symbol('{')?;
        let mut statements = Vec::new();
        while !self.at_symbol('}') && !self.at_eof() {
            if self.consume_keyword("return") {
                let expr = self.expr()?;
                self.symbol(';')?;
                statements.push(Stmt::Return(expr));
            } else {
                let value_type = self.type_name()?;
                let name = self.identifier("local name")?;
                self.symbol('=')?;
                let expr = self.expr()?;
                self.symbol(';')?;
                statements.push(Stmt::Let {
                    name,
                    value_type,
                    expr,
                });
            }
        }
        self.symbol('}')?;
        Ok(statements)
    }

    fn expr(&mut self) -> Result<Expr, ScriptDiagnostic> {
        self.additive()
    }

    fn additive(&mut self) -> Result<Expr, ScriptDiagnostic> {
        let mut expr = self.multiplicative()?;
        loop {
            let op = if self.consume_symbol('+') {
                BinaryOp::Add
            } else if self.consume_symbol('-') {
                BinaryOp::Subtract
            } else {
                break;
            };
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(self.multiplicative()?),
            };
        }
        Ok(expr)
    }

    fn multiplicative(&mut self) -> Result<Expr, ScriptDiagnostic> {
        let mut expr = self.unary()?;
        loop {
            let op = if self.consume_symbol('*') {
                BinaryOp::Multiply
            } else if self.consume_symbol('/') {
                BinaryOp::Divide
            } else {
                break;
            };
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(self.unary()?),
            };
        }
        Ok(expr)
    }

    fn unary(&mut self) -> Result<Expr, ScriptDiagnostic> {
        if self.consume_symbol('-') {
            Ok(Expr::Unary {
                op: UnaryOp::Negate,
                expr: Box::new(self.unary()?),
            })
        } else {
            self.primary()
        }
    }

    fn primary(&mut self) -> Result<Expr, ScriptDiagnostic> {
        let token = self.advance().clone();
        match &token.kind {
            TokenKind::Number(raw) => {
                if raw.contains('.') {
                    raw.parse::<f64>()
                        .map(Expr::Float)
                        .map_err(|_| self.error_at(&token, "invalid float literal"))
                } else {
                    raw.parse::<i64>()
                        .map(Expr::Int)
                        .map_err(|_| self.error_at(&token, "invalid integer literal"))
                }
            }
            TokenKind::Color(raw) => Color::parse(raw)
                .map(Expr::Color)
                .map_err(|message| self.error_at(&token, &message)),
            TokenKind::Ident(name) if name == "true" => Ok(Expr::Bool(true)),
            TokenKind::Ident(name) if name == "false" => Ok(Expr::Bool(false)),
            TokenKind::Ident(name) => {
                if self.consume_symbol('(') {
                    let mut args = Vec::new();
                    if !self.at_symbol(')') {
                        loop {
                            args.push(self.expr()?);
                            if !self.consume_symbol(',') {
                                break;
                            }
                        }
                    }
                    self.symbol(')')?;
                    Ok(Expr::Call {
                        name: name.clone(),
                        args,
                    })
                } else {
                    Ok(Expr::Ident(name.clone()))
                }
            }
            TokenKind::Symbol('(') => {
                let expr = self.expr()?;
                self.symbol(')')?;
                Ok(expr)
            }
            _ => Err(self.error_at(&token, "expected expression")),
        }
    }

    fn param_default_value(
        &mut self,
        value_type: ScriptType,
        options: &[String],
    ) -> Result<RuntimeValue, ScriptDiagnostic> {
        if value_type == ScriptType::Enum {
            let value = self.identifier("enum default")?;
            if !options.contains(&value) {
                return Err(self.error_here(&format!(
                    "enum default `{value}` is not declared in the option list"
                )));
            }
            return Ok(RuntimeValue::Enum(value));
        }
        if value_type == ScriptType::Flags {
            let values = self.flags_default_value()?;
            for value in &values {
                if !options.contains(value) {
                    return Err(self.error_here(&format!(
                        "flags default `{value}` is not declared in the option list"
                    )));
                }
            }
            return Ok(RuntimeValue::Flags(Flags { values }));
        }
        let expr = self.expr()?;
        let value = Vm::eval_constant(&expr)?;
        if is_assignable(value_type, value.value_type()) {
            Vm::coerce_value(value, value_type).map_err(|error| ScriptDiagnostic {
                range: None,
                message: error.message,
            })
        } else {
            Err(self.error_here(&format!(
                "default value must be {value_type}, but found {}",
                value.value_type()
            )))
        }
    }

    fn flags_default_value(&mut self) -> Result<Vec<String>, ScriptDiagnostic> {
        self.symbol('{')?;
        let mut values = Vec::new();
        while !self.at_symbol('}') && !self.at_eof() {
            let value = self.identifier("flag default")?;
            if values.contains(&value) {
                return Err(self.error_here(&format!("duplicate flag default `{value}`")));
            }
            values.push(value);
            if !self.consume_symbol(',') {
                break;
            }
        }
        self.symbol('}')?;
        Ok(values)
    }

    fn expect_arg(&mut self, type_name: &str, name: &str) -> Result<(), ScriptDiagnostic> {
        let actual_type = self.type_name()?;
        let expected_type = match type_name {
            "float" => ScriptType::Float,
            "Fixture" => ScriptType::Fixture,
            "Pixel" => ScriptType::Pixel,
            _ => unreachable!("fixed parser type"),
        };
        if actual_type != expected_type {
            return Err(self.error_here(&format!("expected `{type_name} {name}`")));
        }
        let actual_name = self.identifier("argument name")?;
        if actual_name != name {
            return Err(self.error_here(&format!("expected argument `{name}`")));
        }
        Ok(())
    }

    fn type_name(&mut self) -> Result<ScriptType, ScriptDiagnostic> {
        let name = self.identifier("type")?;
        match name.as_str() {
            "float" => Ok(ScriptType::Float),
            "int" => Ok(ScriptType::Int),
            "bool" => Ok(ScriptType::Bool),
            "color" => Ok(ScriptType::Color),
            "enum" => Ok(ScriptType::Enum),
            "flags" => Ok(ScriptType::Flags),
            "Fixture" => Ok(ScriptType::Fixture),
            "Pixel" => Ok(ScriptType::Pixel),
            "curve" => {
                self.symbol('<')?;
                let inner = self.identifier("curve value type")?;
                self.symbol('>')?;
                match inner.as_str() {
                    "float" => Ok(ScriptType::CurveFloat),
                    "color" => Ok(ScriptType::CurveColor),
                    _ => Err(self.error_here("curve value type must be float or color")),
                }
            }
            _ => Err(self.error_here(&format!("unknown type `{name}`"))),
        }
    }

    fn identifier(&mut self, label: &str) -> Result<String, ScriptDiagnostic> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Ident(value) => Ok(value),
            _ => Err(self.error_at(&token, &format!("expected {label}"))),
        }
    }

    fn keyword(&mut self, keyword: &str) -> Result<(), ScriptDiagnostic> {
        if self.consume_keyword(keyword) {
            Ok(())
        } else {
            Err(self.error_here(&format!("expected `{keyword}`")))
        }
    }

    fn symbol(&mut self, symbol: char) -> Result<(), ScriptDiagnostic> {
        if self.consume_symbol(symbol) {
            Ok(())
        } else {
            Err(self.error_here(&format!("expected `{symbol}`")))
        }
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        if self.at_keyword(keyword) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn at_keyword(&self, keyword: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Ident(value) if value == keyword)
    }

    fn consume_symbol(&mut self, symbol: char) -> bool {
        if self.at_symbol(symbol) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn at_symbol(&self, symbol: char) -> bool {
        matches!(self.peek().kind, TokenKind::Symbol(value) if value == symbol)
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn advance(&mut self) -> Token {
        let token = self.peek().clone();
        if !matches!(token.kind, TokenKind::Eof) {
            self.index += 1;
        }
        token
    }

    fn peek(&self) -> &Token {
        if let Some(token) = self.tokens.get(self.index) {
            token
        } else {
            &self.tokens[self.tokens.len() - 1]
        }
    }

    fn error_here(&self, message: &str) -> ScriptDiagnostic {
        self.error_at(self.peek(), message)
    }

    fn error_at(&self, token: &Token, message: &str) -> ScriptDiagnostic {
        ScriptDiagnostic {
            range: Some(token.range),
            message: message.to_string(),
        }
    }
}

pub fn type_check(effect: &EffectAst) -> Result<(), Vec<ScriptDiagnostic>> {
    let mut checker = TypeChecker::new(effect);
    checker.check();
    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}

struct TypeChecker<'a> {
    effect: &'a EffectAst,
    scopes: HashMap<String, ScriptType>,
    errors: Vec<ScriptDiagnostic>,
}

impl<'a> TypeChecker<'a> {
    fn new(effect: &'a EffectAst) -> Self {
        let mut scopes = HashMap::from([
            ("progress".to_string(), ScriptType::Float),
            ("seconds".to_string(), ScriptType::Float),
            ("fixture".to_string(), ScriptType::Fixture),
            ("pixel".to_string(), ScriptType::Pixel),
            ("PI".to_string(), ScriptType::Float),
            ("TAU".to_string(), ScriptType::Float),
        ]);
        for param in &effect.params {
            scopes.insert(param.name.clone(), param.value_type);
        }
        Self {
            effect,
            scopes,
            errors: Vec::new(),
        }
    }

    fn check(&mut self) {
        let mut saw_return = false;
        for statement in &self.effect.sample {
            match statement {
                Stmt::Let {
                    name,
                    value_type,
                    expr,
                } => {
                    let actual = self.expr_type(expr);
                    if !is_assignable(*value_type, actual) {
                        self.errors.push(ScriptDiagnostic {
                            range: None,
                            message: format!(
                                "local `{name}` is declared as {value_type}, but expression is {actual}"
                            ),
                        });
                    }
                    self.scopes.insert(name.clone(), *value_type);
                }
                Stmt::Return(expr) => {
                    saw_return = true;
                    let actual = self.expr_type(expr);
                    if actual != ScriptType::Color {
                        self.errors.push(ScriptDiagnostic {
                            range: None,
                            message: format!("sample must return color, but returned {actual}"),
                        });
                    }
                }
            }
        }
        if !saw_return {
            self.errors.push(ScriptDiagnostic {
                range: None,
                message: "sample must contain an explicit return".to_string(),
            });
        }
    }

    fn expr_type(&mut self, expr: &Expr) -> ScriptType {
        match expr {
            Expr::Float(_) => ScriptType::Float,
            Expr::Int(_) => ScriptType::Int,
            Expr::Bool(_) => ScriptType::Bool,
            Expr::Color(_) => ScriptType::Color,
            Expr::Ident(name) => self.scopes.get(name).copied().unwrap_or_else(|| {
                self.errors.push(ScriptDiagnostic {
                    range: None,
                    message: format!("unknown identifier `{name}`"),
                });
                ScriptType::Void
            }),
            Expr::Unary { expr, .. } => self.expr_type(expr),
            Expr::Binary { left, op, right } => {
                let left = self.expr_type(left);
                let right = self.expr_type(right);
                match binary_result_type(left, *op, right) {
                    Some(value_type) => value_type,
                    None => {
                        self.errors.push(ScriptDiagnostic {
                            range: None,
                            message: format!("cannot apply binary operator to {left} and {right}"),
                        });
                        ScriptType::Void
                    }
                }
            }
            Expr::Call { name, args } => self.call_type(name, args),
        }
    }

    fn call_type(&mut self, name: &str, args: &[Expr]) -> ScriptType {
        if let Some(param_type) = self.scopes.get(name).copied() {
            let [arg] = args else {
                self.errors.push(ScriptDiagnostic {
                    range: None,
                    message: format!("curve parameter `{name}` expects one argument"),
                });
                return ScriptType::Void;
            };
            return match param_type {
                ScriptType::CurveFloat | ScriptType::CurveColor => {
                    let arg_type = self.expr_type(arg);
                    if !is_float_compatible(arg_type) {
                        self.errors.push(ScriptDiagnostic {
                            range: None,
                            message: format!("curve parameter `{name}` expects a float argument"),
                        });
                    }
                    match param_type {
                        ScriptType::CurveFloat => ScriptType::Float,
                        ScriptType::CurveColor => ScriptType::Color,
                        _ => unreachable!(),
                    }
                }
                _ => {
                    self.errors.push(ScriptDiagnostic {
                        range: None,
                        message: format!("`{name}` is not callable"),
                    });
                    ScriptType::Void
                }
            };
        }

        let arg_types = args
            .iter()
            .map(|arg| self.expr_type(arg))
            .collect::<Vec<_>>();
        match (name, arg_types.as_slice()) {
            ("sin" | "cos" | "abs", [value]) if is_float_compatible(*value) => ScriptType::Float,
            ("min" | "max", [left, right])
                if is_float_compatible(*left) && is_float_compatible(*right) =>
            {
                ScriptType::Float
            }
            ("clamp" | "smoothstep", [first, second, third]) | ("mix", [first, second, third])
                if is_float_compatible(*first)
                    && is_float_compatible(*second)
                    && is_float_compatible(*third) =>
            {
                ScriptType::Float
            }
            ("rgb" | "hsv", [first, second, third])
                if is_float_compatible(*first)
                    && is_float_compatible(*second)
                    && is_float_compatible(*third) =>
            {
                ScriptType::Color
            }
            ("mix", [ScriptType::Color, ScriptType::Color, amount])
                if is_float_compatible(*amount) =>
            {
                ScriptType::Color
            }
            _ => {
                self.errors.push(ScriptDiagnostic {
                    range: None,
                    message: format!("unknown function or invalid call `{name}`"),
                });
                ScriptType::Void
            }
        }
    }
}

struct Vm<'a> {
    effect: &'a CompiledEffect,
    env: HashMap<String, RuntimeValue>,
}

impl<'a> Vm<'a> {
    fn new(
        effect: &'a CompiledEffect,
        progress: f64,
        seconds: f64,
        fixture: FixtureContext,
        pixel: PixelContext,
        params: &BTreeMap<String, RuntimeValue>,
    ) -> Self {
        let mut env = HashMap::from([
            ("progress".to_string(), RuntimeValue::Float(progress)),
            ("seconds".to_string(), RuntimeValue::Float(seconds)),
            ("fixture".to_string(), RuntimeValue::Fixture(fixture)),
            ("pixel".to_string(), RuntimeValue::Pixel(pixel)),
            ("PI".to_string(), RuntimeValue::Float(std::f64::consts::PI)),
            (
                "TAU".to_string(),
                RuntimeValue::Float(std::f64::consts::TAU),
            ),
        ]);
        for param in &effect.params {
            if let Some(value) = params.get(&param.name) {
                env.insert(param.name.clone(), value.clone());
            } else if let Some(ParamDefault::Value(value)) = &param.default {
                env.insert(param.name.clone(), value.clone());
            }
        }
        Self { effect, env }
    }

    fn run(&mut self) -> Result<Color, RuntimeError> {
        for statement in &self.effect.sample {
            match statement {
                Stmt::Let {
                    name,
                    value_type,
                    expr,
                } => {
                    let value = self.eval(expr)?;
                    self.env
                        .insert(name.clone(), Self::coerce_value(value, *value_type)?);
                }
                Stmt::Return(expr) => {
                    let RuntimeValue::Color(color) = self.eval(expr)? else {
                        return Err(self.error("sample returned a non-color value"));
                    };
                    return Ok(color);
                }
            }
        }
        Err(self.error("sample did not return"))
    }

    fn eval_constant(expr: &Expr) -> Result<RuntimeValue, ScriptDiagnostic> {
        match expr {
            Expr::Float(value) => Ok(RuntimeValue::Float(*value)),
            Expr::Int(value) => Ok(RuntimeValue::Int(*value)),
            Expr::Bool(value) => Ok(RuntimeValue::Bool(*value)),
            Expr::Color(value) => Ok(RuntimeValue::Color(*value)),
            _ => Err(ScriptDiagnostic {
                range: None,
                message: "parameter defaults must be literals in Dawn v1".to_string(),
            }),
        }
    }

    fn eval(&mut self, expr: &Expr) -> Result<RuntimeValue, RuntimeError> {
        match expr {
            Expr::Float(value) => Ok(RuntimeValue::Float(*value)),
            Expr::Int(value) => Ok(RuntimeValue::Int(*value)),
            Expr::Bool(value) => Ok(RuntimeValue::Bool(*value)),
            Expr::Color(value) => Ok(RuntimeValue::Color(*value)),
            Expr::Ident(name) => self
                .env
                .get(name)
                .cloned()
                .ok_or_else(|| self.error(&format!("unknown identifier `{name}`"))),
            Expr::Unary { op, expr } => match (op, self.eval(expr)?) {
                (UnaryOp::Negate, RuntimeValue::Float(value)) => Ok(RuntimeValue::Float(-value)),
                (UnaryOp::Negate, RuntimeValue::Int(value)) => value
                    .checked_neg()
                    .map(RuntimeValue::Int)
                    .ok_or_else(|| self.error("integer overflow")),
                _ => Err(self.error("invalid unary expression")),
            },
            Expr::Binary { left, op, right } => self.eval_binary(left, *op, right),
            Expr::Call { name, args } => self.eval_call(name, args),
        }
    }

    fn eval_binary(
        &mut self,
        left: &Expr,
        op: BinaryOp,
        right: &Expr,
    ) -> Result<RuntimeValue, RuntimeError> {
        let left = self.eval(left)?;
        let right = self.eval(right)?;
        match (left, op, right) {
            (RuntimeValue::Float(left), BinaryOp::Add, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left + right))
            }
            (RuntimeValue::Float(left), BinaryOp::Add, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Float(left + right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Add, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left as f64 + right))
            }
            (RuntimeValue::Int(left), BinaryOp::Add, RuntimeValue::Int(right)) => left
                .checked_add(right)
                .map(RuntimeValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            (RuntimeValue::Float(left), BinaryOp::Subtract, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left - right))
            }
            (RuntimeValue::Float(left), BinaryOp::Subtract, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Float(left - right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Subtract, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left as f64 - right))
            }
            (RuntimeValue::Int(left), BinaryOp::Subtract, RuntimeValue::Int(right)) => left
                .checked_sub(right)
                .map(RuntimeValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            (RuntimeValue::Float(left), BinaryOp::Multiply, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left * right))
            }
            (RuntimeValue::Float(left), BinaryOp::Multiply, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Float(left * right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Multiply, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left as f64 * right))
            }
            (RuntimeValue::Int(left), BinaryOp::Multiply, RuntimeValue::Int(right)) => left
                .checked_mul(right)
                .map(RuntimeValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            (RuntimeValue::Float(left), BinaryOp::Divide, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left / right))
            }
            (RuntimeValue::Float(left), BinaryOp::Divide, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Float(left / right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Divide, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left as f64 / right))
            }
            (RuntimeValue::Int(_), BinaryOp::Divide, RuntimeValue::Int(0)) => {
                Err(self.error("integer divide by zero"))
            }
            (RuntimeValue::Int(left), BinaryOp::Divide, RuntimeValue::Int(right)) => left
                .checked_div(right)
                .map(RuntimeValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            (RuntimeValue::Color(color), BinaryOp::Multiply, RuntimeValue::Float(factor))
            | (RuntimeValue::Float(factor), BinaryOp::Multiply, RuntimeValue::Color(color)) => {
                Ok(RuntimeValue::Color(color.scale(factor)))
            }
            (RuntimeValue::Color(color), BinaryOp::Multiply, RuntimeValue::Int(factor))
            | (RuntimeValue::Int(factor), BinaryOp::Multiply, RuntimeValue::Color(color)) => {
                Ok(RuntimeValue::Color(color.scale(factor as f64)))
            }
            _ => Err(self.error("invalid binary expression")),
        }
    }

    fn eval_call(&mut self, name: &str, args: &[Expr]) -> Result<RuntimeValue, RuntimeError> {
        if let Some(RuntimeValue::Curve(curve)) = self.env.get(name).cloned() {
            let arg = self.eval(&args[0])?;
            let amount = self.expect_float(arg)?;
            return match curve.evaluate(amount) {
                Some(CurveValue::Float(value)) => Ok(RuntimeValue::Float(value)),
                Some(CurveValue::Color(value)) => Ok(RuntimeValue::Color(value)),
                None => Err(self.error("curve has no points")),
            };
        }

        let values = args
            .iter()
            .map(|arg| self.eval(arg))
            .collect::<Result<Vec<_>, _>>()?;
        match (name, values.as_slice()) {
            ("sin", [value]) => Ok(RuntimeValue::Float(self.expect_float(value.clone())?.sin())),
            ("cos", [value]) => Ok(RuntimeValue::Float(self.expect_float(value.clone())?.cos())),
            ("abs", [value]) => Ok(RuntimeValue::Float(self.expect_float(value.clone())?.abs())),
            ("min", [left, right]) => Ok(RuntimeValue::Float(
                self.expect_float(left.clone())?
                    .min(self.expect_float(right.clone())?),
            )),
            ("max", [left, right]) => Ok(RuntimeValue::Float(
                self.expect_float(left.clone())?
                    .max(self.expect_float(right.clone())?),
            )),
            ("clamp", [value, min, max]) => Ok(RuntimeValue::Float(
                self.expect_float(value.clone())?.clamp(
                    self.expect_float(min.clone())?,
                    self.expect_float(max.clone())?,
                ),
            )),
            ("smoothstep", [edge0, edge1, value]) => {
                let edge0 = self.expect_float(edge0.clone())?;
                let edge1 = self.expect_float(edge1.clone())?;
                let value = self.expect_float(value.clone())?;
                let x = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
                Ok(RuntimeValue::Float(x * x * (3.0 - 2.0 * x)))
            }
            ("mix", [RuntimeValue::Color(left), RuntimeValue::Color(right), amount]) => Ok(
                RuntimeValue::Color(left.mix(*right, self.expect_float(amount.clone())?)),
            ),
            ("mix", [left, right, amount]) => {
                let left = self.expect_float(left.clone())?;
                let right = self.expect_float(right.clone())?;
                let amount = self.expect_float(amount.clone())?;
                Ok(RuntimeValue::Float(left + (right - left) * amount))
            }
            ("rgb", [red, green, blue]) => Ok(RuntimeValue::Color(Color::new(
                self.expect_float(red.clone())?.round().clamp(0.0, 255.0) as u8,
                self.expect_float(green.clone())?.round().clamp(0.0, 255.0) as u8,
                self.expect_float(blue.clone())?.round().clamp(0.0, 255.0) as u8,
            ))),
            ("hsv", [hue, saturation, value]) => Ok(RuntimeValue::Color(hsv_to_rgb(
                self.expect_float(hue.clone())?,
                self.expect_float(saturation.clone())?,
                self.expect_float(value.clone())?,
            ))),
            _ => Err(self.error(&format!("invalid call `{name}`"))),
        }
    }

    fn expect_float(&self, value: RuntimeValue) -> Result<f64, RuntimeError> {
        match value {
            RuntimeValue::Float(value) => Ok(value),
            RuntimeValue::Int(value) => Ok(value as f64),
            _ => Err(self.error("expected float value")),
        }
    }

    fn coerce_value(
        value: RuntimeValue,
        expected: ScriptType,
    ) -> Result<RuntimeValue, RuntimeError> {
        match (expected, value) {
            (ScriptType::Float, RuntimeValue::Int(value)) => Ok(RuntimeValue::Float(value as f64)),
            (expected, value) if value.value_type() == expected => Ok(value),
            (expected, value) => Err(RuntimeError {
                message: format!(
                    "expected {expected} value, but found {}",
                    value.value_type()
                ),
            }),
        }
    }

    fn error(&self, message: &str) -> RuntimeError {
        RuntimeError {
            message: message.to_string(),
        }
    }
}

fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> Color {
    let hue = hue.rem_euclid(360.0) / 60.0;
    let c = value.clamp(0.0, 1.0) * saturation.clamp(0.0, 1.0);
    let x = c * (1.0 - ((hue % 2.0) - 1.0).abs());
    let m = value.clamp(0.0, 1.0) - c;
    let (red, green, blue) = if hue < 1.0 {
        (c, x, 0.0)
    } else if hue < 2.0 {
        (x, c, 0.0)
    } else if hue < 3.0 {
        (0.0, c, x)
    } else if hue < 4.0 {
        (0.0, x, c)
    } else if hue < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    Color::new(
        ((red + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((green + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((blue + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CurvePoint, CurveValueType};

    fn fixture() -> FixtureContext {
        FixtureContext { index: 0 }
    }

    fn pixel() -> PixelContext {
        PixelContext { index: 0 }
    }

    fn empty_params() -> BTreeMap<String, RuntimeValue> {
        BTreeMap::new()
    }

    fn fade_curve() -> RuntimeValue {
        RuntimeValue::Curve(Curve {
            value_type: CurveValueType::Float,
            points: vec![
                CurvePoint {
                    time: 0.0,
                    value: CurveValue::Float(0.0),
                },
                CurvePoint {
                    time: 1.0,
                    value: CurveValue::Float(1.0),
                },
            ],
        })
    }

    fn sample(script: &CompiledEffect) -> Result<Color, RuntimeError> {
        script.sample(0.25, 0.0, fixture(), pixel(), &empty_params())
    }

    #[test]
    fn int_literals_promote_in_float_binary_contexts() {
        for expr in [
            "progress * speed * 9",
            "9 * progress * speed",
            "progress * 9.0",
        ] {
            let script = compile(&format!(
                r##"
effect Pulse {{
  param float speed = 0.75;

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {{
    float phase = (sin({expr}) + 1.0) / 2.0;
    return rgb(phase * 255.0, 0, 0);
  }}
}}
"##
            ))
            .unwrap();

            sample(&script).unwrap();
        }
    }

    #[test]
    fn int_literals_promote_in_float_call_contexts() {
        let script = compile(
            r##"
effect Calls {
  param color base = #000000;
  param color accent = #ffffff;
  param curve<float> fade;

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    float wave = sin(9);
    color rgb_color = rgb(255, 0, 0);
    color mixed = mix(base, accent, 1);
    float faded = fade(1);
    return mix(rgb_color, mixed, abs(wave) * 0.0 + faded);
  }
}
"##,
        )
        .unwrap();
        let mut params = BTreeMap::new();
        params.insert("fade".to_string(), fade_curve());

        let color = script
            .sample(0.0, 0.0, fixture(), pixel(), &params)
            .unwrap();
        assert_eq!(color, Color::new(255, 255, 255));
    }

    #[test]
    fn int_can_initialize_float_local() {
        let script = compile(
            r##"
effect Local {
  param float defaulted = 1;

  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    float x = 1;
    return rgb(x + defaulted, x + defaulted, x + defaulted);
  }
}
"##,
        )
        .unwrap();

        assert_eq!(sample(&script).unwrap(), Color::new(2, 2, 2));
    }

    #[test]
    fn int_division_truncates_toward_zero() {
        let script = compile(
            r##"
effect Divide {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    int x = 5 / 2;
    return rgb(x, x, x);
  }
}
"##,
        )
        .unwrap();

        assert_eq!(sample(&script).unwrap(), Color::new(2, 2, 2));
    }

    #[test]
    fn float_cannot_initialize_int_local() {
        let errors = compile(
            r##"
effect Bad {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    int x = 1.5;
    return rgb(x, x, x);
  }
}
"##,
        )
        .unwrap_err();

        assert!(errors
            .iter()
            .any(|error| error.message.contains("declared as int")));
    }

    #[test]
    fn int_divide_by_zero_returns_runtime_error() {
        let script = compile(
            r##"
effect Divide {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    int x = 1 / 0;
    return rgb(x, x, x);
  }
}
"##,
        )
        .unwrap();

        let error = sample(&script).unwrap_err();
        assert!(error.message.contains("divide by zero"));
    }

    #[test]
    fn int_factor_scales_color() {
        let script = compile(
            r##"
effect Scale {
  color sample(float progress, float seconds, Fixture fixture, Pixel pixel) {
    color left = #010203 * 2;
    color right = 2 * #010203;
    return mix(left, right, 0.5);
  }
}
"##,
        )
        .unwrap();

        assert_eq!(sample(&script).unwrap(), Color::new(2, 4, 6));
    }
}
