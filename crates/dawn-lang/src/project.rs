use std::path::Path;

use crate::{Diagnostic, DiagnosticCode, Span, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Int(i64),
    Float(f64),
    String(String),
    Color(u8, u8, u8),
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LParen,
    RParen,
    Colon,
    Comma,
    Semicolon,
    Eq,
    Plus,
    Minus,
    Star,
    Slash,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NamedInclude {
    pub name: Ident,
    pub path: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProjectDocument {
    Project(ProjectDoc),
    Display(DisplayDoc),
    Controllers(ControllerDoc),
    Layout(LayoutDoc),
    Patch(PatchDoc),
    Sequence(SequenceDoc),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectDoc {
    pub name: Ident,
    pub version: u32,
    pub displays: Vec<NamedInclude>,
    pub sequences: Vec<NamedInclude>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayDoc {
    pub name: Ident,
    pub version: u32,
    pub consts: Vec<ConstDecl>,
    pub fixtures: Vec<FixtureSource>,
    pub groups: Vec<GroupSource>,
    pub includes: Vec<SectionInclude>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SectionInclude {
    pub section: Ident,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ControllerDoc {
    pub version: u32,
    pub controllers: Vec<ControllerSource>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutDoc {
    pub version: u32,
    pub fixtures: Vec<LayoutSource>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PatchDoc {
    pub version: u32,
    pub patches: Vec<PatchSource>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SequenceDoc {
    pub name: Ident,
    pub version: u32,
    pub display: Ident,
    pub duration: f64,
    pub frame_rate: f64,
    pub audio: Option<String>,
    pub scripts: Vec<ScriptSource>,
    pub events: Vec<EventSource>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstDecl {
    pub name: Ident,
    pub ty: Ident,
    pub value: ConstExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FixtureSource {
    pub name: Ident,
    pub pixel_count: ConstExpr,
    pub color_model: Option<Ident>,
    pub channel_order: Option<Ident>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupSource {
    pub name: Ident,
    pub members: Vec<Ident>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ControllerSource {
    pub name: Ident,
    pub address: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutSource {
    pub fixture: Ident,
    pub shape: Option<LayoutShapeSource>,
    pub positions: Vec<PositionSource>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LayoutShapeSource {
    Line {
        start: PositionSource,
        end: PositionSource,
    },
    Grid {
        top_left: PositionSource,
        bottom_right: PositionSource,
        columns: u32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionSource {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PatchSource {
    pub fixture: Ident,
    pub controller: Ident,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScriptSource {
    pub name: Ident,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EventSource {
    pub target: Ident,
    pub effect: Ident,
    pub start: f64,
    pub duration: f64,
    pub params: Vec<ParamAssignment>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParamAssignment {
    pub name: Ident,
    pub value: ConstExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstExpr {
    pub kind: ConstExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConstExprKind {
    Value(Value),
    Ref(String),
    Unary {
        op: ConstUnaryOp,
        expr: Box<ConstExpr>,
    },
    Binary {
        op: ConstBinaryOp,
        left: Box<ConstExpr>,
        right: Box<ConstExpr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstUnaryOp {
    Neg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
}

pub fn lex(path: &Path, source: &str) -> Result<Vec<Token>, Vec<Diagnostic>> {
    Lexer::new(path, source).tokenize()
}

pub fn parse_document(path: &Path, source: &str) -> Result<ProjectDocument, Vec<Diagnostic>> {
    let tokens = lex(path, source)?;
    Parser::new(path, tokens).parse_document()
}

struct Lexer<'a> {
    path: &'a Path,
    source: &'a str,
    bytes: &'a [u8],
    pos: usize,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn new(path: &'a Path, source: &'a str) -> Self {
        Self {
            path,
            source,
            bytes: source.as_bytes(),
            pos: 0,
            tokens: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn tokenize(mut self) -> Result<Vec<Token>, Vec<Diagnostic>> {
        while self.pos < self.bytes.len() {
            self.skip_ws_and_comments();
            if self.pos >= self.bytes.len() {
                break;
            }
            let start = self.pos;
            match self.bytes[self.pos] {
                b'{' => self.one(TokenKind::LBrace, start),
                b'}' => self.one(TokenKind::RBrace, start),
                b'[' => self.one(TokenKind::LBracket, start),
                b']' => self.one(TokenKind::RBracket, start),
                b'(' => self.one(TokenKind::LParen, start),
                b')' => self.one(TokenKind::RParen, start),
                b':' => self.one(TokenKind::Colon, start),
                b',' => self.one(TokenKind::Comma, start),
                b';' => self.one(TokenKind::Semicolon, start),
                b'=' => self.one(TokenKind::Eq, start),
                b'+' => self.one(TokenKind::Plus, start),
                b'-' => self.one(TokenKind::Minus, start),
                b'*' => self.one(TokenKind::Star, start),
                b'/' => self.one(TokenKind::Slash, start),
                b'"' => self.string(start),
                b'#' => self.color(start),
                b'0'..=b'9' => self.number(start),
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.ident(start),
                other => {
                    self.diagnostics.push(
                        Diagnostic::error(
                            self.path,
                            DiagnosticCode::Lex,
                            format!("unexpected character '{}'", other as char),
                        )
                        .at(Span::new(start, start + 1)),
                    );
                    self.pos += 1;
                }
            }
        }
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            span: Span::new(self.pos, self.pos),
        });
        if self.diagnostics.is_empty() {
            Ok(self.tokens)
        } else {
            Err(self.diagnostics)
        }
    }

    fn one(&mut self, kind: TokenKind, start: usize) {
        self.pos += 1;
        self.tokens.push(Token {
            kind,
            span: Span::new(start, self.pos),
        });
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            while self.pos < self.bytes.len()
                && matches!(self.bytes[self.pos], b' ' | b'\t' | b'\r' | b'\n')
            {
                self.pos += 1;
            }
            if self.bytes.get(self.pos) == Some(&b'/')
                && self.bytes.get(self.pos + 1) == Some(&b'/')
            {
                self.pos += 2;
                while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            break;
        }
    }

    fn string(&mut self, start: usize) {
        self.pos += 1;
        let content_start = self.pos;
        while self.pos < self.bytes.len()
            && self.bytes[self.pos] != b'"'
            && self.bytes[self.pos] != b'\n'
        {
            self.pos += 1;
        }
        if self.bytes.get(self.pos) == Some(&b'"') {
            let value = self.source[content_start..self.pos].to_string();
            self.pos += 1;
            self.tokens.push(Token {
                kind: TokenKind::String(value),
                span: Span::new(start, self.pos),
            });
        } else {
            self.diagnostics.push(
                Diagnostic::error(
                    self.path,
                    DiagnosticCode::Lex,
                    "unterminated string literal",
                )
                .at(Span::new(start, self.pos)),
            );
        }
    }

    fn color(&mut self, start: usize) {
        self.pos += 1;
        let hex_start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_hexdigit() {
            self.pos += 1;
        }
        let hex = &self.source[hex_start..self.pos];
        let color = match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1], 16).ok();
                let g = u8::from_str_radix(&hex[1..2], 16).ok();
                let b = u8::from_str_radix(&hex[2..3], 16).ok();
                match (r, g, b) {
                    (Some(r), Some(g), Some(b)) => Some((r * 17, g * 17, b * 17)),
                    _ => None,
                }
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok();
                let g = u8::from_str_radix(&hex[2..4], 16).ok();
                let b = u8::from_str_radix(&hex[4..6], 16).ok();
                match (r, g, b) {
                    (Some(r), Some(g), Some(b)) => Some((r, g, b)),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some((r, g, b)) = color {
            self.tokens.push(Token {
                kind: TokenKind::Color(r, g, b),
                span: Span::new(start, self.pos),
            });
        } else {
            self.diagnostics.push(
                Diagnostic::error(self.path, DiagnosticCode::Lex, "invalid color literal")
                    .at(Span::new(start, self.pos)),
            );
        }
    }

    fn number(&mut self, start: usize) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        let mut is_float = false;
        if self.bytes.get(self.pos) == Some(&b'.')
            && self.bytes.get(self.pos + 1).is_some_and(u8::is_ascii_digit)
        {
            is_float = true;
            self.pos += 1;
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        let text = &self.source[start..self.pos];
        if is_float {
            match text.parse::<f64>() {
                Ok(value) => self.tokens.push(Token {
                    kind: TokenKind::Float(value),
                    span: Span::new(start, self.pos),
                }),
                Err(_) => self.diagnostics.push(
                    Diagnostic::error(self.path, DiagnosticCode::Lex, "invalid float literal")
                        .at(Span::new(start, self.pos)),
                ),
            }
        } else {
            match text.parse::<i64>() {
                Ok(value) => self.tokens.push(Token {
                    kind: TokenKind::Int(value),
                    span: Span::new(start, self.pos),
                }),
                Err(_) => self.diagnostics.push(
                    Diagnostic::error(self.path, DiagnosticCode::Lex, "invalid int literal")
                        .at(Span::new(start, self.pos)),
                ),
            }
        }
    }

    fn ident(&mut self, start: usize) {
        while self.pos < self.bytes.len()
            && (self.bytes[self.pos].is_ascii_alphanumeric() || self.bytes[self.pos] == b'_')
        {
            self.pos += 1;
        }
        self.tokens.push(Token {
            kind: TokenKind::Ident(self.source[start..self.pos].to_string()),
            span: Span::new(start, self.pos),
        });
    }
}

struct Parser<'a> {
    path: &'a Path,
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Parser<'a> {
    fn new(path: &'a Path, tokens: Vec<Token>) -> Self {
        Self {
            path,
            tokens,
            pos: 0,
            diagnostics: Vec::new(),
        }
    }

    fn parse_document(mut self) -> Result<ProjectDocument, Vec<Diagnostic>> {
        let result = match self.peek_ident() {
            Some("project") => self.parse_project().map(ProjectDocument::Project),
            Some("display") => self.parse_display().map(ProjectDocument::Display),
            Some("controllers") => self.parse_controllers().map(ProjectDocument::Controllers),
            Some("layout") => self.parse_layout_doc().map(ProjectDocument::Layout),
            Some("patch") => self.parse_patch_doc().map(ProjectDocument::Patch),
            Some("sequence") => self.parse_sequence().map(ProjectDocument::Sequence),
            _ => Err(self.error(
                "expected document kind: project, display, controllers, layout, patch, or sequence",
            )),
        };
        match result {
            Ok(doc) if self.diagnostics.is_empty() => Ok(doc),
            Ok(_) | Err(_) => Err(self.diagnostics),
        }
    }

    fn parse_project(&mut self) -> Result<ProjectDoc, ()> {
        self.keyword("project")?;
        let name = self.ident()?;
        self.expect(TokenKind::LBrace)?;
        let mut version = None;
        let mut displays = Vec::new();
        let mut sequences = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            match self.peek_ident() {
                Some("version") => version = Some(self.version_stmt()?),
                Some("display") => displays.push(self.named_include("display")?),
                Some("sequence") => sequences.push(self.named_include("sequence")?),
                _ => self.unexpected_stmt()?,
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(ProjectDoc {
            name,
            version: version.unwrap_or(1),
            displays,
            sequences,
        })
    }

    fn parse_display(&mut self) -> Result<DisplayDoc, ()> {
        self.keyword("display")?;
        let name = self.ident()?;
        self.expect(TokenKind::LBrace)?;
        let mut version = None;
        let mut consts = Vec::new();
        let mut fixtures = Vec::new();
        let mut groups = Vec::new();
        let mut includes = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            match self.peek_ident() {
                Some("version") => version = Some(self.version_stmt()?),
                Some("const") => consts.push(self.const_decl()?),
                Some("fixture") => fixtures.push(self.fixture()?),
                Some("group") => groups.push(self.group()?),
                Some("include") => includes.push(self.include_stmt()?),
                _ => self.unexpected_stmt()?,
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(DisplayDoc {
            name,
            version: version.unwrap_or(1),
            consts,
            fixtures,
            groups,
            includes,
        })
    }

    fn parse_controllers(&mut self) -> Result<ControllerDoc, ()> {
        self.keyword("controllers")?;
        self.expect(TokenKind::LBrace)?;
        let mut version = None;
        let mut controllers = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            match self.peek_ident() {
                Some("version") => version = Some(self.version_stmt()?),
                Some("controller") => controllers.push(self.controller()?),
                _ => self.unexpected_stmt()?,
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(ControllerDoc {
            version: version.unwrap_or(1),
            controllers,
        })
    }

    fn parse_layout_doc(&mut self) -> Result<LayoutDoc, ()> {
        self.keyword("layout")?;
        self.expect(TokenKind::LBrace)?;
        let mut version = None;
        let mut fixtures = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            match self.peek_ident() {
                Some("version") => version = Some(self.version_stmt()?),
                Some("fixture") => fixtures.push(self.layout_fixture()?),
                _ => self.unexpected_stmt()?,
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(LayoutDoc {
            version: version.unwrap_or(1),
            fixtures,
        })
    }

    fn parse_patch_doc(&mut self) -> Result<PatchDoc, ()> {
        self.keyword("patch")?;
        self.expect(TokenKind::LBrace)?;
        let mut version = None;
        let mut patches = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            match self.peek_ident() {
                Some("version") => version = Some(self.version_stmt()?),
                Some("fixture") => patches.push(self.patch_fixture()?),
                _ => self.unexpected_stmt()?,
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(PatchDoc {
            version: version.unwrap_or(1),
            patches,
        })
    }

    fn parse_sequence(&mut self) -> Result<SequenceDoc, ()> {
        self.keyword("sequence")?;
        let name = self.ident()?;
        self.expect(TokenKind::LBrace)?;
        let mut version = None;
        let mut display = None;
        let mut duration = None;
        let mut frame_rate = 40.0;
        let mut audio = None;
        let mut scripts = Vec::new();
        let mut events = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            match self.peek_ident() {
                Some("version") => version = Some(self.version_stmt()?),
                Some("display") => {
                    self.keyword("display")?;
                    display = Some(self.ident()?);
                    self.expect(TokenKind::Semicolon)?;
                }
                Some("duration") => {
                    self.keyword("duration")?;
                    duration = Some(self.duration_expr()?);
                    self.expect(TokenKind::Semicolon)?;
                }
                Some("frame_rate") => {
                    self.keyword("frame_rate")?;
                    frame_rate = self.number_expr()?;
                    self.expect(TokenKind::Semicolon)?;
                }
                Some("audio") => {
                    self.keyword("audio")?;
                    audio = Some(self.string()?);
                    self.expect(TokenKind::Semicolon)?;
                }
                Some("script") => scripts.push(self.script_include()?),
                Some("event") => events.push(self.event()?),
                _ => self.unexpected_stmt()?,
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(SequenceDoc {
            name,
            version: version.unwrap_or(1),
            display: display.unwrap_or_else(|| Ident {
                name: String::new(),
                span: Span::default(),
            }),
            duration: duration.unwrap_or(0.0),
            frame_rate,
            audio,
            scripts,
            events,
        })
    }

    fn version_stmt(&mut self) -> Result<u32, ()> {
        self.keyword("version")?;
        let version = self.int()?;
        self.expect(TokenKind::Semicolon)?;
        Ok(version as u32)
    }

    fn named_include(&mut self, keyword: &str) -> Result<NamedInclude, ()> {
        let start = self.keyword(keyword)?;
        let name = self.ident()?;
        self.keyword("from")?;
        let path = self.string()?;
        let end = self.expect(TokenKind::Semicolon)?;
        Ok(NamedInclude {
            name,
            path,
            span: start.merge(end),
        })
    }

    fn include_stmt(&mut self) -> Result<SectionInclude, ()> {
        self.keyword("include")?;
        let section = self.ident()?;
        self.keyword("from")?;
        let path = self.string()?;
        self.expect(TokenKind::Semicolon)?;
        Ok(SectionInclude { section, path })
    }

    fn script_include(&mut self) -> Result<ScriptSource, ()> {
        self.keyword("script")?;
        let name = self.ident()?;
        self.keyword("from")?;
        let path = self.string()?;
        self.expect(TokenKind::Semicolon)?;
        Ok(ScriptSource { name, path })
    }

    fn const_decl(&mut self) -> Result<ConstDecl, ()> {
        self.keyword("const")?;
        let name = self.ident()?;
        self.expect(TokenKind::Colon)?;
        let ty = self.ident()?;
        self.expect(TokenKind::Eq)?;
        let value = self.const_expr()?;
        self.expect(TokenKind::Semicolon)?;
        Ok(ConstDecl { name, ty, value })
    }

    fn fixture(&mut self) -> Result<FixtureSource, ()> {
        self.keyword("fixture")?;
        let name = self.ident()?;
        self.expect(TokenKind::LBrace)?;
        let mut pixel_count = None;
        let mut color_model = None;
        let mut channel_order = None;
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            match self.peek_ident() {
                Some("pixel_count") => {
                    self.keyword("pixel_count")?;
                    pixel_count = Some(self.const_expr()?);
                    self.expect(TokenKind::Semicolon)?;
                }
                Some("color_model") => {
                    self.keyword("color_model")?;
                    color_model = Some(self.ident()?);
                    self.expect(TokenKind::Semicolon)?;
                }
                Some("channel_order") => {
                    self.keyword("channel_order")?;
                    channel_order = Some(self.ident()?);
                    self.expect(TokenKind::Semicolon)?;
                }
                _ => self.unexpected_stmt()?,
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(FixtureSource {
            name,
            pixel_count: pixel_count.unwrap_or(ConstExpr {
                kind: ConstExprKind::Value(Value::Int(0)),
                span: Span::default(),
            }),
            color_model,
            channel_order,
        })
    }

    fn group(&mut self) -> Result<GroupSource, ()> {
        self.keyword("group")?;
        let name = self.ident()?;
        self.expect(TokenKind::LBrace)?;
        let mut members = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            match self.peek_ident() {
                Some("members") => {
                    self.keyword("members")?;
                    members = self.ident_array()?;
                    self.expect(TokenKind::Semicolon)?;
                }
                _ => self.unexpected_stmt()?,
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(GroupSource { name, members })
    }

    fn controller(&mut self) -> Result<ControllerSource, ()> {
        self.keyword("controller")?;
        let name = self.ident()?;
        self.expect(TokenKind::LBrace)?;
        let mut address = None;
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            match self.peek_ident() {
                Some("address") => {
                    self.keyword("address")?;
                    address = Some(self.string()?);
                    self.expect(TokenKind::Semicolon)?;
                }
                _ => self.unexpected_stmt()?,
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(ControllerSource { name, address })
    }

    fn layout_fixture(&mut self) -> Result<LayoutSource, ()> {
        self.keyword("fixture")?;
        let fixture = self.ident()?;
        self.expect(TokenKind::LBrace)?;
        let mut shape = None;
        let mut positions = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            match self.peek_ident() {
                Some("shape") => shape = Some(self.shape_stmt()?),
                Some("positions") => {
                    self.keyword("positions")?;
                    positions = self.positions_array()?;
                    self.expect(TokenKind::Semicolon)?;
                }
                _ => self.unexpected_stmt()?,
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(LayoutSource {
            fixture,
            shape,
            positions,
        })
    }

    fn shape_stmt(&mut self) -> Result<LayoutShapeSource, ()> {
        self.keyword("shape")?;
        match self.peek_ident() {
            Some("line") => {
                self.keyword("line")?;
                self.expect(TokenKind::LBrace)?;
                let mut start = None;
                let mut end = None;
                while !self.check(&TokenKind::RBrace) && !self.at_eof() {
                    match self.peek_ident() {
                        Some("start") => start = Some(self.position_stmt("start")?),
                        Some("end") => end = Some(self.position_stmt("end")?),
                        _ => self.unexpected_stmt()?,
                    }
                }
                self.expect(TokenKind::RBrace)?;
                Ok(LayoutShapeSource::Line {
                    start: start.unwrap_or(PositionSource { x: 0.0, y: 0.0 }),
                    end: end.unwrap_or(PositionSource { x: 1.0, y: 0.0 }),
                })
            }
            Some("grid") => {
                self.keyword("grid")?;
                self.expect(TokenKind::LBrace)?;
                let mut top_left = None;
                let mut bottom_right = None;
                let mut columns = 1;
                while !self.check(&TokenKind::RBrace) && !self.at_eof() {
                    match self.peek_ident() {
                        Some("top_left") => top_left = Some(self.position_stmt("top_left")?),
                        Some("bottom_right") => {
                            bottom_right = Some(self.position_stmt("bottom_right")?)
                        }
                        Some("columns") => {
                            self.keyword("columns")?;
                            columns = self.int()? as u32;
                            self.expect(TokenKind::Semicolon)?;
                        }
                        _ => self.unexpected_stmt()?,
                    }
                }
                self.expect(TokenKind::RBrace)?;
                Ok(LayoutShapeSource::Grid {
                    top_left: top_left.unwrap_or(PositionSource { x: 0.0, y: 0.0 }),
                    bottom_right: bottom_right.unwrap_or(PositionSource { x: 1.0, y: 1.0 }),
                    columns,
                })
            }
            _ => Err(self.error("expected shape kind 'line' or 'grid'")),
        }
    }

    fn position_stmt(&mut self, keyword: &str) -> Result<PositionSource, ()> {
        self.keyword(keyword)?;
        let pos = self.position_record()?;
        self.expect(TokenKind::Semicolon)?;
        Ok(pos)
    }

    fn patch_fixture(&mut self) -> Result<PatchSource, ()> {
        self.keyword("fixture")?;
        let fixture = self.ident()?;
        self.expect(TokenKind::LBrace)?;
        let mut controller = None;
        let mut port = 0;
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            match self.peek_ident() {
                Some("controller") => {
                    self.keyword("controller")?;
                    controller = Some(self.ident()?);
                    self.expect(TokenKind::Semicolon)?;
                }
                Some("port") => {
                    self.keyword("port")?;
                    port = self.int()? as u16;
                    self.expect(TokenKind::Semicolon)?;
                }
                _ => self.unexpected_stmt()?,
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(PatchSource {
            fixture,
            controller: controller.unwrap_or(Ident {
                name: String::new(),
                span: Span::default(),
            }),
            port,
        })
    }

    fn event(&mut self) -> Result<EventSource, ()> {
        self.keyword("event")?;
        let target = self.ident()?;
        self.expect(TokenKind::LBrace)?;
        let mut effect = None;
        let mut start = None;
        let mut duration = None;
        let mut params = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            match self.peek_ident() {
                Some("effect") => {
                    self.keyword("effect")?;
                    effect = Some(self.ident()?);
                    self.expect(TokenKind::Semicolon)?;
                }
                Some("at") => {
                    self.keyword("at")?;
                    start = Some(self.duration_expr()?);
                    self.keyword("for")?;
                    duration = Some(self.duration_expr()?);
                    self.expect(TokenKind::Semicolon)?;
                }
                Some("params") => {
                    self.keyword("params")?;
                    self.expect(TokenKind::LBrace)?;
                    while !self.check(&TokenKind::RBrace) && !self.at_eof() {
                        let name = self.ident()?;
                        let value = self.const_expr()?;
                        self.expect(TokenKind::Semicolon)?;
                        params.push(ParamAssignment { name, value });
                    }
                    self.expect(TokenKind::RBrace)?;
                }
                _ => self.unexpected_stmt()?,
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(EventSource {
            target,
            effect: effect.unwrap_or(Ident {
                name: String::new(),
                span: Span::default(),
            }),
            start: start.unwrap_or(0.0),
            duration: duration.unwrap_or(0.0),
            params,
        })
    }

    fn ident_array(&mut self) -> Result<Vec<Ident>, ()> {
        self.expect(TokenKind::LBracket)?;
        let mut items = Vec::new();
        while !self.check(&TokenKind::RBracket) && !self.at_eof() {
            items.push(self.ident()?);
            if self.check(&TokenKind::Comma) {
                self.advance();
            }
        }
        self.expect(TokenKind::RBracket)?;
        Ok(items)
    }

    fn positions_array(&mut self) -> Result<Vec<PositionSource>, ()> {
        self.expect(TokenKind::LBracket)?;
        let mut items = Vec::new();
        while !self.check(&TokenKind::RBracket) && !self.at_eof() {
            items.push(self.position_record()?);
            if self.check(&TokenKind::Comma) {
                self.advance();
            }
        }
        self.expect(TokenKind::RBracket)?;
        Ok(items)
    }

    fn position_record(&mut self) -> Result<PositionSource, ()> {
        self.expect(TokenKind::LBrace)?;
        let mut x = None;
        let mut y = None;
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            match self.peek_ident() {
                Some("x") => {
                    self.keyword("x")?;
                    x = Some(self.number_expr()?);
                    self.expect(TokenKind::Semicolon)?;
                }
                Some("y") => {
                    self.keyword("y")?;
                    y = Some(self.number_expr()?);
                    self.expect(TokenKind::Semicolon)?;
                }
                _ => self.unexpected_stmt()?,
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(PositionSource {
            x: x.unwrap_or(0.0),
            y: y.unwrap_or(0.0),
        })
    }

    fn const_expr(&mut self) -> Result<ConstExpr, ()> {
        self.additive()
    }

    fn additive(&mut self) -> Result<ConstExpr, ()> {
        let mut left = self.multiplicative()?;
        loop {
            let op = if self.check(&TokenKind::Plus) {
                ConstBinaryOp::Add
            } else if self.check(&TokenKind::Minus) {
                ConstBinaryOp::Sub
            } else {
                break;
            };
            self.advance();
            let right = self.multiplicative()?;
            let span = left.span.merge(right.span);
            left = ConstExpr {
                kind: ConstExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }
        Ok(left)
    }

    fn multiplicative(&mut self) -> Result<ConstExpr, ()> {
        let mut left = self.unary()?;
        loop {
            let op = if self.check(&TokenKind::Star) {
                ConstBinaryOp::Mul
            } else if self.check(&TokenKind::Slash) {
                ConstBinaryOp::Div
            } else {
                break;
            };
            self.advance();
            let right = self.unary()?;
            let span = left.span.merge(right.span);
            left = ConstExpr {
                kind: ConstExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<ConstExpr, ()> {
        if self.check(&TokenKind::Minus) {
            let start = self.span();
            self.advance();
            let expr = self.primary()?;
            let span = start.merge(expr.span);
            return Ok(ConstExpr {
                kind: ConstExprKind::Unary {
                    op: ConstUnaryOp::Neg,
                    expr: Box::new(expr),
                },
                span,
            });
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<ConstExpr, ()> {
        let span = self.span();
        match self.peek().clone() {
            TokenKind::Int(value) => {
                self.advance();
                Ok(ConstExpr {
                    kind: ConstExprKind::Value(Value::Int(value)),
                    span,
                })
            }
            TokenKind::Float(value) => {
                self.advance();
                Ok(ConstExpr {
                    kind: ConstExprKind::Value(Value::Float(value)),
                    span,
                })
            }
            TokenKind::String(value) => {
                self.advance();
                Ok(ConstExpr {
                    kind: ConstExprKind::Value(Value::String(value)),
                    span,
                })
            }
            TokenKind::Color(r, g, b) => {
                self.advance();
                Ok(ConstExpr {
                    kind: ConstExprKind::Value(Value::Color(r, g, b)),
                    span,
                })
            }
            TokenKind::Ident(value) if value == "true" || value == "false" => {
                self.advance();
                Ok(ConstExpr {
                    kind: ConstExprKind::Value(Value::Bool(value == "true")),
                    span,
                })
            }
            TokenKind::Ident(value) => {
                self.advance();
                Ok(ConstExpr {
                    kind: ConstExprKind::Ref(value),
                    span,
                })
            }
            TokenKind::LParen => {
                self.advance();
                let expr = self.const_expr()?;
                self.expect(TokenKind::RParen)?;
                Ok(expr)
            }
            TokenKind::LBracket => self.array_expr(),
            TokenKind::LBrace => self.record_expr(),
            _ => Err(self.error("expected const expression")),
        }
    }

    fn array_expr(&mut self) -> Result<ConstExpr, ()> {
        let start = self.span();
        self.expect(TokenKind::LBracket)?;
        let mut items = Vec::new();
        while !self.check(&TokenKind::RBracket) && !self.at_eof() {
            items.push(self.const_expr()?);
            if self.check(&TokenKind::Comma) {
                self.advance();
            }
        }
        let end = self.expect(TokenKind::RBracket)?;
        let values = items
            .into_iter()
            .map(fold_const_expr_value)
            .collect::<Option<Vec<_>>>();
        let Some(values) = values else {
            return Err(self.error("array const expressions may only contain compile-time values"));
        };
        Ok(ConstExpr {
            kind: ConstExprKind::Value(Value::Array(values)),
            span: start.merge(end),
        })
    }

    fn record_expr(&mut self) -> Result<ConstExpr, ()> {
        let start = self.span();
        self.expect(TokenKind::LBrace)?;
        let mut fields = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            let name = self.ident()?;
            let value = self.const_expr()?;
            self.expect(TokenKind::Semicolon)?;
            let Some(value) = fold_const_expr_value(value) else {
                return Err(
                    self.error("record const expressions may only contain compile-time values")
                );
            };
            fields.push((name.name, value));
        }
        let end = self.expect(TokenKind::RBrace)?;
        Ok(ConstExpr {
            kind: ConstExprKind::Value(Value::Record(fields)),
            span: start.merge(end),
        })
    }

    fn duration_expr(&mut self) -> Result<f64, ()> {
        let number = self.number_expr()?;
        if self.peek_ident() == Some("s") {
            self.advance();
        }
        Ok(number)
    }

    fn number_expr(&mut self) -> Result<f64, ()> {
        let expr = self.const_expr()?;
        match self.eval_const(&expr)? {
            Value::Float(value) => Ok(value),
            Value::Int(value) => Ok(value as f64),
            _ => Err(self.error("expected numeric expression")),
        }
    }

    fn eval_const(&mut self, expr: &ConstExpr) -> Result<Value, ()> {
        match &expr.kind {
            ConstExprKind::Value(value) => Ok(value.clone()),
            ConstExprKind::Unary {
                op: ConstUnaryOp::Neg,
                expr,
            } => match self.eval_const(expr)? {
                Value::Int(value) => Ok(Value::Int(-value)),
                Value::Float(value) => Ok(Value::Float(-value)),
                _ => Err(self.error("unary '-' requires a number")),
            },
            ConstExprKind::Binary { op, left, right } => {
                let left = self.eval_const(left)?;
                let right = self.eval_const(right)?;
                eval_binary(*op, left, right)
                    .ok_or_else(|| self.error("binary const operator requires numbers"))
            }
            ConstExprKind::Ref(name) => Ok(Value::Ref(name.clone())),
        }
    }

    fn int(&mut self) -> Result<i64, ()> {
        match self.peek().clone() {
            TokenKind::Int(value) => {
                self.advance();
                Ok(value)
            }
            _ => Err(self.error("expected integer")),
        }
    }

    fn string(&mut self) -> Result<String, ()> {
        match self.peek().clone() {
            TokenKind::String(value) => {
                self.advance();
                Ok(value)
            }
            _ => Err(self.error("expected string literal")),
        }
    }

    fn ident(&mut self) -> Result<Ident, ()> {
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                let span = self.span();
                self.advance();
                Ok(Ident { name, span })
            }
            _ => Err(self.error("expected identifier")),
        }
    }

    fn keyword(&mut self, keyword: &str) -> Result<Span, ()> {
        match self.peek() {
            TokenKind::Ident(value) if value == keyword => {
                let span = self.span();
                self.advance();
                Ok(span)
            }
            _ => Err(self.error(format!("expected '{keyword}'"))),
        }
    }

    fn expect(&mut self, expected: TokenKind) -> Result<Span, ()> {
        if self.check(&expected) {
            let span = self.span();
            self.advance();
            Ok(span)
        } else {
            Err(self.error(format!("expected {:?}", expected)))
        }
    }

    fn unexpected_stmt(&mut self) -> Result<(), ()> {
        self.error("unexpected statement");
        while !self.check(&TokenKind::Semicolon)
            && !self.check(&TokenKind::RBrace)
            && !self.at_eof()
        {
            self.advance();
        }
        if self.check(&TokenKind::Semicolon) {
            self.advance();
        }
        Err(())
    }

    fn error(&mut self, message: impl Into<String>) {
        self.diagnostics
            .push(Diagnostic::error(self.path, DiagnosticCode::Parse, message).at(self.span()));
    }

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek_ident(&self) -> Option<&str> {
        match self.peek() {
            TokenKind::Ident(value) => Some(value.as_str()),
            _ => None,
        }
    }

    fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(kind)
    }

    fn span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map_or_else(Span::default, |token| token.span)
    }

    fn at_eof(&self) -> bool {
        self.check(&TokenKind::Eof)
    }

    fn advance(&mut self) {
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
    }
}

fn eval_binary(op: ConstBinaryOp, left: Value, right: Value) -> Option<Value> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => match op {
            ConstBinaryOp::Add => Some(Value::Int(left + right)),
            ConstBinaryOp::Sub => Some(Value::Int(left - right)),
            ConstBinaryOp::Mul => Some(Value::Int(left * right)),
            ConstBinaryOp::Div => Some(Value::Float(left as f64 / right as f64)),
        },
        (left, right) => {
            let left = match left {
                Value::Int(value) => value as f64,
                Value::Float(value) => value,
                _ => return None,
            };
            let right = match right {
                Value::Int(value) => value as f64,
                Value::Float(value) => value,
                _ => return None,
            };
            match op {
                ConstBinaryOp::Add => Some(Value::Float(left + right)),
                ConstBinaryOp::Sub => Some(Value::Float(left - right)),
                ConstBinaryOp::Mul => Some(Value::Float(left * right)),
                ConstBinaryOp::Div => Some(Value::Float(left / right)),
            }
        }
    }
}

fn fold_const_expr_value(expr: ConstExpr) -> Option<Value> {
    match expr.kind {
        ConstExprKind::Value(value) => Some(value),
        ConstExprKind::Ref(name) => Some(Value::Ref(name)),
        ConstExprKind::Unary {
            op: ConstUnaryOp::Neg,
            expr,
        } => match fold_const_expr_value(*expr)? {
            Value::Int(value) => Some(Value::Int(-value)),
            Value::Float(value) => Some(Value::Float(-value)),
            _ => None,
        },
        ConstExprKind::Binary { op, left, right } => eval_binary(
            op,
            fold_const_expr_value(*left)?,
            fold_const_expr_value(*right)?,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_project_file() {
        let doc = parse_document(
            Path::new("project.dawn"),
            r#"
project TestShow {
  version 1;
  display Main from "displays/Main.display.dawn";
  sequence Demo from "sequences/Demo.sequence.dawn";
}
"#,
        )
        .expect("parse");
        let ProjectDocument::Project(project) = doc else {
            panic!("expected project")
        };
        assert_eq!(project.name.name, "TestShow");
        assert_eq!(project.displays[0].name.name, "Main");
    }

    #[test]
    fn parses_display_file() {
        let doc = parse_document(
            Path::new("display.dawn"),
            r#"
display Main {
  version 1;
  const RoofPixels: Int = 50;
  fixture Roofline {
    pixel_count RoofPixels;
    color_model Rgb;
    channel_order Rgb;
  }
  group All { members [Roofline]; }
  include controllers from "controllers.dawn";
}
"#,
        )
        .expect("parse");
        let ProjectDocument::Display(display) = doc else {
            panic!("expected display")
        };
        assert_eq!(display.fixtures[0].name.name, "Roofline");
        assert_eq!(display.groups[0].members[0].name, "Roofline");
    }

    #[test]
    fn parses_sequence_file() {
        let doc = parse_document(
            Path::new("sequence.dawn"),
            r#"
sequence Demo {
  version 1;
  display Main;
  duration 10s;
  frame_rate 40;
  event All {
    effect Solid;
    at 0s for 10s;
    params { Color #40c4ff; }
  }
}
"#,
        )
        .expect("parse");
        let ProjectDocument::Sequence(sequence) = doc else {
            panic!("expected sequence")
        };
        assert_eq!(sequence.events[0].params[0].name.name, "Color");
    }

    #[test]
    fn parses_array_and_record_const_values() {
        let doc = parse_document(
            Path::new("sequence.dawn"),
            r#"
sequence Demo {
  version 1;
  display Main;
  duration 10s;
  event All {
    effect Solid;
    at 0s for 10s;
    params {
      Colors [#ff0000, #00ff00];
      Shape { x 1 + 2; y 4; };
    }
  }
}
"#,
        )
        .expect("parse");
        let ProjectDocument::Sequence(sequence) = doc else {
            panic!("expected sequence")
        };
        assert!(matches!(
            sequence.events[0].params[0].value.kind,
            ConstExprKind::Value(Value::Array(_))
        ));
        assert!(matches!(
            sequence.events[0].params[1].value.kind,
            ConstExprKind::Value(Value::Record(_))
        ));
    }
}
