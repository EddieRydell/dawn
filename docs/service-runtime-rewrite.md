# Dawn Service Runtime Rewrite

This document tracks the cutover contract for `dawn-app-runtime`. The existing
desktop `AppModel` remains available while the runtime is built and verified.
The old path is deleted only after parity is demonstrated by focused runtime
tests and `pnpm check`.

## Current State

The new `dawn-app-runtime` crate exists and compiles in the workspace. It
defines the first-pass contracts, service cores, read-model composition,
bounded-channel runner surface, coordinator ownership, opt-in file logging, and
focused service-core/coordinator tests.

Desktop behavior has moved the editor/project lifecycle cutover through the
runtime `DocumentStore`. Startup restore, explicit project and file opening,
create-file auto-open, active-tab changes, close file, view-mode changes,
active text edits, and text undo/redo submit runtime commands before
`dawn-app-core::app_model::AppModel` mirrors the approved state for the
runtime-state frontend contract.

The frontend no longer subscribes to `app_snapshot_changed` or calls
`get_snapshot`. It hydrates from `get_runtime_state`, listens for
`runtime_state_changed`, stores `runtimeState`, and consumes generated
`RuntimeStateDto` bindings. The DTO currently preserves the previous wire shape
while backend service ownership continues to move out of `AppModel`.

## Next Decision

Choose the next runtime ownership slice outside editor lifecycle. GUI edits,
autosave persistence, file watcher reconciliation, analysis/preview runtime
parity, mutating-command acknowledgements, focused frontend slices, and final
`AppModel` deletion remain on the existing compatibility path.

## Milestone Tracker

### Phase 1: Runtime Contracts

- [x] Add `crates/dawn-app-runtime` to the Rust workspace.
- [x] Add shared `Revision`, `RequestId`, service names, typed runtime errors,
  command acknowledgements, event envelopes, task records, and self-write tags.
- [x] Add focused read-model DTOs for workspace, editor, diagnostics, preview,
  transport/audio, live output, status/tasks, and prefs/window layout.
- [x] Add opt-in file logging through `tracing`/`tracing-subscriber`.
- [x] Add initial architecture and cutover tracking doc.
- [x] Keep old desktop runtime path untouched.

Affected files:

- `Cargo.toml`
- `Cargo.lock`
- `crates/dawn-app-runtime/**`
- `docs/service-runtime-rewrite.md`

### Phase 2: Service Cores

- [x] Add deterministic `DocumentStore` core for project root, buffers, active
  file, view mode, dirty state, stale revision rejection, and conflict marking.
- [x] Add deterministic `ProjectIndex` core for in-memory project analysis from
  overlays.
- [x] Add deterministic `SequenceEdit` core for path/object/revision-validated
  sequence document edits.
- [x] Add deterministic `PreviewEngine` core for latest-only render requests and
  stale frame suppression.
- [x] Add deterministic `AudioEngine` core for passive readiness updates.
- [x] Add deterministic `Autosave` core for self-write tagging.
- [x] Add deterministic `FileWatcher` core for self-write suppression and
  external conflict events.
- [x] Add deterministic `LiveOutput` core that consumes preview frame events.
- [x] Add deterministic `LayoutPrefs` core for project tree and preview-window
  preference state.
- [x] Add deterministic `ReadModel` core that composes service events into
  focused slices.
- [ ] Replace placeholder/file-handle command payloads with app-owned ports for
  filesystem, analysis, preview render, audio, and export work.
- [ ] Extend service-core behavior to cover full current `AppModel` feature
  parity.

Affected files:

- `crates/dawn-app-runtime/src/services/**`
- `crates/dawn-app-runtime/src/read_model.rs`
- `crates/dawn-app-runtime/tests/service_cores.rs`
- Later parity work may read existing behavior from `crates/dawn-app-core/src/**`

### Phase 3: Supervised Runtime

- [x] Add `ServiceCore` trait and bounded `crossbeam-channel` runner.
- [x] Add explicit backpressure policy enum: reject, latest-only, coalesce.
- [x] Add typed shutdown and worker join surface.
- [x] Surface true runner/service failures as fatal typed events.
- [x] Surface ordinary command failures as request-scoped, non-sticky events.
- [ ] Implement actual latest-only queue replacement semantics per message type.
- [ ] Implement actual coalescing semantics per message type.
- [x] Add `AppCoordinator` for request IDs, service handles, routing policy,
  event fan-out, and read-model publication.
