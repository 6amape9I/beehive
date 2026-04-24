# Stage 3 Delivery Report

This report reflects the implemented Stage 3 file lifecycle foundation and the verification performed on 2026-04-24.

## A. What was implemented

- Schema v3 bootstrap and migration for the Stage 3 runtime model.
- Split of Stage 2 one-file-per-entity behavior into logical `entities` plus physical `entity_files`.
- Reconciliation scan with stage directory provisioning, changed-file updates, missing/restored tracking, and summary settings.
- Non-destructive historical tracking for inactive stages, missing files, and stage-state linkage.
- Safe atomic managed next-stage copy with metadata rewrite and collision handling.
- Stage 3 runtime UI updates for Dashboard, Entities, Entity Detail, Workspace Explorer, and Settings / Diagnostics.

## B. Files changed

- Backend:
  - `src-tauri/src/database/mod.rs`
  - `src-tauri/src/discovery/mod.rs`
  - `src-tauri/src/domain/mod.rs`
  - `src-tauri/src/file_ops/mod.rs`
  - `src-tauri/src/commands/mod.rs`
  - `src-tauri/src/lib.rs`
- Frontend:
  - `src/types/domain.ts`
  - `src/lib/runtimeApi.ts`
  - `src/pages/DashboardPage.tsx`
  - `src/pages/EntitiesPage.tsx`
  - `src/pages/EntityDetailPage.tsx`
  - `src/pages/WorkspaceExplorerPage.tsx`
  - `src/pages/SettingsDiagnosticsPage.tsx`
- Docs:
  - `README.md`
  - `docs/codex_stage3_execution_plan.md`
  - `docs/codex_stage3_progress.md`
  - `docs/codex_stage3_instruction_checklist.md`
  - `docs/codex_stage3_delivery_report.md`
  - `docs/codex_stage3_questions.md`

## C. Schema v3 and migration behavior

- SQLite schema version is now `3`.
- Fresh databases are created directly at schema v3.
- Existing Stage 2 databases migrate to v3 without deleting `app.db`.
- Existing v1 databases still bootstrap through the migration path into v3.
- Stage lifecycle fields on `stages` remain intact.
- Stage 2 `entities` rows are migrated into:
  - logical `entities` summary rows;
  - physical `entity_files` rows;
  - linked `entity_stage_states` file-instance references where possible.

## D. Logical entity vs file instance model

- `entities` now represent logical aggregates by `entity_id`.
- `entity_files` represent physical JSON file instances in stage folders.
- `entity_stage_states` track one logical state per `entity_id + stage_id` and link to the most relevant `entity_files.id`.
- Same logical `entity_id` is allowed across different stages.
- The same `entity_id` twice in the same stage is rejected and logged.
- Logical entity summaries track:
  - current/latest stage
  - current status
  - latest file path
  - latest file id
  - file count
  - validation summary
  - first seen / last seen / updated timestamps

## E. Workdir reconciliation behavior

- `scan_workspace` is now reconciliation, not only discovery.
- Reconciliation:
  - loads active stages;
  - provisions missing input/output directories;
  - scans active input folders non-recursively;
  - registers new file instances;
  - updates changed file instances;
  - marks missing files instead of deleting rows;
  - restores missing files when they reappear;
  - recomputes logical entity summaries;
  - updates stage states and summary settings;
  - records reconciliation app events.
- Inactive stages are not scanned for new files, but historical rows remain visible.

## F. Missing/restored file behavior

- Missing tracked files are not deleted from SQLite.
- When a previously registered file disappears:
  - `entity_files.file_exists = 0`
  - `missing_since` is set
  - related `entity_stage_states.file_exists = 0`
  - `file_missing` is recorded in `app_events`
- When that file reappears:
  - `file_exists` becomes `1`
  - `missing_since` is cleared
  - checksum/mtime/size and parsed JSON data are refreshed
  - `file_restored` is recorded

## G. Managed copy behavior

- Added Stage 3 managed next-stage copy in `file_ops`.
- The operation:
  - resolves source file instance by logical entity and source stage;
  - resolves target stage from file `next_stage`, then stage definition `next_stage`;
  - provisions target directories;
  - writes the target JSON atomically through a temp file and rename;
  - registers target `entity_files` and target `entity_stage_states` rows;
  - does not mutate the source file;
  - does not mark source stage `done` by default.
- Target JSON is updated with:
  - `current_stage`
  - `status = pending`
  - target `next_stage`
  - `meta.updated_at`
  - `meta.beehive.copy_source_stage`
  - `meta.beehive.copy_target_stage`
  - `meta.beehive.copy_created_at`

## H. Collision and duplicate handling

- Duplicate `entity_id` in the same stage is rejected with `duplicate_entity_in_stage`.
- Same file path changing to a different `entity_id` is rejected with `entity_id_changed_for_path`.
- Managed copy target collision rules:
  - target path with another entity => fail safely, no overwrite;
  - same entity with incompatible different content => fail safely, no overwrite;
  - same entity with compatible managed-copy metadata => `already_exists`;
  - no destructive overwrite is performed.

## I. UI changes

- Dashboard now shows logical entity count, present files, missing files, managed copies, invalid files, and last reconciliation timestamp.
- Entities page now lists logical entities instead of file-level rows.
- Entity Detail now shows:
  - logical entity summary;
  - all physical file instances;
  - stage state rows;
  - latest JSON preview;
  - minimal `Create next-stage copy` action.
- Workspace Explorer now shows present/missing file state and managed-copy visibility per stage.
- Settings / Diagnostics now show schema v3 and Stage 3 reconciliation/file lifecycle counts.

## J. Tests added/updated

- Migration and schema tests:
  - fresh v3 bootstrap
  - v2 -> v3 migration
  - v1 -> v3 bootstrap path
  - stage lifecycle preservation
- Reconciliation tests:
  - directory provisioning
  - provisioning idempotence
  - same entity across multiple stages
  - duplicate entity in the same stage
  - changed file update
  - missing file
  - restored file
  - inactive stage not scanned
  - malformed JSON
  - missing `id`
  - missing `payload`
  - path identity mutation rejection
- Managed copy tests:
  - copy to active next stage
  - target JSON metadata rewrite
  - DB registration of target file instance
  - source file unchanged
  - source stage not marked `done`
  - target collision failure
  - repeated compatible copy returns `already_exists`
  - no target stage returns blocked

## K. Technical verification results

- `cargo fmt --manifest-path src-tauri/Cargo.toml`
  - Pass
- `cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'`
  - Pass
  - `35` Rust tests passed
- `npm.cmd run build`
  - Pass

## L. Smoke check result

- A fresh `tauri dev` smoke run was retried after the app was closed.
- `beehive`, `cargo`, and `node` processes started successfully.
- UI Automation detected the expected `beehive` window and `beehive — веб-содержимое`.
- App start is verified at smoke level.
- No page-by-page manual crash check is claimed.

## M. Known limitations

- Stage 3 still does not execute n8n workflows.
- No retry runtime, task queue, or background watcher is implemented.
- Managed copy does not mark the source stage `done`.
- Discovery/reconciliation remains non-recursive by design.
- Smoke UI verification is still minimal; page-by-page crash checks were not replayed manually.

## N. Remaining blockers

- No product blocker remains for Stage 3 backend/runtime foundation.
- No blocking environment issue remains for minimal Stage 3 app start as long as the previous `beehive.exe` instance is closed first.
