# Repository Guidelines

## Project Structure & Module Organization

This is a Rust workspace. Example Dawn projects and fixtures are in `examples/`.

## Non-Negotiable Architecture Boundaries

The codebase is intentionally split by ownership. Do not collapse these boundaries for convenience. Do not put behavior in the nearest large file just because it is easy to patch. If a change does not fit these boundaries, stop and ask before editing.

### `crates/dawn-language`

Owns the Dawn language and project document model.

Allowed here:
- parsing and loading Dawn files
- authored and resolved document structs
- project analysis and diagnostics
- document edit primitives
- typed sequence/layout/fixture document edit semantics
- render and evaluation logic that is independent of the desktop app
- path/import semantics for Dawn documents
- effect script compiler/runtime

Not allowed here:
- editor tabs or active-file state
- app preferences
- Tauri, desktop dialogs, windows, or events
- preview window lifecycle or native preview transport
- app commands or command DTOs
- async task scheduling for the desktop app

### `crates/dawn-backend`

Owns application/domain state transitions. This crate is the source of truth for what the app state becomes after a user or system action.

Allowed here:
- open/reload project behavior
- workspace file operations as app behavior
- editor session state
- dirty buffer, save, and conflict behavior
- active document selection and GUI document construction
- active document edit transaction handling
- preview state machine
- render/export task planning
- preferences that affect backend/app state
- plain persisted workbench/window preference data that does not require native window APIs
- backend tasks and task outputs
- backend-native app snapshot/view types

Not allowed here:
- Tauri commands, Tauri events, or window APIs
- native file dialogs
- native audio playback implementation
- shared-buffer preview transport
- generated frontend DTO details
- frontend-specific defaults or presentation choices
- direct UI concepts such as menus, shortcuts, panels, or React state
- Tauri/native window operations or window lifecycle mechanics

### `apps/desktop/src`

Owns the Tauri/native shell boundary.

Allowed here:
- Tauri command registration
- converting command DTOs into backend calls
- converting backend views into frontend DTOs
- Tauri event emission
- native file dialogs
- native audio runtime
- preview window lifecycle
- preview transport/shared-buffer plumbing
- background task spawning glue
- window layout integration when it depends on Tauri window APIs

Not allowed here:
- Dawn document semantics
- GUI edit semantics
- project analysis rules
- backend state decisions
- duplicated backend defaults
- fake snapshot state

### `apps/desktop/frontend/src`

Owns presentation and user interaction.

Allowed here:
- React components
- input handling
- command invocation
- local interaction state such as selection, dragging, viewport, and canvas state
- rendering DTOs into UI
- frontend-only affordances such as menus and shortcuts

Not allowed here:
- Dawn document mutation rules beyond constructing typed commands
- project/file semantic decisions
- backend defaults
- duplicated analysis/render logic
- hidden fallbacks for missing backend state

### File-Level Boundary Rules

`apps/desktop/src/commands.rs` is a thin adapter. It may receive a DTO command, perform a native dialog when required, convert DTO/path types, call backend methods, and hand backend updates to job/event glue. It must not implement app behavior, manually coordinate multiple backend subsystems, or contain policy branches that belong in `crates/dawn-backend`.

`apps/desktop/src/dto.rs` can be large, but it must be dumb. It may define TypeScript-facing shapes and `From`/`TryFrom` mappings. It must not invent app state, hardcode live defaults, hardcode preferences, choose fallback preview sources, or perform domain validation beyond DTO shape validation.

`crates/dawn-backend/src/app_backend.rs` is a transaction-script facade/orchestrator. It may coordinate `Project`, `Editor`, `Analysis`, `Preview`, `Render`, and `Preferences`, expose the public backend API to desktop, return `AppUpdate`, and decide which backend modules/tasks participate in a workflow. It is allowed to stay large when it is sequencing backend transactions, but it must not absorb detailed file algorithms, detailed document edit semantics, render/export implementation, DTO mapping, native runtime logic, or become a replacement god file for the old app model.

Rule of thumb:
- If the code answers "what does Dawn mean?", it belongs in `crates/dawn-language`.
- If the code answers "what should the app state become?", it belongs in `crates/dawn-backend`.
- If the code answers "how does desktop/OS/Tauri do this?", it belongs in `apps/desktop/src`.
- If the code answers "how does the user see or manipulate this?", it belongs in `apps/desktop/frontend/src`.

## Testing Guidelines

Rust integration tests live under `crates/*/tests`, and desktop service tests may live beside the service modules. Do not add or modify tests unless specifically requested. 
When tests are requested for project analysis, document edits, diagnostics, or model behavior, prefer fixtures from `examples/thirty-output-controller` for realistic project flows and use temporary test directories for invalid or synthetic Dawn documents.

## Agent-Specific Instructions

Do not write tests unless specifically requested.
Avoid using strings in internal logic. Prefer enums or other structured data.
Do not reintroduce generated web bindings or desktop schema files.
Avoid unrelated edits to lockfiles, IDE files, or generated assets. 
Check both Rust and desktop manifests before assuming a command or dependency belongs at the workspace root. 
Keep crates and modules independent: do not include functionality across crates, do not make sibling modules depend on each other for shared concepts, and factor shared contracts into the appropriate common type/module (types.rs) instead of creating cross-crate or cross-module coupling.
Do not add compatibility layers, shims, fallbacks, or allow for legacy code when adding features or refactoring. 
Do not add fallbacks when something doesn't work. This hides errors and makes debugging harder.
The goal is fast development, not support. Minimize clutter and favor having a single way of doing things. SSOT is your friend.
Do not use git or commands associated with it unless the user specifically requests it.
Do not use .env files to store information.
Do not jump to editing if the conversation is about diagnosing an issue or discussing architecture/design decisions.
Do not start or leave a frontend dev server running when finishing work. The user needs `pnpm tauri dev` to own the frontend port.
When planning, don't hesitate to ask the user relevant questions.
When presenting a plan for approval from the user, list files that will be affected by the plan.
Run `pnpm check` for linting, tests, and other checks after implementing a plan. Fix any regressions.
Do not use .env files or environment variables in this codebase.
