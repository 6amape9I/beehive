# Stage 3 Instruction Checklist

The authoritative checklist source is `instructions/beehive_stage3_codex_task.md`.

## Core acceptance

- [x] Schema version is bumped to 3.
- [x] v2 to v3 migration works without manual DB deletion.
- [x] `entity_files` physical file-instance model exists.
- [x] Same logical entity can have files in multiple stages.
- [x] Duplicate same entity in the same stage is handled explicitly.
- [x] Active stage folders are provisioned automatically.
- [x] Reconciliation detects new, changed, missing, restored, invalid, and duplicate files.
- [x] Missing files are marked, not deleted.
- [x] Managed next-stage copy exists as a safe backend operation.
- [x] Managed copy writes JSON atomically.
- [x] Managed copy updates target JSON metadata correctly.
- [x] Source file is not mutated by default.
- [x] UI shows logical entities and physical file instances.
- [x] Backend tests cover the required Stage 3 scenarios.
- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml` passes.
- [x] Rust tests pass through `vcvars64.bat`.
- [x] `npm.cmd run build` passes.
- [x] Stage 3 docs are finalized and honest.

## Smoke verification

- [x] Desktop app starts after Stage 3 changes.
- [ ] Main runtime pages do not crash in a quick smoke pass.

## Notes

- A fresh `tauri dev` smoke run was attempted again after the app was closed.
- `beehive`, `cargo`, and `node` processes started successfully.
- UI Automation detected the expected `beehive` window and `beehive — веб-содержимое`.
- Main runtime pages were not re-visited manually, so only app start is marked as verified.
