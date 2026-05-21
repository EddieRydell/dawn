use super::ast::Span;
use super::error::CompileError;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Float(f64),
    Int(i32),
    String(String),
    ColorHex(u8, u8, u8),
    True,
    False,

    // Identifiers & keywords
    Ident(String),
    Let,
    Fn,
    If,
    Else,
    Param,
    Enum,
    Flags,
    Switch,
    Case,
    Default,

    // Type names
    FloatTy,
    IntTy,
    BoolTy,
    ColorTy,
    GradientTy,
    CurveTy,
    PathTy,

    // Punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Dot,
    Colon,
    Arrow,     // ->
    At,        // @

    // Operators
    Plus,
    Minus,
    Star,
    StarStar,  // **
    Slash,
    Percent,
    Lt,
    Gt,
    Le,        // <=
    Ge,        // >=
    EqEq,      // ==
    Ne,        // !=
    And,       // &&
    Or,        // ||
    Bang,      // !
    Pipe,      // |
    Ampersand, // &
    Caret,     // ^
    Shl,       // <<
    Shr,       // >>
    Eq,        // =
    Question,  // ?
    FatArrow,  // =>

    // Special
    Semicolon,
    Eof,
}

#[derive(Debug, Clone)]
pub struct SpannedToken {
    pub token: Token,
    pub span: Span,
}

pub fn lex(source: &str) -> Result<Vec<SpannedToken>, Vec<CompileError>> {
    let mut lexer = Lexer::new(source);
    lexer.tokenize()
}

struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    pos: usize,
    tokens: Vec<SpannedToken>,
    errors: Vec<CompileError>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            tokens: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn tokenize(&mut self) -> Result<Vec<SpannedToken>, Vec<CompileError>> {
        while self.pos < self.bytes.len() {
            self.skip_whitespace_and_comments();
            if self.pos >= self.bytes.len() {
                break;
            }

            let start = self.pos;
            let ch = self.bytes[self.pos];

            match ch {
                b';' => { self.pos += 1; self.push(Token::Semicolon, start, self.pos); }
                b'(' => { self.pos += 1; self.push(Token::LParen, start, self.pos); }
                b')' => { self.pos += 1; self.push(Token::RParen, start, self.pos); }
                b'{' => { self.pos += 1; self.push(Token::LBrace, start, self.pos); }
                b'}' => { self.pos += 1; self.push(Token::RBrace, start, self.pos); }
                b',' => { self.pos += 1; self.push(Token::Comma, start, self.pos); }
                b'.' => { self.pos += 1; self.push(Token::Dot, start, self.pos); }
                b':' => { self.pos += 1; self.push(Token::Colon, start, self.pos); }
                b'@' => { self.pos += 1; self.push(Token::At, start, self.pos); }
                b'+' => { self.pos += 1; self.push(Token::Plus, start, self.pos); }
                b'*' => {
                    self.pos += 1;
                    if self.peek() == Some(b'*') {
                        self.pos += 1;
                        self.push(Token::StarStar, start, self.pos);
                    } else {
                        self.push(Token::Star, start, self.pos);
                    }
                }
                b'/' => { self.pos += 1; self.push(Token::Slash, start, self.pos); }
                b'%' => { self.pos += 1; self.push(Token::Percent, start, self.pos); }
                b'|' => {
                    self.pos += 1;
                    if self.peek() == Some(b'|') {
                        self.pos += 1;
                        self.push(Token::Or, start, self.pos);
                    } else {
                        self.push(Token::Pipe, start, self.pos);
                    }
                }
                b'&' => {
                    self.pos += 1;
                    if self.peek() == Some(b'&') {
                        self.pos += 1;
                        self.push(Token::And, start, self.pos);
                    } else {
                        self.push(Token::Ampersand, start, self.pos);
                    }
                }
                b'^' => { self.pos += 1; self.push(Token::Caret, start, self.pos); }
                b'-' => {
                    self.pos += 1;
                    if self.peek() == Some(b'>') {
                        self.pos += 1;
                        self.push(Token::Arrow, start, self.pos);
                    } else {
                        self.push(Token::Minus, start, self.pos);
                    }
                }
                b'<' => {
                    self.pos += 1;
                    if self.peek() == Some(b'=') {
                        self.pos += 1;
                        self.push(Token::Le, start, self.pos);
                    } else if self.peek() == Some(b'<') {
                        self.pos += 1;
                        self.push(Token::Shl, start, self.pos);
                    } else {
                        self.push(Token::Lt, start, self.pos);
                    }
                }
                b'>' => {
                    self.pos += 1;
                    if self.peek() == Some(b'=') {
                        self.pos += 1;
                        self.push(Token::Ge, start, self.pos);
                    } else if self.peek() == Some(b'>') {
                        self.pos += 1;
                        self.push(Token::Shr, start, self.pos);
                    } else {
                        self.push(Token::Gt, start, self.pos);
                    }
                }
                b'=' => {
                    self.pos += 1;
                    if self.peek() == Some(b'=') {
                        self.pos += 1;
                        self.push(Token::EqEq, start, self.pos);
                    } else if self.peek() == Some(b'>') {
                        self.pos += 1;
                        self.push(Token::FatArrow, start, self.pos);
                    } else {
                        self.push(Token::Eq, start, self.pos);
                    }
                }
                b'!' => {
                    self.pos += 1;
                    if self.peek() == Some(b'=') {
                        self.pos += 1;
                        self.push(Token::Ne, start, self.pos);
                    } else {
                        self.push(Token::Bang, start, self.pos);
                    }
                }
                b'?' => { self.pos += 1; self.push(Token::Question, start, self.pos); }
                b'#' => {
                    self.pos += 1;
                    self.lex_color_hex(start);
                }
                b'"' => {
                    self.pos += 1;
                    self.lex_string(start);
                }
                b'0'..=b'9' => {
                    self.lex_number(start);
                }
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                    self.lex_ident(start);
                }
                _ => {
                    self.errors.push(CompileError::lexer(
                        format!("Unexpected character: '{}'", ch as char),
                        Span::new(start, start + 1),
                    ));
                    self.pos += 1;
                }
            }
        }

        self.tokens.push(SpannedToken {
            token: Token::Eof,
            span: Span::new(self.pos, self.pos),
        });

        if self.errors.is_empty() {
            Ok(std::mem::take(&mut self.tokens))
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn push(&mut self, token: Token, start: usize, end: usize) {
        self.tokens.push(SpannedToken {
            token,
            span: Span::new(start, end),
        });
    }

    fn skip_whitespace_and_comments(&mut self) {
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                b'/' if self.bytes.get(self.pos + 1) == Some(&b'/') => {
                    // Line comment (// style): skip to end of line
                    while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
    }

    fn lex_color_hex(&mut self, start: usize) {
        let hex_start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_hexdigit() {
            self.pos += 1;
        }
        let hex = &self.source[hex_start..self.pos];
        match hex.len() {
            3 => {
                // Short form: #rgb -> #rrggbb
                // hex chars pre-validated by lexer (only ascii hex digits reach here)
                let r = u8::from_str_radix(&hex[0..1], 16).unwrap_or(0);
                let g = u8::from_str_radix(&hex[1..2], 16).unwrap_or(0);
                let b = u8::from_str_radix(&hex[2..3], 16).unwrap_or(0);
                self.push(Token::ColorHex(r * 17, g * 17, b * 17), start, self.pos);
            }
            6 => {
                // hex chars pre-validated by lexer (only ascii hex digits reach here)
                let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
                let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
                let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
                self.push(Token::ColorHex(r, g, b), start, self.pos);
            }
            _ => {
                self.errors.push(CompileError::lexer(
                    format!("Invalid color hex: expected 3 or 6 hex digits, got {}", hex.len()),
                    Span::new(start, self.pos),
                ));
            }
        }
    }

    fn lex_string(&mut self, start: usize) {
        let str_start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos] != b'"' && self.bytes[self.pos] != b'\n' {
            self.pos += 1;
        }
        let s = self.source[str_start..self.pos].to_string();
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'"' {
            self.pos += 1;
        } else {
            self.errors.push(CompileError::lexer(
                "Unterminated string literal",
                Span::new(start, self.pos),
            ));
        }
        self.push(Token::String(s), start, self.pos);
    }

    fn lex_number(&mut self, start: usize) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        // Check for decimal point
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'.'
            && self.bytes.get(self.pos + 1).is_some_and(u8::is_ascii_digit)
        {
            self.pos += 1; // skip '.'
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            let text = &self.source[start..self.pos];
            match text.parse::<f64>() {
                Ok(v) => self.push(Token::Float(v), start, self.pos),
                Err(_) => self.errors.push(CompileError::lexer(
                    format!("Invalid float: {text}"),
                    Span::new(start, self.pos),
                )),
            }
        } else {
            let text = &self.source[start..self.pos];
            match text.parse::<i32>() {
                Ok(v) => self.push(Token::Int(v), start, self.pos),
                Err(_) => {
                    // Try as float (it might just be a large number)
                    match text.parse::<f64>() {
                        Ok(v) => self.push(Token::Float(v), start, self.pos),
                        Err(_) => self.errors.push(CompileError::lexer(
                            format!("Invalid number: {text}"),
                            Span::new(start, self.pos),
                        )),
                    }
                }
            }
        }
    }

    fn lex_ident(&mut self, start: usize) {
        while self.pos < self.bytes.len()
            && (self.bytes[self.pos].is_ascii_alphanumeric() || self.bytes[self.pos] == b'_')
        {
            self.pos += 1;
        }
        let word = &self.source[start..self.pos];
        let token = match word {
            "let" => Token::Let,
            "fn" => Token::Fn,
            "if" => Token::If,
            "else" => Token::Else,
            "param" => Token::Param,
            "enum" => Token::Enum,
            "flags" => Token::Flags,
            "true" => Token::True,
            "false" => Token::False,
            "switch" => Token::Switch,
            "case" => Token::Case,
            "default" => Token::Default,
            "float" => Token::FloatTy,
            "int" => Token::IntTy,
            "bool" => Token::BoolTy,
            "color" => Token::ColorTy,
            "gradient" => Token::GradientTy,
            "curve" => Token::CurveTy,
            "path" => Token::PathTy,
            _ => Token::Ident(word.to_string()),
        };
        self.push(token, start, self.pos);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn tok(s: &str) -> Vec<Token> {
        lex(s).unwrap().into_iter().map(|t| t.token).collect()
    }

    #[test]
    fn simple_tokens() {
        let tokens = tok("1 + 2.0");
        assert_eq!(tokens, vec![Token::Int(1), Token::Plus, Token::Float(2.0), Token::Eof]);
    }

    #[test]
    fn keywords() {
        let tokens = tok("let fn if else param enum flags");
        assert_eq!(tokens, vec![
            Token::Let, Token::Fn, Token::If, Token::Else,
            Token::Param, Token::Enum, Token::Flags, Token::Eof,
        ]);
    }

    #[test]
    fn type_keywords() {
        let tokens = tok("float int bool color gradient curve path");
        assert_eq!(tokens, vec![
            Token::FloatTy, Token::IntTy, Token::BoolTy, Token::ColorTy,
            Token::GradientTy, Token::CurveTy, Token::PathTy, Token::Eof,
        ]);
    }

    #[test]
    fn vec2_is_ident() {
        let tokens = tok("vec2(1.0, 2.0)");
        assert_eq!(tokens, vec![
            Token::Ident("vec2".into()), Token::LParen,
            Token::Float(1.0), Token::Comma, Token::Float(2.0),
            Token::RParen, Token::Eof,
        ]);
    }

    #[test]
    fn operators() {
        let tokens = tok("+ - * / % < > <= >= == != && || !");
        assert_eq!(tokens, vec![
            Token::Plus, Token::Minus, Token::Star, Token::Slash, Token::Percent,
            Token::Lt, Token::Gt, Token::Le, Token::Ge, Token::EqEq, Token::Ne,
            Token::And, Token::Or, Token::Bang, Token::Eof,
        ]);
    }

    #[test]
    fn arrow_and_pipe() {
        let tokens = tok("-> | ||");
        assert_eq!(tokens, vec![Token::Arrow, Token::Pipe, Token::Or, Token::Eof]);
    }

    #[test]
    fn color_hex() {
        let tokens = tok("#ff0000 #abc");
        assert_eq!(tokens, vec![
            Token::ColorHex(255, 0, 0),
            Token::ColorHex(170, 187, 204),
            Token::Eof,
        ]);
    }

    #[test]
    fn string_literal() {
        let tokens = tok("@name \"Fire Flicker\"");
        assert_eq!(tokens, vec![
            Token::At, Token::Ident("name".into()), Token::String("Fire Flicker".into()), Token::Eof,
        ]);
    }

    #[test]
    fn semicolons_as_terminators() {
        let tokens = tok("let x = 1; let y = 2;");
        assert_eq!(tokens, vec![
            Token::Let, Token::Ident("x".into()), Token::Eq, Token::Int(1),
            Token::Semicolon,
            Token::Let, Token::Ident("y".into()), Token::Eq, Token::Int(2),
            Token::Semicolon,
            Token::Eof,
        ]);
    }

    #[test]
    fn newlines_are_whitespace() {
        // Newlines are ignored — no Semicolon tokens produced
        let tokens = tok("x +\ny");
        assert_eq!(tokens, vec![
            Token::Ident("x".into()), Token::Plus, Token::Ident("y".into()), Token::Eof,
        ]);
    }

    #[test]
    fn multiline_expression() {
        // Multi-line expressions just work (newlines are whitespace)
        let tokens = tok("rgb(1.0,\n0.0,\n0.0)");
        assert_eq!(tokens, vec![
            Token::Ident("rgb".into()), Token::LParen,
            Token::Float(1.0), Token::Comma,
            Token::Float(0.0), Token::Comma,
            Token::Float(0.0), Token::RParen,
            Token::Eof,
        ]);
    }

    #[test]
    fn comments_stripped() {
        let tokens = tok("x + y // this is a comment\nz");
        assert_eq!(tokens, vec![
            Token::Ident("x".into()), Token::Plus, Token::Ident("y".into()),
            Token::Ident("z".into()), Token::Eof,
        ]);
    }

    #[test]
    fn booleans() {
        let tokens = tok("true false");
        assert_eq!(tokens, vec![Token::True, Token::False, Token::Eof]);
    }

    #[test]
    fn complex_expression() {
        let tokens = tok("smoothstep(center - width, center, x) * 0.5");
        assert_eq!(tokens, vec![
            Token::Ident("smoothstep".into()),
            Token::LParen,
            Token::Ident("center".into()), Token::Minus, Token::Ident("width".into()),
            Token::Comma,
            Token::Ident("center".into()),
            Token::Comma,
            Token::Ident("x".into()),
            Token::RParen,
            Token::Star,
            Token::Float(0.5),
            Token::Eof,
        ]);
    }

    #[test]
    fn braces_with_newlines() {
        let tokens = tok("{\nx\n}");
        assert_eq!(tokens, vec![
            Token::LBrace,
            Token::Ident("x".into()),
            Token::RBrace,
            Token::Eof,
        ]);
    }

    #[test]
    fn method_call_across_lines() {
        // Method call on next line works because newlines are whitespace
        let tokens = tok("mycolor\n.scale(0.5)");
        assert_eq!(tokens, vec![
            Token::Ident("mycolor".into()), Token::Dot,
            Token::Ident("scale".into()), Token::LParen,
            Token::Float(0.5), Token::RParen, Token::Eof,
        ]);
    }

    #[test]
    fn star_star_token() {
        let tokens = tok("2.0 ** 3.0");
        assert_eq!(tokens, vec![
            Token::Float(2.0), Token::StarStar, Token::Float(3.0), Token::Eof,
        ]);
    }

    #[test]
    fn question_token() {
        let tokens = tok("x ? y : z");
        assert_eq!(tokens, vec![
            Token::Ident("x".into()), Token::Question,
            Token::Ident("y".into()), Token::Colon,
            Token::Ident("z".into()), Token::Eof,
        ]);
    }

    #[test]
    fn fat_arrow_token() {
        let tokens = tok("case A => x");
        assert_eq!(tokens, vec![
            Token::Case, Token::Ident("A".into()), Token::FatArrow,
            Token::Ident("x".into()), Token::Eof,
        ]);
    }

    #[test]
    fn switch_case_default_keywords() {
        let tokens = tok("switch case default");
        assert_eq!(tokens, vec![
            Token::Switch, Token::Case, Token::Default, Token::Eof,
        ]);
    }

    #[test]
    fn star_star_vs_star() {
        let tokens = tok("a * b ** c");
        assert_eq!(tokens, vec![
            Token::Ident("a".into()), Token::Star, Token::Ident("b".into()),
            Token::StarStar, Token::Ident("c".into()), Token::Eof,
        ]);
    }

    #[test]
    fn bitwise_operators() {
        let tokens = tok("& ^ << >>");
        assert_eq!(tokens, vec![
            Token::Ampersand, Token::Caret, Token::Shl, Token::Shr, Token::Eof,
        ]);
    }

    #[test]
    fn shift_vs_comparison() {
        let tokens = tok("a << b <= c >> d >= e");
        assert_eq!(tokens, vec![
            Token::Ident("a".into()), Token::Shl, Token::Ident("b".into()),
            Token::Le, Token::Ident("c".into()), Token::Shr,
            Token::Ident("d".into()), Token::Ge, Token::Ident("e".into()),
            Token::Eof,
        ]);
    }

    #[test]
    fn ampersand_vs_and() {
        let tokens = tok("a & b && c");
        assert_eq!(tokens, vec![
            Token::Ident("a".into()), Token::Ampersand, Token::Ident("b".into()),
            Token::And, Token::Ident("c".into()), Token::Eof,
        ]);
    }

    #[test]
    fn fat_arrow_vs_eq() {
        let tokens = tok("a = b == c => d");
        assert_eq!(tokens, vec![
            Token::Ident("a".into()), Token::Eq, Token::Ident("b".into()),
            Token::EqEq, Token::Ident("c".into()), Token::FatArrow,
            Token::Ident("d".into()), Token::Eof,
        ]);
    }

    #[test]
    fn semicolon_token() {
        let tokens = tok("let x = 1;");
        assert_eq!(tokens, vec![
            Token::Let, Token::Ident("x".into()), Token::Eq, Token::Int(1),
            Token::Semicolon, Token::Eof,
        ]);
    }

    #[test]
    fn extra_semicolons_tolerated() {
        let tokens = tok(";;; let x = 1;;;");
        assert_eq!(tokens, vec![
            Token::Semicolon, Token::Semicolon, Token::Semicolon,
            Token::Let, Token::Ident("x".into()), Token::Eq, Token::Int(1),
            Token::Semicolon, Token::Semicolon, Token::Semicolon,
            Token::Eof,
        ]);
    }
}