- [x] Add startup/shutdown orchestration for all service runners.
- [ ] Add runtime smoke tests for runner lifecycle, backpressure, and fatal
  event publication.

Affected files:

- `crates/dawn-app-runtime/src/runtime.rs`
- `crates/dawn-app-runtime/src/coordinator.rs`
- `crates/dawn-app-runtime/tests/**`

### Phase 4: Desktop Adapter Cutover

- [ ] Keep native dialogs, windows, shared preview buffers, native audio, file
  watcher adapters, and preview transport adapters in `apps/desktop`.
- [x] Add desktop-owned runtime state beside the old `AppModel` state.
- [x] Runtime-gate `update_active_text` while preserving
  `updateActiveText(text) -> RuntimeStateDto`.
- [x] Runtime-gate explicit `open_project` and `open_file` while preserving
  `RuntimeStateDto` command returns and `AppModel` runtime-state publication.
- [x] Runtime-gate explicit active-tab changes, close file, and view-mode
  changes while preserving `RuntimeStateDto` command returns and `AppModel`
  runtime-state publication.
- [x] Runtime-own startup project/session restore, with `AppModel` only
  mirroring the restored compatibility snapshot.
- [x] Runtime-gate create-file auto-open, active text undo, and active text
  redo while preserving `RuntimeStateDto` command returns.
- [x] Replace `get_snapshot`/`app_snapshot_changed` with
  `get_runtime_state`/`runtime_state_changed`.
- [x] Generate TypeScript bindings with the public `RuntimeStateDto` contract.
- [ ] Replace selected Tauri command wiring with explicit runtime commands that
  return minimal typed acknowledgements.
- [ ] Emit read-model slice events from backend to frontend.
- [ ] Move FSEQ export to a background task with task status records.
- [ ] Preserve old `AppModel` route until service parity is proven.
- [ ] Delete old `AppModel` dispatch route only after full parity and frontend
  migration pass.

Affected files:

- `apps/desktop/src/state.rs`
- `apps/desktop/src/app_runtime.rs`
- `apps/desktop/src/commands.rs`
- `apps/desktop/src/bindings.rs`
- `apps/desktop/src/bin/generate_bindings.rs`
- `apps/desktop/src/filesystem_watcher.rs`
- `apps/desktop/src/audio_runtime.rs`
- `apps/desktop/src/preview_transport.rs`
- `apps/desktop/src/live_output.rs`
- `crates/dawn-app-core/src/**` during final deletion only

### Phase 5: Frontend Read-Model Migration

- [x] Replace the frontend snapshot store name and event subscription with
  runtime-state hydration and `runtimeState` storage.
- [ ] Replace the single runtime-state store with focused Zustand slices/hooks.
- [ ] Preserve current screens and workflows.
- [ ] Add targeted UI for stale/updating state, autosave/task status,
  conflicts, and fatal runtime errors.
- [ ] Use frontend draft previews for interactions; committed state comes from
  backend ack/read-model events.
- [x] Remove frontend dependency on `AppSnapshotDto`, `getSnapshot`,
  `runSnapshotCommand`, and `app_snapshot_changed`.
- [ ] Remove remaining full-state assumptions after all commands use runtime
  slices and acknowledgements.

Affected files:

- `apps/desktop/frontend/src/api.ts`
- `apps/desktop/frontend/src/bindings.ts`
- `apps/desktop/frontend/src/store.ts`
- `apps/desktop/frontend/src/previewTransport.ts`
- `apps/desktop/frontend/src/ui/**`

### Phase 6: Parity, Cleanup, And Deletion

- [ ] Add focused runtime and contract tests for every service boundary.
- [ ] Cover stale revision rejection, conflict handling, autosave self-write
  tagging, dependency-incremental analysis, latest-only preview, passive audio
  preload, task/status events, event ordering, and binding compatibility.
- [ ] Use `examples/thirty-output-controller` as the primary realistic fixture.
- [ ] Use `examples/starter`, `examples/club-rig`, and
  `examples/christmas-house` for breadth.
