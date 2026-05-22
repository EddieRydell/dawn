pub fn lex(source: &str) -> (Vec<LexToken>, Vec<Diagnostic>) {
    let mut tokens = Vec::new();
    let mut diagnostics = Vec::new();
    let mut cursor = 0;

    while cursor < source.len() {
        let rest = &source[cursor..];
        let Some((rule, len)) = best_match(rest) else {
            let len = rest.chars().next().map(char::len_utf8).unwrap_or(1);
            diagnostics.push(Diagnostic::new(
                DiagnosticKind::InvalidToken,
                cursor..cursor + len,
                DiagnosticKind::InvalidToken.message(),
            ));
            cursor += len;
            continue;
        };

        let start = cursor;
        cursor += len;
        if let Some(kind) = rule.diagnostic {
            diagnostics.push(Diagnostic::new(kind, start..cursor, kind.message()));
        }
        if !rule.skip {
            tokens.push(LexToken {
                kind: rule.kind,
                range: start..cursor,
                text: source[start..cursor].to_string(),
            });
        }
    }

    (tokens, diagnostics)
}

fn best_match(text: &str) -> Option<(Rule, usize)> {
    let regexes = regexes();
    let mut best = None;
    for rule in RULES {
        let len = match rule.matcher {
            Matcher::Regex(index) => regexes[index]
                .find(text)
                .filter(|matched| matched.start() == 0)
                .map(|matched| matched.end()),
            Matcher::Delimited { start, end } => {
                if text.starts_with(start) {
                    Some(
                        text[start.len()..]
                            .find(end)
                            .map(|offset| start.len() + offset + end.len())
                            .unwrap_or(text.len()),
                    )
                } else {
                    None
                }
            }
            Matcher::Literal(literal) => text.starts_with(literal).then_some(literal.len()),
        };

        let Some(len) = len.filter(|len| *len > 0) else {
            continue;
        };
        if best.is_none_or(|(_, best_len)| len > best_len) {
            best = Some((*rule, len));
        }
    }
    best
}

fn regexes() -> &'static [Regex] {
    static REGEXES: OnceLock<Vec<Regex>> = OnceLock::new();
    REGEXES
        .get_or_init(|| {
            REGEX_PATTERNS
                .iter()
                .map(|pattern| Regex::new(pattern).expect("generated regex pattern is valid"))
                .collect()
        })
        .as_slice()
}
