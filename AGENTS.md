# Repository Guidelines

## Project Structure & Module Organization

This is a mixed Rust and Tauri/React workspace. Rust workspace members are listed in `Cargo.toml`: `crates/dawn-project` contains the Dawn project model and analysis logic, while `apps/desktop/src-tauri` contains the Tauri backend. The desktop frontend lives in `apps/desktop/src`, with workbench infrastructure under `apps/desktop/src/workbench`, panels under `apps/desktop/src/panels`, and shared state in `apps/desktop/src/store`. Example Dawn projects and fixtures are in `examples/`, especially `examples/club-rig`. Generated TypeScript bindings are in `apps/desktop/src/generated/`.

## Build, Test, and Development Commands

- `pnpm install`: install frontend and Tauri CLI dependencies.
- `pnpm dev` or `pnpm desktop`: run the desktop app via `@dawn/desktop`.
- `pnpm --filter @dawn/desktop build`: typecheck with `tsc` and build the Vite frontend.
- `cargo test`: run all Rust workspace tests.
- `pnpm --filter @dawn/desktop bindings`: regenerate `apps/desktop/src/generated/bindings.ts`.
- `pnpm --filter @dawn/desktop bindings:check`: verify generated bindings are current.

## Coding Style & Naming Conventions

Use standard Rust 2021 style and run `cargo fmt` before submitting Rust changes. Rust tests use descriptive snake_case names such as `analyzes_club_rig_to_resolved_project`. TypeScript and React files use two-space indentation, double quotes, semicolons, PascalCase for components, and camelCase for functions and values. Keep frontend additions aligned with the existing workbench, panel, and Zustand store patterns.

## Testing Guidelines

Rust integration tests live under `crates/*/tests`. Do not add or modify tests unless specifically requested. When tests are requested for project analysis, document edits, diagnostics, or model behavior, prefer fixtures from `examples/club-rig` for realistic project flows and use temporary test directories for invalid or synthetic Dawn documents. For frontend changes, run the desktop build at minimum unless asked to add a test harness.

## Commit & Pull Request Guidelines

Recent commit history is informal, so use short, imperative commit messages going forward, for example `Add project diagnostic test` or `Update desktop bindings`. Pull requests should include a concise description, the commands run (`cargo test`, build, bindings check), linked issues when applicable, and screenshots or screen recordings for visible desktop UI changes.

## Agent-Specific Instructions

Do not write tests unless specifically requested. Do not hand-edit generated bindings or Tauri schema files unless explicitly requested; regenerate them with the project scripts. Avoid unrelated edits to lockfiles, IDE files, or generated assets. Check both Rust and desktop manifests before assuming a command or dependency belongs at the workspace root.
