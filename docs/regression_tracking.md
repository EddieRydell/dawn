# Regression Tracking

This document tracks how Dawn protects behavior and performance while refactoring. The goal is not
to freeze the code structure. The goal is to make important behavior observable enough that cleanup
does not accidentally change project loading, editing, rendering, output timing, or UI contracts.

## Current Gates

Run the full local gate before merging refactors:

```powershell
pnpm check
```

This currently runs generated binding checks, TypeScript typecheck/lint/knip/build, Rust fmt/check,
Rust clippy, and Rust tests.

Run the performance baseline separately:

```powershell
pnpm baseline:thirty
```

This currently expands to:

```powershell
cargo run --release -p dawn-cli -- baseline examples/thirty-output-controller --json
```

The baseline is intentionally observational. It produces JSON for comparison and CI artifacts, but
does not yet fail on timing thresholds. Local machine load, compiler state, and thermal behavior can
move timing numbers enough that hard assertions would be noisy.

## Thirty Output Release Baseline

Measured on June 7, 2026 on the local Windows development machine with:

```powershell
pnpm baseline:thirty
```

The run used `iterations=30` and `warmup=5`.

### Project And Document

| Metric | Value |
| --- | ---: |
| Analysis p50 | 165.180 ms |
| Analysis p95 | 218.630 ms |
| Reachable files | 16 |
| Objects | 49 |
| Document load p50 | 401.574 ms |
| Document load p95 | 453.311 ms |
| Sequence duration | 200.0 s |
| Sequence frame rate | 144 fps |
| Mark collections | 67 |
| Authored effects | 161 |
| Lanes | 31 |

### Renderer Preparation

| Metric | Value |
| --- | ---: |
| Prepare p50 | 7284.328 ms |
| Prepare p95 | 9378.331 ms |
| Internal prepare total | 7275.466 ms |
| Layout template | 0.208 ms |
| Authored sample | 110.518 ms |
| Generator expansion | 7063.626 ms |
| Timeline index | 39.842 ms |
| Prepared effects | 104,076 |
| Generator parents | 65 |
| Generated children | 103,980 |

Renderer preparation is currently the dominant cost. Generator expansion accounts for nearly all of
the internal preparation time.

### Frame Scenarios

| Scenario | Time | Frame p50 | Frame p95 | Active authored | Active prepared | Visited prepared | Sampled pixels | VM evaluations | Reuse saved |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `idle_start` | 1.0 s | 0.105 ms | 0.122 ms | 1 | 1 | 1 | 3,390 | 113 | 3,277 |
| `intro_build` | 14.5 s | 0.297 ms | 0.310 ms | 0 | 30 | 30 | 3,390 | 113 | 3,277 |
| `drop1_entry` | 41.0 s | 2.047 ms | 3.214 ms | 1 | 181 | 181 | 6,780 | 3,443 | 3,337 |
| `drop1_body` | 66.0 s | 0.692 ms | 0.719 ms | 0 | 60 | 60 | 6,780 | 226 | 6,554 |
| `drop1_tail` | 80.0 s | 1.563 ms | 2.643 ms | 0 | 360 | 360 | 12,240 | 352 | 11,888 |
| `breakdown_dense_marks` | 132.5 s | 0.934 ms | 1.102 ms | 1 | 301 | 301 | 5,790 | 129 | 5,661 |
| `drop2_final` | 180.1 s | 2.235 ms | 2.265 ms | 1 | 181 | 181 | 8,280 | 3,513 | 4,767 |

The heaviest frame scenarios by p50 are `drop2_final`, `drop1_entry`, and `drop1_tail`. These are
good canaries for render-path refactors.

## Regression Classes To Protect

### Project Analysis

Risk: refactors can change import resolution, project-key inference, diagnostics, source ranges,
object indexing, script analysis, or overlay behavior.

Current coverage:

- `pnpm check`
- `crates/dawn-project/tests/analysis.rs`
- `dawn baseline` analysis timing and project shape
- `dawn analyze examples/thirty-output-controller --json`

Useful future checks:

- Store normalized analysis JSON for `examples/thirty-output-controller` and compare reachable
  files, object counts, diagnostics, script IDs, and default object keys.
- Add a CLI comparison mode that reports semantic deltas between two baseline JSON files.
- Keep invalid/synthetic Dawn documents in temporary directories so realistic examples stay clean.

### Document Editing And Serialization

Risk: cleanup in document edit code can reorder effects, drop imports, rewrite too much of a file,
change generated object keys, alter lane repair, or change YAML formatting in ways that affect the
GUI workflow.

Current coverage:

- `crates/dawn-project/tests/analysis.rs` exercises several layout, fixture, and sequence edit
  paths.
- `pnpm check`

Useful future checks:

- Add a CLI command that applies a fixed set of sequence/layout/fixture GUI edits to temporary copies
  of `examples/thirty-output-controller` and emits normalized before/after summaries.
- Include undo/redo-equivalent edit sequences where possible.
- Compare serialized document text for intentional edit scope: which top-level object changed, which
  imports changed, and whether unrelated objects stayed byte-identical.

