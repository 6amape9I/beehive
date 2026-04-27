# Stage 6 Polish Instruction Checklist

## Scope

- [x] Did not start Stage 7.
- [x] Did not add YAML save, stage CRUD, background scheduler, role model, or new design system.
- [x] Did not bump SQLite schema version.

## Backend JSON Edit Policy

- [x] Backend checks runtime state before saving business JSON.
- [x] Save allowed for `pending`, `retry_wait`, `failed`, `blocked`, `skipped`.
- [x] Save rejected for `queued`, `in_progress`, `done`.
- [x] Missing `entity_stage_states` row rejects save with a scan-before-edit message.
- [x] Rejected save does not write the JSON file.
- [x] Rejected save does not update entity file checksum/mtime snapshot.
- [x] Rejected save records warning app event `entity_file_json_edit_rejected`.

## Entity Detail DTO / UI

- [x] Entity Detail DTO includes file-level allowed actions and reasons.
- [x] Entity JSON editor disables edit/save from backend policy.
- [x] Entity JSON editor shows a backend policy reason.
- [x] Read-only JSON preview remains available.

## Database Decomposition

- [x] Added a database submodule for Stage 6 entity policy/action helpers.
- [x] Kept public callers on the existing `crate::database::...` API shape.
- [x] Avoided broad refactor of schema, migration, executor, stage runs, and discovery persistence.

## Verification

- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml`
- [x] Rust tests through `vcvars64.bat` - 76 passed.
- [x] `npm.cmd run build`
