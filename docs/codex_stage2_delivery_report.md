# Stage 2 Delivery Report

This report reflects the implemented Stage 2 runtime foundation and the finalization-pass verification performed on 2026-04-24.

## A. What was implemented

- Schema v2 bootstrap and idempotent migration from Stage 1 databases.
- Stable stage lifecycle in SQLite: YAML stages are upserted as active; removed YAML stages are archived/inactivated instead of hard-deleted.
- Stronger workdir path validation using normalized/canonical path checks for existing and non-existing targets.
- Manual non-recursive discovery of JSON files from active stage input folders only.
- Entity registration into `entities` plus matching `entity_stage_states` rows with SHA-256 checksum, file size, modified time, validation metadata, and timestamps.
- Structured `app_events` recording for scan lifecycle, invalid files, duplicate IDs, stage deactivation, and schema migration.
- Typed Tauri runtime commands for scan, summary, stages, entities, entity detail, app events, and workspace explorer data.
- Runtime UI for Dashboard, Entities, Entity Detail, Stage Editor, Workspace Explorer, and Settings / Diagnostics using real backend data.

## B. Files changed

- Backend:
  - `src-tauri/src/bootstrap/mod.rs`
  - `src-tauri/src/commands/mod.rs`
  - `src-tauri/src/config/mod.rs`
  - `src-tauri/src/database/mod.rs`
  - `src-tauri/src/discovery/mod.rs`
  - `src-tauri/src/domain/mod.rs`
  - `src-tauri/src/lib.rs`
  - `src-tauri/src/workdir/mod.rs`
- Frontend:
  - `src/app/AppShell.tsx`
  - `src/app/styles.css`
  - `src/components/CommandErrorsPanel.tsx`
  - `src/components/StatusBadge.tsx`
  - `src/lib/formatters.ts`
  - `src/lib/runtimeApi.ts`
  - `src/pages/DashboardPage.tsx`
  - `src/pages/EntitiesPage.tsx`
  - `src/pages/EntityDetailPage.tsx`
  - `src/pages/SettingsDiagnosticsPage.tsx`
  - `src/pages/StageEditorPage.tsx`
  - `src/pages/WorkspaceExplorerPage.tsx`
  - `src/types/domain.ts`
- Config and dependency metadata:
  - `src-tauri/Cargo.toml`
  - `src-tauri/Cargo.lock`
- Docs:
  - `docs/codex_stage2_execution_plan.md`
  - `docs/codex_stage2_progress.md`
  - `docs/codex_stage2_instruction_checklist.md`
  - `docs/codex_stage2_delivery_report.md`
  - `docs/codex_stage2_questions.md`

## C. Stage lifecycle decision

Removed stages from `pipeline.yaml` are no longer deleted from SQLite. Stage sync now keeps historical `stages` rows stable and applies lifecycle metadata:

- current YAML stages are upserted with `is_active = 1`;
- missing YAML stages are marked `is_active = 0`;
- archived rows receive `archived_at`;
- sync also tracks `last_seen_in_config_at`.

This keeps entity and stage-state history readable and prevents Stage 1 style hard-delete from breaking future runtime history.

## D. Workdir validation decision

Workdir validation now rejects unsafe paths using normalized/canonical checks instead of raw string comparison.

- Relative paths still fail.
- Existing paths are canonicalized before validation.
- New paths are validated by canonicalizing the nearest existing parent and then applying normalized remaining segments.
- Paths inside the application/runtime directory are rejected even if disguised with `..`, mixed separators, or Windows case variation.
- Errors remain user-facing and structured through bootstrap/workdir validation responses.

## E. SQLite schema and migration

Stage 2 bumps SQLite schema version from `1` to `2`.

- Fresh databases are created directly at v2.
- Existing v1 databases are migrated sequentially to v2 without manual deletion.
- `stages` gained lifecycle columns:
  - `is_active INTEGER NOT NULL DEFAULT 1`
  - `archived_at TEXT NULL`
  - `last_seen_in_config_at TEXT NULL`
