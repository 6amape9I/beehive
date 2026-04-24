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
- [x] `npm.cmd run build` passes.
- [x] Rust tests pass.
- [ ] Stage 2 docs are updated to match reality.

## Manual verification

- [ ] Fresh app launch.
- [ ] Open existing Stage 1 workdir.
- [ ] Initialize new workdir.
- [ ] Create valid JSON in active stage folder.
- [ ] Run manual scan.
- [ ] Verify entity appears in Entities table.
- [ ] Open Entity Detail.
- [ ] Edit JSON and rescan.
- [ ] Verify checksum and updated timestamp change.
- [ ] Create malformed JSON and verify error appears.
- [ ] Create JSON missing `id` and verify invalid behavior.
- [ ] Remove stage from YAML and verify it becomes inactive in DB/UI.
- [ ] Verify inactive stage folder is not scanned.
- [ ] Verify `npm.cmd run build` passes.
- [ ] Verify Rust tests pass through `vcvars64.bat`.
