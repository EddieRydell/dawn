# Dawn Syntax Source

This directory is the planned source of truth for generated Dawn syntax tooling.
The generator should treat files here as the only editable syntax definition and
emit lexer, parser, rowan CST kinds, and typed AST facade code from them.

Planned generated outputs:

- `TokenKind` and lexer rules
- `SyntaxKind` and rowan language glue
- event parser rules
- rowan CST sink metadata
- typed AST facade structs/enums and accessors

Handwritten code should stay outside this syntax spec and cover semantic work:
validation, name resolution, type checking, import resolution, and runtime
lowering.

Current scope is intentionally the top-level Dawn grammar plus token policy. The
document body is represented as a balanced block until the body grammar is
designed.
