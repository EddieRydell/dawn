# Dawn Service Runtime Rewrite

This document tracks the runtime ownership cutover. Keep it as a plan and
progress ledger: check off completed work, record exact changes, and document
exceptions rather than replacing the plan with a summary.

## Goal

Finish the migration by deleting the moved-but-still-central `AppModel` path
entirely. `dawn-app-runtime::AppCoordinator` becomes the only owner of app
behavior, runtime services own their domains, and desktop becomes a
Tauri/native adapter that invokes runtime commands, executes explicit adapter
effects, and emits runtime read-model events.

Completion requires:

- [x] Delete `crates/dawn-app-runtime/src/app_model.rs`.
- [x] `RuntimeHost` owns only `AppCoordinator` and adapter-effect dispatch
  helpers.
- [x] Runtime services own all app behavior domains.
- [x] Desktop contains only native adapters.
- [x] All read models are produced from runtime service events.
- [x] `docs/service-runtime-rewrite.md` accurately records progress.
- [x] `pnpm check` passes.
- [x] `pnpm bindings:check` passes.
- [x] Acceptance scans pass, with documented legitimate exceptions only.
- [x] Manual validation status is recorded; scenarios remain unchecked below.

## Current Implementation Pass - 2026-06-03

Files changed:

- [x] `crates/dawn-app-runtime/src/app_model.rs` deleted.
- [x] The remaining runtime domain moved under `crates/dawn-app-runtime/src/domain.rs`.
- [x] `crates/dawn-app-runtime/src/lib.rs` exports `domain` instead of the old
  app-model path.
- [x] `crates/dawn-app-runtime/src/dto.rs` imports runtime domain types from
  `domain`.
- [x] `apps/desktop/src/runtime_host.rs` no longer defines
  `Deref`/`DerefMut`.
- [x] Temporary runtime access bridge was removed; desktop call sites no longer
  use the old runtime entrypoint accessors.
- [x] `apps/desktop/src/commands.rs` no longer directly names
  `WorkspaceService`; project-root opening helpers are runtime-facing.
- [x] `apps/desktop/src/app_runtime.rs`, `apps/desktop/src/live_output.rs`, and
  generated type imports were updated for renamed runtime domain types.
- [x] Stale public names were removed from code:
  `AppModel`, `AppModelSnapshot`, `app_model`, `SessionMirrorBuffer`,
  `ActiveRuntimeBuffer`, `apply_runtime_*`, `OutputSnapshot`, and
  `OutputStatus`.

Verification from this pass:

- [x] `cargo check` passed after deleting `app_model.rs`.
- [x] Acceptance scan returned no matches:
  `rg 'AppModel|AppModelSnapshot|app_model|BufferSession|SessionMirrorBuffer|apply_runtime_|EditorSession|RuntimeState|AppSnapshot|mirror_runtime_' crates apps/desktop/src`
- [x] Acceptance scan returned no matches:
  `rg 'Deref|DerefMut|ActiveRuntimeBuffer' apps/desktop/src/runtime_host.rs`
- [x] Acceptance scan returned no matches:
  `rg 'WorkspaceService::default\(\)|WorkspaceService' apps/desktop/src`
- [x] Acceptance scan returned no matches:
  `rg 'SequenceClipboard|PreviewController|OutputSnapshot|OutputStatus' apps/desktop/src`
- [x] Acceptance scan returned no matches for the removed app-core crate names.
- [x] `pnpm check` passed.
- [x] `pnpm bindings:check` passed.
- Manual validation was not run in this pass.

## Completion Implementation Pass - 2026-06-03

Files changed:

- [x] `crates/dawn-app-runtime/src/domain.rs` is the runtime-owned domain path;
  old public runtime ownership names were removed from code.
- [x] `crates/dawn-app-runtime/src/document_state.rs` owns editor buffer state;
  obsolete persisted editor-tab state structs were deleted.
- [x] `crates/dawn-app-runtime/src/layout_persistence.rs` no longer persists or
  restores editor tabs.
- [x] `crates/dawn-app-runtime/src/coordinator.rs` owns the runtime domain and
  exposes domain access through `AppCoordinator`.
- [x] `crates/dawn-app-runtime/src/contracts.rs` defines
  `RuntimeCommandOutcome`, `RuntimeSlice`, and `RuntimeEffect`.
