# Service Runtime Rewrite

## Ownership Ledger

- [x] Removed the top-level runtime domain module.
- [x] Removed the top-level document state module.
- [x] Moved app runtime state into service-owned runtime modules.
- [x] Removed `RuntimeDomain` and `RuntimeDomainSnapshot` from runtime and desktop code.
- [x] Removed desktop `domain()` / `domain_mut()` runtime accessors.
- [x] Kept frontend command names and generated binding shapes stable.
- [ ] Manually verify project open, startup restore, file open, and file edit flows.
- [ ] Manually verify GUI sequence, layout, and fixture edits.
- [ ] Manually verify preview playback, effect preview, and live output flows.
- [ ] Manually verify filesystem watcher reconciliation.

## Progress Notes

### 2026-06-03

- Deleted `crates/dawn-app-runtime/src/domain.rs` by moving runtime ownership into `crates/dawn-app-runtime/src/services/app_core.rs`.
- Deleted `crates/dawn-app-runtime/src/document_state.rs` by moving editor buffer state into `crates/dawn-app-runtime/src/services/editor_state.rs`.
- Renamed old domain/editor ownership types so desktop no longer depends on `RuntimeDomain`, `RuntimeDomainSnapshot`, `DocumentBufferStore`, `FileDiskVersion`, `EditorBuffer`, or runtime domain accessors.
- Renamed workspace and preview helper APIs that leaked old ownership names.
- Replaced desktop active GUI document enum matching with runtime-owned query methods.
- Did not run manual scenario checks.

## Verification

Commands run:

```powershell
pnpm bindings:check
pnpm check
Test-Path crates/dawn-app-runtime/src/domain.rs
Test-Path crates/dawn-app-runtime/src/document_state.rs
rg "RuntimeDomain|RuntimeDomainSnapshot|domain\(|domain_mut\(" crates/dawn-app-runtime/src apps/desktop/src
rg "application\(|application_mut\(|RuntimeApplication|RuntimeApplicationSnapshot" crates apps/desktop/src
rg "shim|compat|legacy|fallback" crates/dawn-app-runtime/src apps/desktop/src
rg "ProjectAnalysis" apps/desktop/src
rg "RuntimeReadModelsDto::from\(.*snapshot|RuntimeDomainSnapshot|RuntimeApplicationSnapshot" crates apps/desktop/src
rg "Deref|DerefMut|ActiveRuntimeBuffer" apps/desktop/src
rg "get_runtime_read_models" apps/desktop/frontend/src
```

Results:

- `pnpm bindings:check` passed; bindings were not regenerated.
- `pnpm check` passed.
- `Test-Path` returned `False` for both deleted files.
- Overall acceptance scans returned no matches except the expected `get_runtime_read_models` binding entry.