- `entities` supports Stage 2 discovery metadata including file path/name, stage, checksum, file size, modified time, payload/meta JSON, validation status, validation errors, discovered/updated timestamps.
- `entity_stage_states` supports read-only Stage 2 runtime bootstrap with `UNIQUE(entity_id, stage_id, file_path)`.
- Migration/bootstrap writes schema/update events into `app_events` and keeps `PRAGMA user_version = 2`.

## F. Discovery and entity registration behavior

Discovery is manual and read-only toward user JSON files.

- Only active stages are scanned.
- Each active stage input folder is scanned non-recursively.
- Eligible files must be readable regular `.json` files in an active stage folder.
- Valid files must parse as JSON and include `id` and `payload`.
- For valid files the backend computes SHA-256, file size, and file modified timestamp, then inserts or updates one `entities` row and one matching `entity_stage_states` row.
- `entity_id` is globally unique across the workdir.
- Duplicate `entity_id` at a different file path is rejected, logged to `app_events`, and does not overwrite the existing entity row.
- Malformed JSON, missing `id`, or missing `payload` create `app_events` entries and do not register entity/state rows.
- Folder stage is authoritative; `current_stage` mismatch is stored as a validation warning.
- Re-scan is idempotent for unchanged files and updates checksum/metadata for changed files.

## G. UI changes

- Dashboard shows workdir/config/database state, active/inactive stage counts, entity counts, last scan summary, and a manual `Scan workspace` action.
- Entities page shows a real filterable/searchable entity table backed by SQLite data.
- Entity Detail shows metadata, validation issues, read-only JSON preview, and stage-state rows.
- Stage Editor shows active/inactive status, lifecycle metadata, folders, workflow settings, next stage, and entity counts.
- Workspace Explorer shows grouped stage folders with discovered files and invalid/unregistered discovery items derived from event data.
- Settings / Diagnostics shows schema version, lifecycle counts, recent app events, discovery errors, and workdir/config state.

## H. Tests added/updated

- Rust backend tests cover:
  - fresh v2 bootstrap;
  - v1 to v2 migration;
  - inactive stage lifecycle behavior;
  - workdir path validation edge cases;
  - valid JSON registration;
  - malformed JSON handling;
  - missing `id` handling;
  - missing `payload` handling;
  - duplicate entity ID handling;
  - unchanged re-scan idempotence;
  - changed-file checksum/timestamp update;
  - `current_stage` mismatch warning behavior.
- Frontend technical verification covers TypeScript compilation and Vite production build.

### Technical Verification Notes

| Command | Result | Pass/Fail | Notes |
| --- | --- | --- | --- |
| `cargo fmt --manifest-path src-tauri/Cargo.toml` | Exit code `0` | Pass | Formatting command completed without reported changes or errors. |
| `cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'` | Exit code `0` | Pass | 29 Rust tests passed. |
| `npm.cmd run build` | Exit code `0` | Pass | `tsc` and `vite build` succeeded. |

## I. Manual verification performed

This finalization pass did not repeat the full Stage 2 manual acceptance matrix.

Smoke-level verification only:

- the desktop app process and main window were confirmed running during the finalization pass;
- no mouse-driven page walkthrough or visual QA was repeated;
- no claim of full UI manual QA is made in this report.

## J. Known limitations

- Stage 2 is still a runtime foundation only; it does not execute n8n workflows.
- No retry runtime, task queue, file movement between stages, or automatic stage transitions are implemented.
- Discovery is manual only; there is no continuous watcher/background daemon.
- Workspace Explorer invalid-file visibility is event-driven rather than backed by a dedicated invalid-files table.
- This finalization pass intentionally limits UI verification to smoke level instead of full manual replay.

## K. Remaining blockers for Stage 2

None for procedural Stage 2 closure. The remaining gaps are intentional Stage 3+ scope items, not blockers for Stage 2 foundation completion.