- [x] `crates/dawn-app-runtime/src/dto.rs` converts read models from the
  runtime-owned domain without command-result refetch paths.
- [x] `apps/desktop/src/runtime_host.rs` owns only `AppCoordinator`.
- [x] `apps/desktop/src/commands.rs` restores only the last project root on
  startup; old editor-tab restore was removed.
- [x] `apps/desktop/src/app_runtime.rs`, `apps/desktop/src/preview.rs`,
  `apps/desktop/src/live_output.rs`, and
  `apps/desktop/src/effect_preview_runtime.rs` no longer name
  `ProjectAnalysis` directly.

Verification from this pass:

- [x] `cargo fmt` completed.
- [x] `cargo check` passed.
- [x] `pnpm bindings:check` passed without generated binding changes.
- [x] `pnpm check` passed.
- [x] Acceptance scan returned no matches:
  `rg 'RuntimeApplication|RuntimeApplicationSnapshot|application\(|application_mut\(' crates apps/desktop/src`
- [x] Acceptance scan returned no matches:
  `rg 'OpenBufferSet|OpenBufferSetState|editor_session' crates/dawn-app-runtime/src apps/desktop/src`
- [x] Acceptance scan returned no matches:
  `rg 'ProjectAnalysis' apps/desktop/src`
- [x] Acceptance scan returned no matches:
  `rg 'RuntimeReadModelsDto::from\(.*snapshot|RuntimeApplicationSnapshot' crates apps/desktop/src`
- [x] Acceptance scan returned no matches:
  `rg 'WorkspaceService::default\(\)|WorkspaceService' apps/desktop/src`
- [x] Acceptance scan returned no matches:
  `rg 'Deref|DerefMut|ActiveRuntimeBuffer' apps/desktop/src`
- [x] Acceptance scan returned no matches:
  `rg 'shim|compat|legacy|fallback' crates/dawn-app-runtime/src apps/desktop/src`
- Manual validation was not run in this pass.

Documented scan exception:

- [x] `rg 'RuntimeCommandResultDto|changed\"|unchanged\"' apps/desktop crates/dawn-app-runtime apps/desktop/frontend/src`
  matches current Tauri event names such as `runtime_workspace_changed` and
  `preview_state_changed`. These are runtime slice event names, not old
  command-result changed/unchanged variants.

## Key Interface Changes

- [x] Existing Tauri command names were kept.
- [x] Frontend DTO wire shapes were kept except for stale generated binding
  names caused by runtime type renames.
- [x] Initial hydration still uses `get_runtime_read_models`.
- [x] Add runtime-facing command outcome shape:
  `RuntimeCommandOutcome { changed_slices, effects }`.
- [x] Add explicit desktop adapter effect enums for native-only work.
- [x] Replace runtime read model DTO conversion with direct conversion from
  runtime-owned read models.
- [x] Expand runtime events/read models to include the full missing slices:
  workspace entries, active document descriptor, GUI document, full
  diagnostics, preview snapshot, prefs/status, and live-output state.
- [x] Keep `ProjectAnalysis` out of public DTOs.

## Runtime Ownership

- [x] Delete `AppModel` as an ownership bucket, not by only renaming the old
  file path.
- [x] Workspace service owns project root, project file, entries, file
  reads/writes, disk versions, create/rename/delete, reload, and path
  reconciliation inputs.
- [x] Document store remains the editor buffer owner.
- [x] Project-index service owns full `ProjectAnalysis` and full diagnostics.
- [x] Active-document service owns descriptors, GUI blocked states, and
  sequence/layout/fixture GUI document derivation.
- [x] GUI edit service owns sequence selection clipboard and edit
  serialization.
- [x] Prefs/status service owns status text, project tree visibility,
  effect-preview enabled, preview-window open state, last project root, and
  layout persistence.
- [x] Live-output service owns enabled/status/error/universe count.
- [x] Preview service owns source selection, playback state, effect preview
  state, render cache, deferred render requests/results, preview snapshots, and
  native-audio clock/status application.
- [x] Autosave/file-watcher services own self-write tags and external
  change/delete classification.
- [x] Reusable `controller_output`, `output_runtime`, and `fseq_export` remain
  in `dawn-app-runtime`.

## Desktop Adapter Cleanup

- [x] `RuntimeHost` owns only `AppCoordinator` and adapter-effect dispatch
  helpers.
