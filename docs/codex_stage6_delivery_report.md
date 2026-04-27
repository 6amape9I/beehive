# Stage 6 Delivery Report

## Implemented

- Stage 6 Entities table and Entity Detail operator workflow.
- Server-side entity search, filtering, sorting, and pagination.
- Entity detail read model with files, stage states, timeline, runs, selected JSON, and allowed actions.
- Backend-mediated manual actions: retry now, reset to pending, skip.
- Backend-mediated open file/folder and safe payload/meta JSON save.

## Backend/read models

- `list_entities` now accepts `EntityListQuery` and returns `EntityTableRow` rows with page metadata, available stages, and available statuses.
- Filtering/search/sorting/pagination are performed in SQLite with whitelisted sort columns and `LIMIT/OFFSET`.
- `get_entity` now accepts `selected_file_id` and returns a single detail payload with `stage_runs`, `timeline`, `selected_file_json`, and `allowed_actions`.
- Lightweight query indexes were added with `CREATE INDEX IF NOT EXISTS`; SQLite remains schema `user_version = 4`.
- Entity summary recomputation now preserves `entity_stage_states` runtime status when file metadata changes.

## Manual actions

- Added commands:
  - `retry_entity_stage_now`
  - `reset_entity_stage_to_pending`
  - `skip_entity_stage`
- Conservative Stage 6 semantics:
  - Retry now: `pending` and `retry_wait`.
  - Reset: `failed`, `blocked`, `skipped`, `retry_wait`.
  - Skip: `pending`, `retry_wait`.
- Manual reset/skip use explicit state machine transitions and write `app_events`.
- Retry now uses the existing safe manual/debug execution path and records a manual retry event.
- `failed` and `blocked` are not retried as a combined reset+run action; the operator must reset first.

## JSON viewer/editor

- Detail page shows selected file JSON.
- Editing is limited to business `payload` and `meta`.
- Save checks:
  - registered file id exists;
  - file path is inside the current workdir;
  - file exists and is stable;
  - disk checksum/size/mtime still match the DB snapshot;
  - payload is non-null;
  - meta is a JSON object.
- Save uses temp-file atomic rename, updates `entity_files`, recomputes entity summary without runtime-state regression, and writes `entity_file_json_saved`.

## Open file/folder

- Added commands:
  - `open_entity_file`
  - `open_entity_folder`
- Paths are resolved from registered entity file ids and validated against the selected workdir.
- OS open uses the cross-platform `opener` crate.
- Automated tests cover safe registered path resolution and unknown id rejection. No mouse-driven OS-open walkthrough was performed.

## Frontend/UI

- Added `@tanstack/react-table` for table rendering/state only; data remains server-side paginated/sorted.
- Replaced `EntitiesPage` with search, stage/status/validation filters, sortable columns, pagination, refresh, clear filters, and URL query state.
- Replaced `EntityDetailPage` with sections for header, manual actions, timeline, file instances, JSON viewer/editor, validation, and stage runs.
- Added components under:
  - `src/components/entities/`
  - `src/components/entity-detail/`

## Files changed

- Backend: `src-tauri/src/domain/mod.rs`, `src-tauri/src/database/mod.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/state_machine/mod.rs`, `src-tauri/src/file_ops/mod.rs`, `src-tauri/src/file_open/mod.rs`, `src-tauri/src/lib.rs`.
- Frontend: `src/types/domain.ts`, `src/lib/runtimeApi.ts`, `src/pages/EntitiesPage.tsx`, `src/pages/EntityDetailPage.tsx`, `src/components/entities/*`, `src/components/entity-detail/*`, `src/app/styles.css`.
- Dependencies: `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`.
- Docs: README and Stage 6 docs.

## Tests

- Added/updated Rust coverage for:
  - entity table filter/sort/page read model;
  - entity detail payload including runs/timeline/selected JSON/allowed actions;
  - manual reset/skip events and invalid active-state rejection;
  - safe open path resolution;
  - payload/meta JSON save, stale snapshot rejection, and runtime-status preservation.
- Existing Stage 1-5.5 regression tests continue to pass.

## Verification commands

- `cargo fmt --manifest-path src-tauri/Cargo.toml`
  - Result: pass.
- `cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'`
  - Result: pass, 74 tests passed.
- `npm.cmd run build`
  - Result: pass, TypeScript and Vite production build completed.

## Known limitations

- No full manual UI walkthrough was performed.
- No real n8n endpoint was called.
- No bulk actions, rich JSON diff/versioning, Monaco editor, Stage Editor CRUD, graph editor, or Workspace Explorer redesign were implemented.
- Timeline ordering uses the stage graph/next-stage chain available in SQLite; no new schema field for YAML order was added.

## Stage 6 acceptance status

Stage 6 is ready for review based on automated backend tests, TypeScript/Vite build, updated docs, and explicit limitations above.

