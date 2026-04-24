# Stage 2 Instruction Checklist

The authoritative checklist source is `instructions/beehive_stage2_codex_task.md`.

## Core acceptance

- [x] Stage lifecycle uses inactive/archive behavior instead of hard delete.
- [x] Workdir validation uses stronger normalized/canonical path checks.
- [x] Schema version is bumped to 2.
- [x] Fresh DB bootstrap at v2 works.
- [x] v1 to v2 migration works without manual DB deletion.
- [x] Manual workspace scan exists.
- [x] Only active stages are scanned.
- [x] Valid JSON files are registered as entities and stage states.
- [x] Invalid JSON files are recorded without crashing.
- [x] Duplicate entity IDs at different paths are handled explicitly.
- [x] Re-scan is idempotent for unchanged files.
- [x] Dashboard shows runtime counts.
- [x] Entities page shows real registered entities.
- [x] Entity Detail shows real entity data.
- [x] Stage Editor shows active/inactive stage status.
- [x] Workspace Explorer shows grouped runtime/discovery data.
- [x] Diagnostics shows schema/discovery/event information.
- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml` passes in the finalization pass.
- [x] `npm.cmd run build` passes.
- [x] Rust tests pass.
- [x] Stage 2 docs are updated to match reality.

## Smoke UI verification

- [x] Desktop app starts and a main window is present during the finalization pass.
- [ ] Main pages were manually re-rendered/visited in the finalization pass.
- [ ] Scan action was manually triggered in the finalization pass.

## Notes

- This finalization pass intentionally does not claim full manual UI QA.
- The unchecked smoke items were not re-verified because this pass was limited to technical verification and non-invasive smoke confirmation.