### App Model Workflows

Risk: `AppModel` dispatch refactors can change autosave timing, preview sync, editor buffer state,
conflict handling, active file/view-mode behavior, status messages, or workbench persistence.

Current coverage:

- A small number of app-core and desktop service tests.
- `pnpm check`

Useful future checks:

- Add a headless app-workflow CLI or test harness that opens `thirty-output-controller`, opens the
  sequence, switches text/gui modes, applies edits, flushes autosave, reloads from disk, and records
  snapshot summaries.
- Assert behavioral state, not UI implementation details: active file, dirty flags, diagnostics,
  active GUI document type, preview source, live-output state, and editor conflict state.
- Keep this harness independent of Tauri windows so it can run in CI without a frontend server.

### Rendering And Output Runtime

Risk: rendering refactors can change output colors, active effect indexing, generated child
expansion, timeline buckets, sample reuse, bytecode preparation, or frame timing.

Current coverage:

- `dawn baseline examples/thirty-output-controller`
- `dawn bench-effect`
- `crates/dawn-app-core/src/output_runtime.rs` tests
- `crates/dawn-project/src/effect_script/tests.rs`

Useful future checks:

- Add stable frame fingerprints for selected scenarios. The fingerprint should summarize fixture
  count, pixel count, topology identity, and a small deterministic color hash. Avoid storing full
  frame dumps unless a bug needs that detail.
- Add baseline comparison that reports changes in active authored effects, active prepared effects,
  visited prepared effects, sampled pixels, VM evaluations, sample reuse, and frame color hash.
- Keep performance thresholds advisory at first, then introduce warning bands only after multiple
  clean release runs establish normal variance.

### Preview And Audio Sync

Risk: preview refactors can reschedule too often, miss frame boundaries, show stale sources, lose
effect-preview filtering, or change native-audio clock behavior.

Current coverage:

- `preview_session` tests for deferred render scheduling and native-audio frame boundaries.
- `app_runtime` test for loading-to-play audio clock behavior.
- Frame scenarios in `dawn baseline` indirectly exercise renderer output.

Useful future checks:

- Add a headless preview session trace: source refresh, play, pause, seek, rewind, effect preview
  selection, native-audio tick, and frame-boundary behavior.
- Emit event counts and final preview state so refactors cannot silently add render churn.

### Live Output

Risk: live-output refactors can rebuild output plans too often, reuse stale plans, change fixture
mapping, or alter frame publication timing.

Current coverage:

- `live_output::tests::output_plan_is_reused_until_analysis_handle_changes`
- `pnpm check`

Useful future checks:

- Add a fixed live-output plan summary for `thirty-output-controller`: controller count, channel
  ranges, fixture mapping, and cached-plan reuse behavior.
- Add a frame-output checksum for one or two active scenarios after the renderer fingerprint exists.

### Frontend Interaction

Risk: frontend refactors can change canvas gestures, selection semantics, path matching, command
wrapping, dirty-state display, terminal behavior, or preview controls without breaking TypeScript.

Current coverage:

- TypeScript typecheck
- ESLint
- Knip
- Production frontend build

Useful future checks:

- Extract pure frontend logic for sequence selection, mark movement, curve editing, path matching,
  and command mapping into small modules with focused tests.
- Add browser-level smoke coverage only after the headless app model harness exists. The first
  Playwright checks should verify loading, opening a project, switching editor modes, and basic
  canvas rendering.

### Generated Bindings And Command Contracts

Risk: command refactors can desynchronize Rust commands and frontend usage, or bypass typed command
wrappers with raw string invocations.

Current coverage:

- `pnpm bindings:check`
- TypeScript compile

Useful future checks:

- Remove raw string command invocations where generated bindings already exist.
- Treat `bindings.ts` as generated only. Do not hand-edit it.
- Add a small check that frontend command wrappers cover all generated commands intentionally, or
  explicitly document commands that bypass the wrapper.

### Persistence And User Config

Risk: workbench-layout refactors can silently drop user state because config loading currently
defaults on missing, unreadable, or invalid JSON.

Current coverage:

- Serde typechecking through Rust compile.

Useful future checks:

- Add focused tests for valid layout persistence, invalid JSON handling, missing config handling,
  and reset behavior.
- Decide whether invalid persisted config should stay silent or emit a diagnostic/status message.
  Whichever behavior is chosen should be documented and tested.

## Refactor Policy

- Keep behavior-preserving refactors separate from behavior changes.
- Prefer one subsystem at a time: analysis, document edits, app model, rendering, preview, frontend.
- Run `pnpm check` for every refactor.
- Run `pnpm baseline:thirty` for refactors touching analysis, document loading, rendering,
  generated effects, output runtime, preview source construction, or sequence documents.
- Compare JSON before and after. Timing movement needs judgment; shape/count/fingerprint movement
  should be treated as a behavior change unless intentionally explained.
- Do not add fallbacks or compatibility shims to hide regressions. Fail clearly when contracts break.
