# Repository Guidelines

## Project Structure & Module Organization

This is a Rust workspace. The desktop service and UI state live under `apps/desktop/src`. Example Dawn projects and fixtures are in `examples/`.

## Testing Guidelines

Rust integration tests live under `crates/*/tests`, and desktop service tests may live beside the service modules. Do not add or modify tests unless specifically requested. 
When tests are requested for project analysis, document edits, diagnostics, or model behavior, prefer fixtures from `examples/thirty-ouptut-controller` for realistic project flows and use temporary test directories for invalid or synthetic Dawn documents.

## Agent-Specific Instructions

Do not write tests unless specifically requested.
Avoid using strings in internal logic. Prefer enums or other structured data.
Do not reintroduce generated web bindings or desktop schema files.
Avoid unrelated edits to lockfiles, IDE files, or generated assets. 
Check both Rust and desktop manifests before assuming a command or dependency belongs at the workspace root. 
Do not add compatibility layers, shims, fallbacks, or allow for legacy code when adding features or refactoring. 
Do not add fallbacks when something doesn't work. This hides errors and makes debugging harder.
The goal is fast development, not support. Minimize clutter and favor having a single way of doing things. SSOT is your friend.
Do not use git or commands associated with it unless the user specifically requests it.
Do not use .env files to store information.
Do not jump to editing if the conversation is about diagnosing an issue or discussing architecture/design decisions.
Do not start or leave a frontend dev server running when finishing work. The user needs `pnpm tauri dev` to own the frontend port.
When planning, don't hesitate to ask the user relevant questions.
When presenting a plan for approval from the user, list files that will be affected by the plan.
Run `pnpm check` for linting, tests, and other checks after implementing a plan.
