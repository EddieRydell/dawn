## 2026-05-25 16:09:55 -07:00 - Sequence raster RefCell borrow panic

Status: Done
Area: Desktop UI / sequence timeline
Severity: High
Type: Bug fix / regression prevention
User impact: Opening a resolved sequence with raster metadata can panic during timeline painting instead of rendering the sequence editor.

Root cause:
`paint_clips` holds an immutable borrow of timeline state while `paint_clip_raster` tries to mutably borrow the same state for `raster_cache`.

Fix:
`paint_clips` now copies `pixels_per_ms`, `scroll_x`, `scroll_y`, and `lane_height` into local scalar values before iterating clips. The immutable timeline state borrow is dropped before `paint_clip_raster` runs, so raster cache lookup and insertion can use the existing mutable borrow without a nested `RefCell` panic.

Regression coverage:
No tests added per instruction. Existing workspace tests cover project analysis and desktop service behavior; this UI paint regression is covered by build verification plus manual desktop rendering.

Verification:
- `cargo fmt`
- `cargo check -p dawn-desktop`
- `cargo test`
- `cargo run -p dawn-desktop` launched `target\debug\dawn-desktop.exe`; manual resolved-sequence raster interaction remains to be confirmed in the running app.

Files changed:
- `apps/desktop/src/ui/editor/gui/sequence.rs`
- `docs/fix_log.md`

Follow-up risk:
The narrow borrow-lifetime fix preserves the existing raster cache design. Manual GUI verification should still confirm raster clips render and clip selection, dragging, resizing, scroll, zoom, and degraded clips behave correctly.

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
