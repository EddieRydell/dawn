use crate::diagnostic::{Diagnostic, DiagnosticKind};
use crate::generated::kind::SyntaxKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexToken {
    pub kind: SyntaxKind,
    pub range: std::ops::Range<usize>,
    pub text: String,
}

pub fn lex(source: &str) -> (Vec<LexToken>, Vec<Diagnostic>) {
    let mut tokens = Vec::new();
    let mut diagnostics = Vec::new();
    let mut cursor = 0;

    while cursor < source.len() {
        let rest = &source[cursor..];
        let start = cursor;
        let Some(ch) = rest.chars().next() else {
            break;
        };

        if ch.is_ascii_whitespace() {
            cursor += ch.len_utf8();
            while cursor < source.len() {
                let Some(next) = source[cursor..].chars().next() else {
                    break;
                };
                if !next.is_ascii_whitespace() {
                    break;
                }
                cursor += next.len_utf8();
            }
            tokens.push(LexToken {
                kind: SyntaxKind::Whitespace,
                range: start..cursor,
                text: source[start..cursor].to_string(),
            });
            continue;
        }

        if rest.starts_with("//") {
            cursor += rest.find('\n').unwrap_or(rest.len());
            tokens.push(LexToken {
                kind: SyntaxKind::LineComment,
                range: start..cursor,
                text: source[start..cursor].to_string(),
            });
            continue;
        }

        if rest.starts_with("/*") {
            if let Some(end) = rest.find("*/") {
                cursor += end + 2;
            } else {
                cursor = source.len();
                diagnostics.push(Diagnostic::new(
                    DiagnosticKind::UnterminatedBlockComment,
                    start..cursor,
                    "unterminated block comment",
                ));
            }
            tokens.push(LexToken {
                kind: SyntaxKind::BlockComment,
                range: start..cursor,
                text: source[start..cursor].to_string(),
            });
            continue;
        }

        if rest.starts_with('"') {
            cursor += 1;
            while cursor < source.len() && !source[cursor..].starts_with('"') {
                cursor += source[cursor..].chars().next().unwrap().len_utf8();
            }
            if cursor < source.len() {
                cursor += 1;
            }
            tokens.push(LexToken {
                kind: SyntaxKind::String,
                range: start..cursor,
                text: source[start..cursor].to_string(),
            });
            continue;
        }

        if rest.starts_with('#') {
            cursor += 1;
            while cursor < source.len() {
                let Some(next) = source[cursor..].chars().next() else {
                    break;
                };
                if !next.is_ascii_alphanumeric() {
                    break;
                }
                cursor += next.len_utf8();
            }
            let text = &source[start..cursor];
            let valid_len = text.len() == 4 || text.len() == 7;
            let valid_hex = text[1..].chars().all(|ch| ch.is_ascii_hexdigit());
            let kind = if valid_len && valid_hex {
                SyntaxKind::Color
            } else {
                diagnostics.push(Diagnostic::new(
                    DiagnosticKind::InvalidColor,
                    start..cursor,
                    "invalid color literal",
                ));
                SyntaxKind::InvalidColor
            };
            tokens.push(LexToken { kind, range: start..cursor, text: source[start..cursor].to_string() });
            continue;
        }

        if ch.is_ascii_digit() || (ch == '.' && rest[1..].chars().next().is_some_and(|next| next.is_ascii_digit())) {
            cursor += ch.len_utf8();
            while cursor < source.len() {
                let Some(next) = source[cursor..].chars().next() else {
                    break;
                };
                if !(next.is_ascii_digit() || next == '_') {
                    break;
                }
                cursor += next.len_utf8();
            }

            let mut kind = SyntaxKind::Int;
            if cursor < source.len() && source[cursor..].starts_with('.') {
                kind = SyntaxKind::Float;
                cursor += 1;
                while cursor < source.len() {
                    let Some(next) = source[cursor..].chars().next() else {
                        break;
                    };
                    if !(next.is_ascii_digit() || next == '_') {
                        break;
                    }
                    cursor += next.len_utf8();
                }
            }

            if cursor < source.len() {
                let next = source[cursor..].chars().next().unwrap();
                if next == 'e' || next == 'E' {
                    kind = SyntaxKind::Float;
                    cursor += 1;
                    if cursor < source.len() {
                        let sign = source[cursor..].chars().next().unwrap();
                        if sign == '+' || sign == '-' {
                            cursor += sign.len_utf8();
                        }
                    }
                    let digits_start = cursor;
                    while cursor < source.len() {
                        let Some(next) = source[cursor..].chars().next() else {
                            break;
                        };
                        if !(next.is_ascii_digit() || next == '_') {
                            break;
                        }
                        cursor += next.len_utf8();
                    }
                    if digits_start == cursor {
                        diagnostics.push(Diagnostic::new(
                            DiagnosticKind::InvalidToken,
                            start..cursor,
                            "invalid exponent",
                        ));
                    }
                }
            }

            if kind == SyntaxKind::Int && source[start..cursor].replace('_', "").parse::<i64>().is_err() {
                diagnostics.push(Diagnostic::new(
                    DiagnosticKind::InvalidInt,
                    start..cursor,
                    "invalid integer literal",
                ));
            }
            tokens.push(LexToken { kind, range: start..cursor, text: source[start..cursor].to_string() });
            continue;
        }

        if is_ident_start(ch) {
            cursor += ch.len_utf8();
            while cursor < source.len() {
                let Some(next) = source[cursor..].chars().next() else {
                    break;
                };
                if !is_ident_continue(next) {
                    break;
                }
                cursor += next.len_utf8();
            }
            tokens.push(LexToken {
                kind: keyword_kind(&source[start..cursor]).unwrap_or(SyntaxKind::Ident),
                range: start..cursor,
                text: source[start..cursor].to_string(),
            });
            continue;
        }

        if let Some((kind, len)) = punctuation_kind(rest) {
            cursor += len;
            tokens.push(LexToken { kind, range: start..cursor, text: source[start..cursor].to_string() });
            continue;
        }

        cursor += ch.len_utf8();
        diagnostics.push(Diagnostic::new(
            DiagnosticKind::InvalidToken,
            start..cursor,
            "invalid token",
        ));
    }

    (tokens, diagnostics)
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_continue(ch: char) -> bool {
    is_ident_start(ch) || ch.is_ascii_digit()
}

fn keyword_kind(text: &str) -> Option<SyntaxKind> {
    Some(match text {
/*KEYWORD_MATCH*/        _ => return None,
    })
}

fn punctuation_kind(text: &str) -> Option<(SyntaxKind, usize)> {
    for (punctuation, result) in [
/*PUNCTUATION_MATCH*/    ] {
        if text.starts_with(punctuation) {
            return result;
        }
    }
    None
}
