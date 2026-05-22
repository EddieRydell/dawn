#[derive(Clone, Copy)]
struct Rule {
    kind: SyntaxKind,
    items: &'static [Item],
    first: &'static [SyntaxKind],
    follow: &'static [SyntaxKind],
}

#[derive(Clone, Copy)]
struct Alternative {
    item: Item,
    first: &'static [SyntaxKind],
}

#[derive(Clone, Copy)]
enum Item {
    Token(SyntaxKind),
    Node(usize),
    RepeatRule(usize),
    RepeatTokenSet(usize),
    Choice(&'static [Alternative]),
    TokenSet(usize),
}

pub fn parse_green(tokens: Vec<LexToken>) -> (GreenNode, Vec<Diagnostic>) {
    let mut parser = Parser {
        builder: GreenNodeBuilder::new(),
        diagnostics: Vec::new(),
        tokens,
        cursor: 0,
    };
    parser.parse_rule(ENTRY_RULE);
    while parser.peek().is_some() {
        parser.error_token(DiagnosticKind::UnexpectedToken.message());
    }
    (parser.builder.finish(), parser.diagnostics)
}

struct Parser {
    builder: GreenNodeBuilder<'static>,
    diagnostics: Vec<Diagnostic>,
    tokens: Vec<LexToken>,
    cursor: usize,
}

impl Parser {
    fn parse_rule(&mut self, index: usize) {
        let rule = RULES[index];
        self.builder.start_node(rule.kind.into());
        self.parse_items(rule.items, rule.follow);
        self.builder.finish_node();
    }

    fn parse_items(&mut self, items: &'static [Item], follow: &'static [SyntaxKind]) {
        for item in items {
            self.parse_item(*item, follow);
        }
    }

    fn parse_item(&mut self, item: Item, follow: &'static [SyntaxKind]) {
        match item {
            Item::Token(kind) => {
                self.expect(kind);
            }
            Item::Node(index) => {
                if self.peek().is_some_and(|kind| contains(RULES[index].first, kind)) {
                    self.parse_rule(index);
                } else {
                    self.error_here(DiagnosticKind::UnexpectedEof.message());
                }
            }
            Item::RepeatRule(index) => {
                while self.peek().is_some_and(|kind| contains(RULES[index].first, kind)) {
                    self.parse_rule(index);
                    if self.peek().is_some_and(|kind| contains(follow, kind)) {
                        break;
                    }
                }
            }
            Item::RepeatTokenSet(index) => {
                while self.peek().is_some_and(|kind| contains(token_set(index), kind)) {
                    self.bump();
                    if self.peek().is_some_and(|kind| contains(follow, kind)) {
                        break;
                    }
                }
            }
            Item::Choice(alternatives) => {
                let Some(kind) = self.peek() else {
                    self.error_here(DiagnosticKind::UnexpectedEof.message());
                    return;
                };
                if let Some(alternative) = alternatives.iter().find(|alternative| contains(alternative.first, kind)) {
                    self.parse_item(alternative.item, follow);
                } else {
                    self.error_here(DiagnosticKind::UnexpectedToken.message());
                }
            }
            Item::TokenSet(index) => {
                if self.peek().is_some_and(|kind| contains(token_set(index), kind)) {
                    self.bump();
                } else {
                    self.error_here(DiagnosticKind::UnexpectedToken.message());
                }
            }
        }
    }

    fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.peek() == Some(kind) {
            self.bump();
            true
        } else {
            self.error_here(DiagnosticKind::UnexpectedToken.message());
            false
        }
    }

    fn skip_trivia(&mut self) {
        while self.peek_raw().is_some_and(SyntaxKind::is_trivia) {
            self.bump_raw();
        }
    }

    fn peek(&mut self) -> Option<SyntaxKind> {
        self.skip_trivia();
        self.peek_raw()
    }

    fn peek_raw(&self) -> Option<SyntaxKind> {
        self.tokens.get(self.cursor).map(|token| token.kind)
    }

    fn bump(&mut self) {
        self.skip_trivia();
        self.bump_raw();
    }

    fn bump_raw(&mut self) {
        if let Some(token) = self.tokens.get(self.cursor) {
            self.builder.token(token.kind.into(), &token.text);
            self.cursor += 1;
        }
    }

    fn error_token(&mut self, message: &str) {
        let range = self.tokens[self.cursor].range.clone();
        self.diagnostics.push(Diagnostic::new(
            DiagnosticKind::UnexpectedToken,
            range,
            message,
        ));
        self.builder.start_node(SyntaxKind::Error.into());
        self.bump();
        self.builder.finish_node();
    }

    fn error_here(&mut self, message: &str) {
        let range = self
            .tokens
            .get(self.cursor)
            .map(|token| token.range.clone())
            .unwrap_or(0..0);
        self.diagnostics.push(Diagnostic::new(
            if self.cursor >= self.tokens.len() {
                DiagnosticKind::UnexpectedEof
            } else {
                DiagnosticKind::UnexpectedToken
            },
            range,
            message,
        ));
    }
}

fn contains(tokens: &[SyntaxKind], kind: SyntaxKind) -> bool {
    tokens.contains(&kind)
}
