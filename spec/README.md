# Dawn Spec Source

This directory is the planned source of truth for generated Dawn language
tooling. The syntax generator should treat `syntax.ron` as the only editable
syntax definition and emit lexer, parser, rowan CST kinds, and typed AST facade
code from it.

Planned generated outputs:

- `TokenKind` and lexer rules
- `SyntaxKind` and rowan language glue
- event parser rules
- rowan CST sink metadata
- typed AST facade structs/enums and accessors

Regenerate syntax outputs with:

```powershell
cargo run -p syntax-gen -- --write --grammar spec/syntax.ron --out crates/dawn-syntax
```

`builtins.ron` is compiler-owned metadata for builtin Dawn document and record
types. It is intentionally declarative and provisional: semantic validation,
name resolution, type checking, import resolution, and runtime lowering remain
handwritten or future generated work outside this spec directory.

Handwritten code should stay outside this syntax spec and cover semantic work:
validation, name resolution, type checking, import resolution, and runtime
lowering.

Current scope is intentionally the top-level Dawn grammar plus token policy. The
document body is represented as a balanced block until the body grammar is
designed.
