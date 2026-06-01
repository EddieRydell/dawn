use crate::model::{Color, Flags};

use super::ast::{
    BinaryOp, EffectAst, EffectEntrypoint, EffectImport, EffectModuleAst, EffectVisibility,
    EmitEffectRef, EmitParam, EmitStmt, Expr, Stmt, UnaryOp,
};
use super::lexer::{Token, TokenKind};
use super::params;
use super::{
    is_assignable, EffectParamSchema, ParamDefault, RuntimeValue, ScriptDiagnostic, ScriptType,
};
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

pub fn parse_module(tokens: &[Token]) -> Result<EffectModuleAst, Vec<ScriptDiagnostic>> {
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
    let parsed = parser.module();
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
    fn module(&mut self) -> Result<EffectModuleAst, ScriptDiagnostic> {
        let mut imports = Vec::new();
        while self.at_keyword("use") {
            imports.push(self.import()?);
        }
        let mut names = Vec::new();
        let mut effects = Vec::new();
        while !self.at_eof() {
            let effect = self.effect_with_imports(imports.clone())?;
            if names.contains(&effect.name) {
                return Err(
                    self.error_here(&format!("duplicate effect `{}` in module", effect.name))
                );
            }
            names.push(effect.name.clone());
            effects.push(effect);
        }
        if effects.is_empty() {
            return Err(self.error_here("missing effect declaration"));
        }
        Ok(EffectModuleAst { imports, effects })
    }

    fn effect(&mut self) -> Result<EffectAst, ScriptDiagnostic> {
        let mut imports = Vec::new();
        while self.at_keyword("use") {
            imports.push(self.import()?);
        }
        let effect = self.effect_with_imports(imports)?;
        if !self.at_eof() {
            return Err(self.error_here("expected exactly one effect declaration per file"));
        }
        Ok(effect)
    }

    fn effect_with_imports(
        &mut self,
        imports: Vec<EffectImport>,
    ) -> Result<EffectAst, ScriptDiagnostic> {
        let visibility = if self.consume_keyword("internal") {
            EffectVisibility::Internal
        } else {
            EffectVisibility::Addable
        };
        self.keyword("effect")?;
        let name = self.identifier("effect name")?;
        self.symbol('{')?;
        let mut params = Vec::new();
        let mut entrypoint = None;
        while !self.at_symbol('}') && !self.at_eof() {
            if self.at_keyword("param") {
                params.push(self.param()?);
            } else {
                if entrypoint.is_some() {
                    return Err(self.error_here("effect must declare exactly one entrypoint"));
                }
                entrypoint = Some(self.entrypoint()?);
            }
        }
        self.symbol('}')?;
        Ok(EffectAst {
            name,
            visibility,
            imports,
            params,
            entrypoint: entrypoint.ok_or_else(|| self.error_here("missing effect entrypoint"))?,
        })
    }

    fn import(&mut self) -> Result<EffectImport, ScriptDiagnostic> {
        self.keyword("use")?;
        let token = self.advance().clone();
        let path = match token.kind {
            TokenKind::String(value) => value,
            _ => return Err(self.error_at(&token, "expected import path string")),
        };
        self.keyword("as")?;
        let alias = self.identifier("import alias")?;
        self.symbol(';')?;
        Ok(EffectImport { path, alias })
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
            if value_type == ScriptType::Marks {
                return Err(self.error_here("marks parameters cannot declare defaults"));
            }
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

    fn entrypoint(&mut self) -> Result<EffectEntrypoint, ScriptDiagnostic> {
        if self.consume_keyword("void") {
            let name = self.identifier("function name")?;
            if name != "generate" {
                return Err(self.error_here("void entrypoint must be generate"));
            }
            self.symbol('(')?;
            self.expect_arg("Timeline", "timeline")?;
            self.symbol(',')?;
            self.expect_arg("Target", "target")?;
            self.symbol(',')?;
            self.expect_arg("float", "duration")?;
            self.symbol(')')?;
            self.symbol('{')?;
            let statements = self.block_statements()?;
            self.symbol('}')?;
            return Ok(EffectEntrypoint::Generator(statements));
        }
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
        let statements = self.block_statements()?;
        self.symbol('}')?;
        Ok(EffectEntrypoint::Sample(statements))
    }

    fn block_statements(&mut self) -> Result<Vec<Stmt>, ScriptDiagnostic> {
        let mut statements = Vec::new();
        while !self.at_symbol('}') && !self.at_eof() {
            statements.push(self.statement()?);
        }
        Ok(statements)
    }

    fn statement(&mut self) -> Result<Stmt, ScriptDiagnostic> {
        if self.consume_keyword("return") {
            let expr = self.expr()?;
            self.symbol(';')?;
            return Ok(Stmt::Return(expr));
        }
        if self.consume_keyword("for") {
            return self.for_statement();
        }
        if self.consume_keyword("if") {
            return self.if_statement();
        }
        if self.at_keyword("timeline")
            && self
                .token_at(self.index + 1)
                .is_some_and(|token| matches!(token.kind, TokenKind::Symbol('.')))
            && self.token_at(self.index + 2).is_some_and(
                |token| matches!(&token.kind, TokenKind::Ident(value) if value == "emit"),
            )
        {
            return self.emit_statement();
        }
        if self.peek_type_name() {
            return self.let_statement(true);
        }
        if let Some(name) = self.peek_assignment_name() {
            self.identifier("local name")?;
            self.symbol('=')?;
            let expr = self.expr()?;
            self.symbol(';')?;
            return Ok(Stmt::Assign { name, expr });
        }
        let expr = self.expr()?;
        self.symbol(';')?;
        Ok(Stmt::Expr(expr))
    }

    fn emit_statement(&mut self) -> Result<Stmt, ScriptDiagnostic> {
        self.keyword("timeline")?;
        self.symbol('.')?;
        self.keyword("emit")?;
        let first = self.identifier("effect name")?;
        let effect = if self.consume_symbol('.') {
            EmitEffectRef::Imported {
                alias: first,
                name: self.identifier("effect name")?,
            }
        } else {
            EmitEffectRef::Local { name: first }
        };
        self.symbol('{')?;
        let mut target = None;
        let mut start = None;
        let mut duration = None;
        let mut params = None;
        while !self.at_symbol('}') && !self.at_eof() {
            let field = self.identifier("emit field")?;
            self.symbol(':')?;
            match field.as_str() {
                "target" => {
                    target = Some(self.expr()?);
                    self.symbol(';')?;
                }
                "start" => {
                    start = Some(self.expr()?);
                    self.symbol(';')?;
                }
                "duration" => {
                    duration = Some(self.expr()?);
                    self.symbol(';')?;
                }
                "params" => {
                    self.symbol('{')?;
                    let mut values = Vec::new();
                    while !self.at_symbol('}') && !self.at_eof() {
                        let name = self.identifier("emitted parameter")?;
                        self.symbol(':')?;
                        let expr = self.expr()?;
                        self.symbol(';')?;
                        values.push(EmitParam { name, expr });
                    }
                    self.symbol('}')?;
                    self.symbol(';')?;
                    params = Some(values);
                }
                _ => return Err(self.error_here(&format!("unknown emit field `{field}`"))),
            }
        }
        self.symbol('}')?;
        self.symbol(';')?;
        Ok(Stmt::Emit(EmitStmt {
            effect,
            target: target.ok_or_else(|| self.error_here("emit is missing target"))?,
            start: start.ok_or_else(|| self.error_here("emit is missing start"))?,
            duration: duration.ok_or_else(|| self.error_here("emit is missing duration"))?,
            params: params.unwrap_or_default(),
        }))
    }

    fn let_statement(&mut self, semicolon: bool) -> Result<Stmt, ScriptDiagnostic> {
        let value_type = self.type_name()?;
        let name = self.identifier("local name")?;
        self.symbol('=')?;
        let expr = self.expr()?;
        if semicolon {
            self.symbol(';')?;
        }
        Ok(Stmt::Let {
            name,
            value_type,
            expr,
        })
    }

    fn for_statement(&mut self) -> Result<Stmt, ScriptDiagnostic> {
        self.symbol('(')?;
        let Stmt::Let {
            name,
            value_type,
            expr: initializer,
        } = self.let_statement(false)?
        else {
            unreachable!("for initializer parser only returns let statements");
        };
        self.symbol(';')?;
        let condition = self.expr()?;
        self.symbol(';')?;
        let update = self.assignment_without_semicolon()?;
        self.symbol(')')?;
        self.symbol('{')?;
        let body = self.block_statements()?;
        self.symbol('}')?;
        Ok(Stmt::For {
            name,
            value_type,
            initializer,
            condition,
            update: Box::new(update),
            body,
        })
    }

    fn if_statement(&mut self) -> Result<Stmt, ScriptDiagnostic> {
        self.symbol('(')?;
        let condition = self.expr()?;
        self.symbol(')')?;
        let then_body = self.braced_block()?;
        let else_body = if self.consume_keyword("else") {
            if self.consume_keyword("if") {
                vec![self.if_statement()?]
            } else {
                self.braced_block()?
            }
        } else {
            Vec::new()
        };
        Ok(Stmt::If {
            condition,
            then_body,
            else_body,
        })
    }

    fn braced_block(&mut self) -> Result<Vec<Stmt>, ScriptDiagnostic> {
        self.symbol('{')?;
        let statements = self.block_statements()?;
        self.symbol('}')?;
        Ok(statements)
    }

    fn assignment_without_semicolon(&mut self) -> Result<Stmt, ScriptDiagnostic> {
        let name = self.identifier("local name")?;
        self.symbol('=')?;
        let expr = self.expr()?;
        Ok(Stmt::Assign { name, expr })
    }

    fn expr(&mut self) -> Result<Expr, ScriptDiagnostic> {
        self.logical_or()
    }

    fn logical_or(&mut self) -> Result<Expr, ScriptDiagnostic> {
        let mut expr = self.logical_and()?;
        while self.consume_symbol_pair('|', '|') {
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::LogicalOr,
                right: Box::new(self.logical_and()?),
            };
        }
        Ok(expr)
    }

    fn logical_and(&mut self) -> Result<Expr, ScriptDiagnostic> {
        let mut expr = self.equality()?;
        while self.consume_symbol_pair('&', '&') {
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::LogicalAnd,
                right: Box::new(self.equality()?),
            };
        }
        Ok(expr)
    }

    fn equality(&mut self) -> Result<Expr, ScriptDiagnostic> {
        let mut expr = self.comparison()?;
        loop {
            let op = if self.consume_symbol_pair('=', '=') {
                BinaryOp::Equal
            } else if self.consume_symbol_pair('!', '=') {
                BinaryOp::NotEqual
            } else {
                break;
            };
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(self.comparison()?),
            };
        }
        Ok(expr)
    }

    fn comparison(&mut self) -> Result<Expr, ScriptDiagnostic> {
        let mut expr = self.additive()?;
        loop {
            let op = if self.consume_symbol_pair('<', '=') {
                BinaryOp::LessEqual
            } else if self.consume_symbol_pair('>', '=') {
                BinaryOp::GreaterEqual
            } else if self.consume_symbol('<') {
                BinaryOp::Less
            } else if self.consume_symbol('>') {
                BinaryOp::Greater
            } else {
                break;
            };
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(self.additive()?),
            };
        }
        Ok(expr)
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
        } else if self.consume_symbol('!') {
            Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(self.unary()?),
            })
        } else {
            self.primary()
        }
    }

    fn primary(&mut self) -> Result<Expr, ScriptDiagnostic> {
        let token = self.advance().clone();
        let mut expr = match &token.kind {
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
        }?;
        while self.consume_symbol('.') {
            let member = self.identifier("member name")?;
            expr = match expr {
                Expr::Ident(alias)
                    if alias != "item" && alias != "target" && alias != "timeline" =>
                {
                    Expr::Qualified {
                        alias,
                        name: member,
                    }
                }
                _ => Expr::Member {
                    object: Box::new(expr),
                    member,
                },
            };
        }
        Ok(expr)
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
        let value = params::eval_constant(&expr)?;
        if is_assignable(value_type, value.value_type()) {
            params::coerce_value(value, value_type).map_err(|error| ScriptDiagnostic {
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
            "Timeline" => ScriptType::Timeline,
            "Target" => ScriptType::Target,
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
            "marks" => Ok(ScriptType::Marks),
            "enum" => Ok(ScriptType::Enum),
            "flags" => Ok(ScriptType::Flags),
            "Fixture" => Ok(ScriptType::Fixture),
            "Pixel" => Ok(ScriptType::Pixel),
            "Timeline" => Ok(ScriptType::Timeline),
            "Target" => Ok(ScriptType::Target),
            "TargetItems" => Ok(ScriptType::TargetItems),
            "TargetItem" => Ok(ScriptType::TargetItem),
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

    fn consume_symbol_pair(&mut self, first: char, second: char) -> bool {
        if self.at_symbol(first)
            && self.token_at(self.index + 1).is_some_and(
                |token| matches!(token.kind, TokenKind::Symbol(value) if value == second),
            )
        {
            self.index += 2;
            true
        } else {
            false
        }
    }

    fn at_symbol(&self, symbol: char) -> bool {
        matches!(self.peek().kind, TokenKind::Symbol(value) if value == symbol)
    }

    fn peek_type_name(&self) -> bool {
        match &self.peek().kind {
            TokenKind::Ident(value)
                if matches!(
                    value.as_str(),
                    "float"
                        | "int"
                        | "bool"
                        | "color"
                        | "marks"
                        | "enum"
                        | "flags"
                        | "Fixture"
                        | "Pixel"
                        | "Timeline"
                        | "Target"
                        | "TargetItems"
                        | "TargetItem"
                ) =>
            {
                true
            }
            TokenKind::Ident(value) if value == "curve" => true,
            _ => false,
        }
    }

    fn peek_assignment_name(&self) -> Option<String> {
        let TokenKind::Ident(name) = &self.peek().kind else {
            return None;
        };
        self.token_at(self.index + 1)
            .filter(|token| matches!(token.kind, TokenKind::Symbol('=')))
            .filter(|_| {
                !self
                    .token_at(self.index + 2)
                    .is_some_and(|token| matches!(token.kind, TokenKind::Symbol('=')))
            })
            .map(|_| name.clone())
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
        if let Some(token) = self.token_at(self.index) {
            token
        } else {
            &self.tokens[self.tokens.len() - 1]
        }
    }

    fn token_at(&self, index: usize) -> Option<&Token> {
        self.tokens.get(index)
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
