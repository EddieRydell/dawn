#[derive(Clone, Copy)]
struct Rule {
    kind: SyntaxKind,
    items: &'static [Item],
    first: &'static [SyntaxKind],
}

#[derive(Clone, Copy)]
struct Alternative {
    item: Item,
    first: &'static [SyntaxKind],
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum Item {
    Token(SyntaxKind),
    Node(usize),
    RepeatRule { index: usize, stop: &'static [SyntaxKind] },
    RepeatTokenSet { index: usize, stop: &'static [SyntaxKind] },
    Choice(&'static [Alternative]),
    TokenSet(usize),
    Expr(usize),
}

pub fn parse_green(tokens: Vec<LexToken>) -> (GreenNode, Vec<Diagnostic>) {
    let mut parser = Parser {
        builder: GreenNodeBuilder::new(),
        diagnostics: Vec::new(),
        tokens,
        cursor: 0,
    };
    parser.parse_entry_rule();
    (parser.builder.finish(), parser.diagnostics)
}

struct Parser {
    builder: GreenNodeBuilder<'static>,
    diagnostics: Vec<Diagnostic>,
    tokens: Vec<LexToken>,
    cursor: usize,
}

impl Parser {
    fn parse_entry_rule(&mut self) {
        let rule = RULES[ENTRY_RULE];
        self.builder.start_node(rule.kind.into());
        self.parse_items(rule.items);
        while self.peek().is_some() {
            self.error_token(UNEXPECTED_TOKEN_DIAGNOSTIC.message());
        }
        self.builder.finish_node();
    }

    fn parse_rule(&mut self, index: usize) {
        let rule = RULES[index];
        self.builder.start_node(rule.kind.into());
        self.parse_items(rule.items);
        self.builder.finish_node();
    }

    fn parse_items(&mut self, items: &'static [Item]) {
        for item in items {
            self.parse_item(*item);
        }
    }

    fn parse_item(&mut self, item: Item) {
        match item {
            Item::Token(kind) => {
                self.expect(kind);
            }
            Item::Node(index) => {
                if self.peek().is_some_and(|kind| contains(RULES[index].first, kind)) {
                    self.parse_rule(index);
                } else {
                    self.error_here(UNEXPECTED_EOF_DIAGNOSTIC.message());
                }
            }
            Item::RepeatRule { index, stop } => {
                while self.peek().is_some_and(|kind| contains(RULES[index].first, kind)) {
                    self.parse_rule(index);
                    if self.peek().is_some_and(|kind| contains(stop, kind)) {
                        break;
                    }
                }
            }
            Item::RepeatTokenSet { index, stop } => {
                while self.peek().is_some_and(|kind| contains(token_set(index), kind)) {
                    self.bump();
                    if self.peek().is_some_and(|kind| contains(stop, kind)) {
                        break;
                    }
                }
            }
            Item::Choice(alternatives) => {
                let Some(kind) = self.peek() else {
                    self.error_here(UNEXPECTED_EOF_DIAGNOSTIC.message());
                    return;
                };
                if let Some(alternative) = alternatives.iter().find(|alternative| contains(alternative.first, kind)) {
                    self.parse_item(alternative.item);
                } else {
                    self.error_here(UNEXPECTED_TOKEN_DIAGNOSTIC.message());
                }
            }
            Item::TokenSet(index) => {
                if self.peek().is_some_and(|kind| contains(token_set(index), kind)) {
                    self.bump();
                } else {
                    self.error_here(UNEXPECTED_TOKEN_DIAGNOSTIC.message());
                }
            }
            Item::Expr(index) => self.parse_expr(index, 0),
        }
    }

    fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.peek() == Some(kind) {
            self.bump();
            true
        } else {
            self.error_here(UNEXPECTED_TOKEN_DIAGNOSTIC.message());
            false
        }
    }

    fn parse_expr(&mut self, expr_index: usize, min_bp: u8) {
        let expr = EXPRESSIONS[expr_index];
        self.builder.start_node(expr.root.into());

        if let Some(prefix) = expr.prefix.iter().find(|prefix| self.peek() == Some(prefix.token)) {
            self.builder.start_node(prefix.node.into());
            self.bump();
            self.parse_expr(expr_index, PREFIX_BINDING_POWER);
            self.builder.finish_node();
        } else if let Some(atom) = expr.atoms.iter().find(|atom| self.peek().is_some_and(|kind| contains(atom.first, kind))) {
            self.parse_item(atom.item);
        } else {
            self.error_here(UNEXPECTED_TOKEN_DIAGNOSTIC.message());
            self.builder.finish_node();
            return;
        }

        loop {
            if let Some(postfix) = expr.postfix.iter().find(|postfix| self.peek().is_some_and(|kind| contains(postfix.first, kind))) {
                self.builder.start_node(postfix.node.into());
                self.parse_items(postfix.items);
                self.builder.finish_node();
                continue;
            }
            let Some(op) = expr.infix.iter().find(|op| self.peek() == Some(op.token) && op.left_bp >= min_bp) else {
                break;
            };
            self.builder.start_node(op.node.into());
            self.bump();
            self.parse_expr(expr_index, op.right_bp);
            self.builder.finish_node();
        }

        self.builder.finish_node();
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
            UNEXPECTED_TOKEN_DIAGNOSTIC,
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
                UNEXPECTED_EOF_DIAGNOSTIC
            } else {
                UNEXPECTED_TOKEN_DIAGNOSTIC
            },
            range,
            message,
        ));
    }
}

fn contains(tokens: &[SyntaxKind], kind: SyntaxKind) -> bool {
    tokens.contains(&kind)
}
