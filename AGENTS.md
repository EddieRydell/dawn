# Repository Guidelines

## Project Structure & Module Organization

This is a Rust workspace. Domain types and canonical automation-clip evaluation live in `crates/dawn-language`; project parsing, import/source ownership, diagnostics, and serialization live in `crates/dawn-project-io`; playback preparation and rendering live in `crates/dawn-runtime`. The desktop service and UI state live under `apps/desktop/src`. Example Dawn projects and fixtures are in `examples/`.

The typed `DawnProject` is authoritative after loading. `SourceProject` records document ownership, imports, original source needed for non-YAML DSL documents, and referenced assets. Saving derives YAML directly from typed state; do not add a synchronization or typed-to-YAML mutation phase.
Project/source metadata and project-owned/relative-path policy live in `crates/dawn-project-io/src/source.rs`. Loading, import resolution, parsing, diagnostics, and serialization live in their descriptive modules under `crates/dawn-project-io/src`; `lib.rs` is the public facade.

Desktop state orchestration is split by workflow under `apps/desktop/src/state`, and typed GUI behavior is split into projection, editing, selection, and model conversion under `apps/desktop/src/gui`. Keep new behavior with the owning workflow instead of growing the module roots.
Mutual Dawn document imports are valid. The loader indexes a document's local objects before following imports; do not reject an in-progress document as a cycle error.

## Testing Guidelines

Rust integration tests live under `crates/*/tests`, and desktop service tests may live beside the service modules. Do not add or modify tests unless specifically requested. 
When tests are requested for project analysis, document edits, diagnostics, or model behavior, prefer fixtures from `examples/thirty-output-controller` for realistic project flows and use temporary test directories for invalid or synthetic Dawn documents.

## Benchmark Guidelines

Effect DSL VM and real-project render benchmarks use Criterion only. Use `pnpm bench:effect-vm:quick` for a fast smoke pass, `pnpm bench:effect-vm:save` before optimization work, `pnpm bench:effect-vm:compare` after optimization work, and `pnpm bench:effect-vm` for the full benchmark set. Focused runs are `cargo bench -p dawn-language --bench effect_vm_bench -- scan_sweep` and `cargo bench -p dawn-runtime --bench render_bench -- render_frame_9504`.

Do not reintroduce custom benchmark CLIs, JSON reporters, legacy aliases, or old render bench flags such as `--project`, `--frames`, `--iterations`, `--warmup`, or `--render-only`. Criterion output lives under `target/criterion` and must not be committed. Timing changes are advisory; checksum and active-effect-count assertion changes are behavior changes unless intentional.

## Agent-Specific Instructions

Do not write tests unless specifically requested.
Never reinvent a pattern or solve a problem that has already been solved. Use dependencies (after asking the user) to solve problems rather than reinventing the wheel.
Avoid using strings in internal logic. Prefer enums or other structured data.
All static color literals must be defined in `apps/desktop/frontend/src/styles.css` as CSS custom properties. TypeScript, JSX, Rust, and tests must reference CSS-backed tokens or receive data-driven colors; do not define palette values elsewhere.
Do not reintroduce generated web bindings or desktop schema files.
Avoid unrelated edits to lockfiles, IDE files, or generated assets. 
Check both Rust and desktop manifests before assuming a command or dependency belongs at the workspace root. 
Do not add compatibility layers, shims, fallbacks, or allow for legacy code when adding features or refactoring. 
Do not add fallbacks when something doesn't work. This hides errors and makes debugging harder.
The goal is fast development, not support. Minimize clutter and favor having a single way of doing things. SSOT is your friend.
Put domain evaluation rules used by both editing and playback in `dawn-language`; desktop projection code must not reimplement runtime semantics.
`DesktopState` owns GUI edit transactionality and history. Loaded and historical project snapshots are immutable `Arc<ProjectSession>` values; a GUI edit makes one deep candidate clone, then shares the accepted snapshot with state, history, save, render-refresh, and clip-raster work. GUI mutation helpers edit the candidate session they receive; do not add another whole-session transactional clone inside them.
Desktop background save/render scheduling and GUI history storage live in `state_tasks.rs`; use its single latest-request scheduler rather than adding another channel scheduler. Read-only fixture/layout geometry projection lives in `gui_geometry.rs`, outside GUI mutation dispatch.
Sequence waveform decoding, cache management, and drawing live in `sequenceWaveform.ts`; keep audio processing out of `SequenceCanvas.tsx`, and pass palette values into the waveform renderer instead of duplicating colors.
Source object kind conversion to desktop `ObjectKind` belongs in the DTO boundary. Do not duplicate that mapping in state or GUI modules.
GUI edits must mutate typed domain state only. Do not construct, inspect, or mutate YAML/text directly from GUI edit code.
GUI edits must not run project checks or reload from YAML after mutation. Persistence belongs to the IO save path.
Do not use git or commands associated with it unless the user specifically requests it.
Do not use .env files to store information.
Do not jump to editing if the conversation is about diagnosing an issue or discussing architecture/design decisions.
Do not start or leave a frontend dev server running when finishing work. The user needs `pnpm tauri dev` to own the frontend port.
When planning, don't hesitate to ask the user relevant questions.
When presenting a plan for approval from the user, list files that will be affected by the plan.
Run `cargo fmt` and then `pnpm check` for linting, tests, and other checks after implementing a plan. Fix any regressions. Running these checks is not necessary after updating docs, example projects, or other things unaffected by tests or checks.
