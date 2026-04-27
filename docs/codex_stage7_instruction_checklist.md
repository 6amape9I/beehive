# Stage 7 Instruction Checklist

- [x] Editor state command implemented.
- [x] Draft validation command implemented.
- [x] Save pipeline config command implemented.
- [x] Atomic YAML write implemented.
- [x] Backup / failure-safe strategy implemented.
- [x] Stage add/edit/remove UI implemented.
- [x] Project/runtime settings UI implemented.
- [x] Stage ID immutability enforced.
- [x] Terminal output rule enforced.
- [x] Path safety enforced.
- [x] Removed stages preserve DB history.
- [x] SQLite stage sync after save works.
- [x] Directories provisioned after save.
- [x] Validation errors shown in UI.
- [x] Save/discard/dirty state works.
- [x] README updated.
- [x] Rust tests added.
- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml` passed.
- [x] Rust tests passed through `vcvars64.bat`: 83 passed.
- [x] `npm.cmd run build` passed.
- [x] No real n8n endpoint called.
- [x] No manual UI walkthrough claimed.

## Notes

- Saved stage IDs are immutable in Stage 7.
- Removing a stage removes it from active YAML config only; SQLite history remains.
- Stage removal is blocked while other draft stages reference the target via `next_stage`.
- Stages with historical usage require explicit UI confirmation before draft removal.