- [ ] Use temporary directories for invalid or synthetic Dawn documents.
- [ ] Keep `pnpm check` passing at every milestone.
- [ ] Delete the old runtime path, compatibility code, and any temporary
  parallel-build wiring.

Affected files:

- `crates/dawn-app-runtime/tests/**`
- `crates/dawn-app-core/src/**`
- `apps/desktop/src/**`
- `apps/desktop/frontend/src/**`

## Protocols

Service cores are deterministic Rust state machines. Tests call cores directly
with typed commands and receive typed events. Production runners move the same
commands and events across bounded `crossbeam-channel` queues.

Commands carry a `RequestId` and expected `Revision`. A command that targets an
old revision is rejected with `RuntimeErrorKind::StaleRevision`; there is no
last-write-wins behavior. Command acknowledgements are minimal and contain an
optional target revision, not an app snapshot. Root and lifecycle commands use
no target revision because their committed revisions are only known after
service processing and event publication.

Ordinary command failures such as missing buffers are request-scoped and do not
set the global fatal read-model status. Fatal status is reserved for stopped
services, disconnected runners, panics, and equivalent runtime failures.

## Services

- `DocumentStore`: project root lifecycle, open buffers, active file, view mode,
  dirty state, and disk conflict state.
- `ProjectIndex`: in-memory analysis from project overlays with immutable
  `Arc<ProjectAnalysis>` snapshots.
- `SequenceEdit`: sequence GUI edits by path, object key, and expected revision.
- `PreviewEngine`: latest-only render requests and stale frame suppression.
- `AudioEngine`: audio readiness and clock status without mutating preview
  state during passive preload.
- `Autosave`: self-write tagging for watcher reconciliation.
- `FileWatcher`: external disk events, with self-writes ignored by tag.
- `LiveOutput`: consumes frames published by `PreviewEngine`.
- `LayoutPrefs`: window and workbench preference state.
- `ReadModel`: focused frontend slices composed from service events.

## Read Models

The frontend should move away from a single full snapshot toward focused slices:
workspace, editor, diagnostics, active GUI document, preview, transport/audio,
live output, status/tasks, and prefs/window layout. Slices expose `Revision`,
`stale`, and `updating` flags where async work can temporarily lag committed
editor state.

## Backpressure

Every production runner has a bounded queue and an explicit policy:

- `Reject`: fail the command immediately with `Backpressure`.
- `LatestOnly`: used for preview-like work where only the newest request should
  publish.
- `Coalesce`: used for read-model/status updates where equivalent pending work
  may be collapsed by the coordinator.

The first implementation provides the shared policy surface and rejection
behavior. Per-message coalescing and latest-only queue replacement are cutover
items before the old runtime is deleted.

## Logging

Runtime logging is opt-in. `init_file_logging` writes structured tracing output
to a file path selected by the desktop adapter. The runtime must not emit
unconditional console or stderr timing logs.

## Cutover Rules

1. Keep `.dawn` project files compatible.
2. Keep `dawn-project` synchronous and side-effect-light.
3. Do not add compatibility shims or hidden fallback paths.
4. Keep desktop-native dialogs, windows, shared preview buffers, and audio
   adapters in `apps/desktop`.
5. Delete the old `AppModel` dispatch path only after service parity and
   frontend read-model migration pass.

## Manual Verification Notes

Run these manually with `pnpm tauri dev`, then stop the dev server when done:

- Launch with a persisted project and restored tabs. Confirm the runtime-owned
  restored session opens in the saved tab order, the saved active tab is active,
  and saved view modes are applied before opening any new file.
- Open a different project. Confirm previous tabs are cleared and later valid
  file operations still work after any previous missing-buffer command error.
- Open multiple files, switch tabs, close inactive tabs, and close the active
  tab. Confirm active fallback follows the editor session rule.
- Create a file. Confirm the new file is opened as a runtime-known tab.
- Edit text, undo, redo, switch tabs, and toggle view mode. Confirm the
  compatibility snapshot mirrors runtime-approved text and view state.
- Trigger a missing-buffer command from DevTools/API. Confirm the command
  returns an error and a subsequent valid project/file command still succeeds.
