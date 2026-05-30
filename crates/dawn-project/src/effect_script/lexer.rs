use super::{ScriptDiagnostic, SourcePosition, SourceRange};

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub(super) kind: TokenKind,
    pub(super) range: SourceRange,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum TokenKind {
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
            } else if "{}();,<>+-*/=!&|".contains(character) {
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