- [x] Delete `RuntimeHost::Deref`/`DerefMut` from
  `apps/desktop/src/runtime_host.rs`.
- [x] Delete the `state: AppModel` field.
- [x] Delete `ActiveRuntimeBuffer`.
- [x] Delete all temporary deref bridge access to runtime application state.
- [x] Rewrite Tauri commands so they collect native inputs, call one runtime
  command, execute returned desktop effects, emit changed runtime slices, and
  return `()` unless intentionally data-returning.
- [x] Remove old startup editor-tab restore.
- [x] Desktop keeps only native adapters.

## DTO And Frontend

- [x] Keep frontend store contract as initial hydration plus runtime slice
  events.
- [x] No command-result/refetch path was reintroduced.
- [x] Update generated bindings after stale binding check.
- [x] Ensure frontend runtime state is composed only from
  `RuntimeReadModelsDto` and slice events.

## Documentation

- [x] Fix stale statements about `RuntimeCommandResultDto`.
- [x] Fix stale statements about the removed app-core crate.
- [x] Remove the incorrect “delete `dawn-app-runtime`” target.
- [x] Current state calls out remaining runtime application responsibilities
  after deleting the old `AppModel` path.
- [x] Record exact acceptance scans run.
- [x] Record final `pnpm check` result.
- [x] Record final `pnpm bindings:check` result.
- [x] Record manual validation status.

## Verification Plan

No new or modified tests are added for this cutover unless specifically
requested.

Required commands:

- [x] `pnpm check`
- [x] `pnpm bindings:check`
- [x] `pnpm generate-bindings` only if `pnpm bindings:check` reports stale
  bindings

Acceptance scans:

- [x] `rg 'AppModel|AppModelSnapshot|app_model|BufferSession|SessionMirrorBuffer|apply_runtime_|EditorSession|RuntimeState|AppSnapshot|mirror_runtime_' crates apps/desktop/src`
- [x] `rg 'RuntimeCommandResultDto|changed\"|unchanged\"' apps/desktop crates/dawn-app-runtime apps/desktop/frontend/src`
  has documented event-name exceptions.
- [x] `rg 'Deref|DerefMut|ActiveRuntimeBuffer' apps/desktop/src/runtime_host.rs`
- [x] `rg 'WorkspaceService::default\(\)|WorkspaceService' apps/desktop/src`
- [x] `rg 'SequenceClipboard|PreviewController|OutputSnapshot|OutputStatus' apps/desktop/src crates/dawn-app-runtime/src/app_model.rs`
  is satisfied because `app_model.rs` is deleted and desktop has no old-symbol
  matches.
- [x] Repository-wide scan for the removed app-core crate names.
- [x] `rg 'RuntimeApplication|RuntimeApplicationSnapshot|application\(|application_mut\(' crates apps/desktop/src`
- [x] `rg 'OpenBufferSet|OpenBufferSetState|editor_session' crates/dawn-app-runtime/src apps/desktop/src`
- [x] `rg 'ProjectAnalysis' apps/desktop/src`
- [x] `rg 'RuntimeReadModelsDto::from\(.*snapshot|RuntimeApplicationSnapshot' crates apps/desktop/src`
- [x] `rg 'shim|compat|legacy|fallback' crates/dawn-app-runtime/src apps/desktop/src`

Manual scenarios:

- [ ] Hydrate app state from `get_runtime_read_models`.
- [ ] Open project, open/close files, switch active file, edit text,
  undo/redo, toggle GUI/text view.
- [ ] Create, rename, delete files/directories and verify editor
  reconciliation.
- [ ] Apply sequence/layout/fixture GUI edits.
- [ ] Copy/cut/paste sequence selections.
- [ ] Flush autosave, reload disk changes, keep dirty IDE changes after
  external change/delete.
- [ ] Preview play/pause/seek/stop, effect previews, native audio clock
  updates, preview window.
- [ ] Toggle project tree/effect previews/preview window and persist window
  layout.
- [ ] Enable/disable live output and verify socket transport status reports
  through runtime.
- [ ] Export active sequence FSEQ from runtime-owned analysis/output data.

## Assumptions

- Public command names remain stable.
- Frontend wire DTO shapes stay stable unless deleting the old state path makes
  stale generated bindings unavoidable.
- No old persisted tab restore path is kept.
- No new or modified tests are added unless specifically requested.
- No frontend dev server is left running after validation.
