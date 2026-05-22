# Dawn Syntax Generation Plan

`grammar/dawn/` is the source of truth for Dawn syntax. The syntax generator reads the grammar RON files and emits grammar-derived files under `crates/dawn-syntax/src/generated/`, including rowan language glue, syntax kinds, lexer, parser, syntax diagnostic kinds, and typed AST facade.

The `dawn-syntax` crate shell remains hand-owned: `Cargo.toml`, `src/lib.rs`, the `Parse` wrapper, and diagnostic container type are stable runtime API rather than grammar output.

Generated files are committed for reviewability. They must be refreshed with:

```sh
cargo run -p dawn-syntax-gen -- --write
```

CI and local checks should enforce freshness with:

```sh
cargo run -p dawn-syntax-gen -- --check
```

## Phases

1. Spec loader, cross-reference validation, and generated syntax kind enums.
2. Generated lexer from `tokens.ron`.
3. Generated rowan CST parser for the current top-level grammar.
4. Stable CST corpus goldens.
5. Generated typed AST facade from `ast.ron`.
6. Expression and precedence generation from `precedence.ron`.

## Current Scope

The first generated parser preserves the top-level contract: imports, one document, declared document kinds, path literals, and balanced document bodies. Semantic lowering, validation, type checking, and import resolution remain outside syntax generation.

## Acceptance

- `cargo test -p dawn-syntax-gen`
- `cargo test -p dawn-syntax`
- `cargo run -p dawn-syntax-gen -- --check`
