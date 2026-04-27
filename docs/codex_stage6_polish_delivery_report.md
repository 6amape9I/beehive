# Stage 6 Polish Delivery Report

## Implemented

- Backend-enforced business JSON edit policy for `save_entity_file_business_json`.
- File-level allowed actions in Entity Detail payload.
- UI disable/reason behavior for the JSON editor.
- First safe database decomposition step via `src-tauri/src/database/entities.rs`.
- Rust coverage for allowed/rejected edit statuses and detail policy exposure.

## JSON Edit Policy

- Editable statuses: `pending`, `retry_wait`, `failed`, `blocked`, `skipped`.
- Locked statuses: `queued`, `in_progress`, `done`.
- Missing runtime state is locked with: `No runtime stage state exists for this file. Run Scan workspace before editing.`
- Rejected edits do not write the JSON file or update DB file metadata.
- Rejected edits write warning app event `entity_file_json_edit_rejected`.

## Database Decomposition

- Added `database/entities.rs` for Stage 6 entity action-policy and file edit-policy helpers.
- Existing callers continue using the `crate::database::...` surface.
- Schema, migrations, executor claim logic, stage run core, and discovery persistence were not refactored.

## Tests

- Added/updated Rust tests for:
  - allowed save statuses;
  - forbidden save statuses;
  - missing stage state rejection;
  - rejected-save file immutability;
  - rejected-save DB snapshot immutability;
  - rejected-save app event logging;
  - Entity Detail file policy DTO.

## Verification

- PASS: `cargo fmt --manifest-path src-tauri/Cargo.toml`
- PASS: `cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'`
  - Result: 76 passed, 0 failed.
- PASS: `npm.cmd run build`
  - Result: TypeScript compile and Vite production build completed.

No mouse-driven UI walkthrough was performed for this polish pass.

## Known Limitations

- This pass does not introduce a role/permission model.
- Completed `done` artifacts are locked for silent business JSON edits; any future post-completion correction should be an explicit workflow.
- Database decomposition is intentionally incremental, not a full backend rewrite.

## Acceptance Status

Accepted for the Stage 6 polish scope: backend policy, UI reflection, database submodule split, tests, docs, and technical verification are complete.
