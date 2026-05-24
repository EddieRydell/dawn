## 2026-05-23 23:51:07 -07:00 - Atomic batch move bug

Status: Done
Area: Desktop backend / file operations
Severity: High
Type: Bug fix / regression prevention
User impact: Failed batch moves could leave files partially moved while the UI thought nothing changed, so open editor paths were not reconciled.

Root cause:
`move_paths` validated and renamed inside the same loop, so validation and mutation were interleaved.

Fix:
`move_paths` now precomputes and validates the complete move plan before mutation, applies renames only after validation, rolls back completed renames in reverse order if a later rename fails, and updates active sequence state only after the full batch succeeds.

Regression coverage:
- Rollback after later rename failure.
- Duplicate source rejection before mutation.
- Duplicate destination rejection before mutation.
- Nested selected path rejection before mutation.
- Existing later target conflict without partial mutation.
- Successful batch move output.
- Active sequence update only after full success.

Verification:
- `cargo test`
- `cargo check -p dawn-desktop`

Files changed:
- `apps/desktop/src/workspace.rs`
- `docs/fix_log.md`

Follow-up risk:
Rollback can still fail if the filesystem changes externally during the operation; the returned error now includes rollback failure detail.
