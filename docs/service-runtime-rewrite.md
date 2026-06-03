# Dawn Service Runtime Rewrite

This document records the post-cutover runtime contract. The desktop no longer
publishes an AppModel-shaped runtime blob to the frontend, and normal UI updates
flow through focused runtime read-model slices.

## Current Baseline

`RuntimeHost` owns desktop orchestration. It drives `dawn-app-runtime`
`DocumentStore` commands for project/session open, tab lifecycle, active file,
view mode, text edits, and undo/redo before mirroring state needed by the
desktop adapters. Desktop-owned adapters remain in `apps/desktop`: native
dialogs, filesystem watcher plumbing, native audio, preview transport/window,
live-output socket work, and file I/O.

The frontend hydrates with `get_runtime_read_models() -> RuntimeReadModelsDto`.
Mutating Tauri commands return `RuntimeCommandResultDto::{Changed, Unchanged}`
unless they are data-returning commands such as dialogs, preview scene/transport
queries, effect-preview results, sequence selection edits, or export-style
reports. A `Changed` result means committed read models were updated and slice
events are emitted; the command result is not a state snapshot.

## Slice Event Contract

Initial hydration returns all slices:

- `workspace: WorkspaceReadModelDto`
- `editor: EditorReadModelDto`
- `activeDocument: ActiveDocumentReadModelDto`
- `diagnostics: DiagnosticsReadModelDto`
- `preview: PreviewReadModelDto`
- `liveOutput: LiveOutputReadModelDto`
- `status: StatusReadModelDto`
- `prefs: PrefsReadModelDto`

Committed changes are published with focused events:

- `runtime_workspace_changed`
- `runtime_editor_changed`
- `runtime_active_document_changed`
- `runtime_diagnostics_changed`
- `runtime_preview_changed`
- `runtime_live_output_changed`
- `runtime_status_changed`
- `runtime_prefs_changed`

`preview_state_changed` remains the high-frequency preview transport event and
is derived from the preview slice plus timing data.

## Runtime Contracts

The runtime model uses typed command acknowledgements, request IDs, revisions,
and structured service errors. Stale document edits are rejected through expected
revisions; there is no last-write-wins behavior. Document buffers carry dirty
state, view mode, revisions, undo/redo history, and structured external disk
state.

Current external disk states:

- `Current`
- `ChangedOnDisk`
- `DeletedOnDisk`

The target disk identity contract is `DiskVersion { len, modified_millis,
content_hash }`. Desktop file I/O is responsible for computing and passing disk
versions into runtime-owned state transitions.

## Completed Migration Items

- Removed frontend use of `get_runtime_state`, `runtime_state_changed`, and
  generated `RuntimeStateDto`.
- Added `RuntimeReadModelsDto` and focused read-model DTOs for workspace,
  editor, active document, diagnostics, preview, live output, status, and prefs.
- Changed desktop publication to per-slice runtime events.
- Kept mutating commands snapshot-free with `RuntimeCommandResultDto`.
- Kept data-returning commands data-returning.
- Added runtime-native `DiskVersion` and moved `DocumentStore` buffers from
  revision-based disk identity plus a conflict boolean to
  `disk_version: Option<DiskVersion>` and
  `external_state: BufferExternalState`.
- Extended `DocumentStore` commands/events/read models for structured external
  changes, reload, keep, moved-path reconciliation, deleted-path
  reconciliation, editor text, dirty state, view mode, and disk version.
- Moved desktop file-version conversion to the desktop/runtime boundary for
  restored sessions and opened/created files.
- Regenerated TypeScript bindings from Rust sources.

## Remaining Runtime Debt

- Finish moving autosave and watcher decisions fully through `RuntimeHost`, then
  wire desktop watcher events to `DocumentStore` reload/keep/delete commands.
- Replace remaining desktop mirror state with direct runtime/service read models
  where adapters no longer need local mutable state.
- Move sequence clipboard and all GUI edit commit paths into `RuntimeHost`
  orchestration backed by `DocumentStore` buffers.
- Add adapter-boundary tests for autosave/watcher behavior and GUI edit commit
  paths requested by the migration plan.
- Delete obsolete compatibility helpers after the desktop no longer mirrors
  editor state for adapter compatibility.

## Manual Verification Notes

Run manual verification with `pnpm tauri dev`, then stop it afterward:

- Launch with a persisted project and restored tabs. Confirm tab order, active
  tab, and saved view modes restore before opening a new file.
- Open a different project. Confirm previous tabs clear and the project tree,
  editor, diagnostics, preview, live-output, status, and prefs update through
  slice events.
- Open files, switch tabs, close inactive tabs, and close the active tab.
  Confirm active fallback and editor text stay synchronized.
- Edit text, undo, redo, switch tabs, toggle GUI/text view, and confirm no
  command returns a snapshot.
- Trigger GUI sequence/layout/fixture edits and confirm autosave, diagnostics,
  active GUI document, and preview update from the slice store.
