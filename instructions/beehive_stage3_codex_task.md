# beehive — Stage 3 Codex Task

## Workdir Scanner and File Management Foundation

This document is the **single source of truth** for Codex when implementing **Stage 3** of the desktop application **beehive**.

Stage 3 must build on the completed Stage 1 and Stage 2 foundation. Do not rely on assumptions outside this document. If a decision is not specified here, choose the option that best preserves historical data, keeps runtime behavior deterministic, and avoids irreversible file-system actions.

---

# 0. Current project state

The project already has:

- Tauri v2 desktop app shell;
- React + TypeScript frontend;
- Rust backend modules;
- SQLite via `rusqlite`;
- YAML parsing via `serde_yaml`;
- workdir initialization/open/reload;
- schema v2 bootstrap and migration;
- active/inactive stage lifecycle;
- stronger canonical/normalized workdir validation;
- manual non-recursive discovery of JSON files in active stage input folders;
- entity registration into SQLite;
- app events for discovery errors;
- runtime UI screens backed by real data.

Stage 2 is considered closed. Stage 3 must **not** rewrite the whole project. It must evolve the current implementation toward reliable file lifecycle management.

---

# 1. Critical Stage 3 design correction

## 1.1. Problem: Stage 2 entity model is too strict for future pipeline copies

In Stage 2, `entity_id` was treated as globally unique across the workdir, and the same `entity_id` at a different file path was treated as a duplicate error.

That was acceptable for Stage 2 discovery because workflow copying did not exist yet.

However, the original product model requires the same logical JSON entity to move through multiple stage folders as copied file instances:

```text
stages/incoming/entity-001.json
stages/normalized/entity-001.json
stages/enriched/entity-001.json
```

All of these files may legitimately represent the same logical entity ID at different stages.

Therefore, Stage 3 must evolve the model from:

```text
one entity_id = exactly one file path
```

into:

```text
one logical entity_id = many physical file instances across stages
```

This is the most important architectural correction in Stage 3.

## 1.2. Required Stage 3 decision

Introduce a persistent file-instance / artifact layer.

Recommended model:

- `entities` = logical entity aggregate;
- `entity_files` = physical JSON file instances in stage folders;
- `entity_stage_states` = status of logical entity on a specific stage, linked to the relevant file instance where possible.

After Stage 3, the same `entity_id` may exist in different stage folders **if each file belongs to a different stage**.

Still invalid:

- same `entity_id` appearing in two different files inside the same stage folder;
- one file path changing from one `entity_id` to another without explicit reset/reconciliation;
- unmanaged overwrite of a target file with different content.

---

# 2. Stage 3 goal

Stage 3 implements a reliable workdir scanner and file management foundation.

The target chain is:

```text
workdir -> active stages -> stage folders -> JSON file instances -> logical entities -> entity files -> stage states -> safe file operations -> UI visibility
```

Stage 3 must add:

1. schema v3 migration;
2. logical entity vs physical file-instance separation;
3. robust workdir reconciliation;
4. stage folder provisioning;
5. missing/deleted file detection;
6. safe atomic file copy utilities;
7. managed copy to next stage;
8. JSON metadata update for managed copies;
9. collision handling;
10. app events for file lifecycle operations;
11. minimal UI visibility for file instances and file operations.

Stage 3 is **still not** the n8n execution stage.

---

# 3. Testing policy for this stage

Do **not** spend quota on manual mouse-driven UI testing or screenshots; verify logic with automated/backend tests and use only minimal smoke checks that the app starts and pages do not crash.

---

# 4. Out of scope

Do not implement these features in Stage 3:

- n8n workflow execution;
- HTTP calls to workflow URLs;
- task queue worker;
- retry runtime engine;
- scheduled/background watcher;
- continuous filesystem daemon;
- multi-instance distributed locking;
- advanced visual graph editor;
- full JSON editor with arbitrary save;
- polished UI/UX QA;
- visual/pixel-perfect testing.

Stage 3 may implement backend operations that will later be used by n8n runtime, but it must not call n8n yet.

---

# 5. Required database changes

## 5.1. Schema version

Bump SQLite schema version from `2` to `3`.

Requirements:

- fresh DB must be created directly at v3;
- existing v2 DB must migrate to v3 without manual deletion;
- v1 -> v2 -> v3 path should still work if the current migration architecture supports it;
- `PRAGMA user_version` must become `3`;
- migration must be idempotent and tested.

## 5.2. `entities` table after Stage 3

`entities` should represent the **logical entity**, not a single physical file.

Minimum expected fields:

- `entity_id TEXT PRIMARY KEY`
- `current_stage_id TEXT NULL`
- `current_status TEXT NOT NULL`
- `latest_file_path TEXT NULL`
- `latest_file_id INTEGER NULL`
- `file_count INTEGER NOT NULL DEFAULT 0`
- `first_seen_at TEXT NOT NULL`
- `last_seen_at TEXT NOT NULL`
- `updated_at TEXT NOT NULL`

If keeping some Stage 2 fields for compatibility, document what is now considered legacy/summary data.

## 5.3. New `entity_files` table

Add a new table for physical JSON file instances.

Required fields:

- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `entity_id TEXT NOT NULL`
- `stage_id TEXT NOT NULL`
- `file_path TEXT NOT NULL UNIQUE`
- `file_name TEXT NOT NULL`
- `checksum TEXT NOT NULL`
- `file_mtime TEXT NOT NULL`
- `file_size INTEGER NOT NULL`
- `payload_json TEXT NOT NULL DEFAULT '{}'`
- `meta_json TEXT NOT NULL DEFAULT '{}'`
- `current_stage TEXT NULL`
- `next_stage TEXT NULL`
- `status TEXT NOT NULL`
- `validation_status TEXT NOT NULL`
- `validation_errors_json TEXT NOT NULL DEFAULT '[]'`
- `is_managed_copy INTEGER NOT NULL DEFAULT 0`
- `copy_source_file_id INTEGER NULL`
- `file_exists INTEGER NOT NULL DEFAULT 1`
- `missing_since TEXT NULL`
- `first_seen_at TEXT NOT NULL`
- `last_seen_at TEXT NOT NULL`
- `updated_at TEXT NOT NULL`

Recommended constraints:

- `FOREIGN KEY(entity_id) REFERENCES entities(entity_id)`
- `FOREIGN KEY(stage_id) REFERENCES stages(stage_id)`
- `FOREIGN KEY(copy_source_file_id) REFERENCES entity_files(id)`
- `UNIQUE(entity_id, stage_id)` unless there is a strong reason to allow multiple physical files for the same entity in the same stage.

If you choose not to enforce `UNIQUE(entity_id, stage_id)` at DB level, enforce it in code and explain why.

## 5.4. `entity_stage_states` evolution

Stage 3 should link stage states to file instances where possible.

Add if missing:

- `file_instance_id INTEGER NULL`
- `file_exists INTEGER NOT NULL DEFAULT 1`
- `last_seen_at TEXT NULL`

Recommended behavior:

- one logical state per `entity_id + stage_id`;
- state points to the most relevant `entity_files.id`;
- duplicate files for the same entity/stage are rejected and logged;
- existing Stage 2 rows should be migrated safely.

## 5.5. Migration from Stage 2 data

Existing Stage 2 `entities` rows currently represent both logical entity and file instance.

Migration to v3 must:

1. preserve logical entity IDs;
2. create one `entity_files` row per existing Stage 2 entity row;
3. update logical `entities` summary fields;
4. connect existing `entity_stage_states` to the newly created file instance where possible;
5. preserve app events and stages;
6. not require deleting `app.db`.

## 5.6. App events

Record app events for important file lifecycle changes:

- schema migrated to v3;
- stage directories provisioned;
- workspace reconciliation started/completed;
- file discovered;
- file updated;
- file missing;
- file restored/reappeared;
- duplicate entity file in same stage;
- managed copy created;
- managed copy skipped because target already exists;
- managed copy failed;
- unsafe file operation rejected.

Do not spam events for every unchanged file unless useful for debugging. Summary events are enough for normal scans.

---

# 6. Workdir scanner and reconciliation

## 6.1. Rename/clarify scan semantics

Stage 2 has `scan_workspace`. Stage 3 may keep that command name, but internally it should become a **reconciliation scan**, not just discovery.

A reconciliation scan should:

1. load active stages;
2. ensure required stage folders exist;
3. scan active stage input folders;
4. register new file instances;
5. update changed file instances;
6. mark missing files as missing instead of deleting rows;
7. restore missing files if they reappear;
8. update logical entity summary fields;
9. update stage states;
10. write a scan summary.

## 6.2. Active vs inactive stages

Rules:

- active stages are scanned;
- inactive stages are not scanned for new files;
- historical file records connected to inactive stages remain visible;
- if a stage becomes inactive, existing `entity_files` and `entity_stage_states` remain readable;
- no automatic deletion of inactive-stage files.

## 6.3. Stage folder provisioning

Stage 3 must ensure expected folders exist.

For every active stage:

- ensure `input_folder` exists;
- ensure `output_folder` exists if configured;
- create missing directories safely;
- record a summary event if directories were created.

This should happen during bootstrap/reconciliation, not through ad-hoc UI code.

## 6.4. File eligibility

A file is eligible if:

- extension is `.json`, case-insensitive;
- it is a regular file;
- it is inside an active stage input folder;
- it can be read;
- root JSON is an object;
- `id` is present and non-empty;
- `payload` is present and non-null.

Invalid files must not crash the scan.

## 6.5. Missing/deleted files

If a previously registered file path is no longer present in the filesystem:

- do not delete its DB row;
- mark `entity_files.file_exists = 0`;
- set `missing_since` if not already set;
- update related `entity_stage_states.file_exists = 0` if applicable;
- write an app event `file_missing`;
- keep it visible in diagnostics/workspace explorer.

If the file reappears:

- set `file_exists = 1`;
- clear `missing_since`;
- update checksum/mtime/size if changed;
- write an app event `file_restored` or include it in scan summary.

## 6.6. Changed files

If a registered file still exists but checksum/mtime/size changed:

- update `entity_files` metadata;
- update parsed `payload_json`, `meta_json`, `current_stage`, `next_stage`, validation data;
- update logical `entities.updated_at` and summary fields;
- keep `first_seen_at` stable;
- update `last_seen_at`;
- count it as `updated` in scan summary.

## 6.7. Re-scan idempotency

If nothing changed:

- no duplicate rows;
- no duplicate stage states;
- unchanged files counted as unchanged;
- no noisy app events for unchanged files unless intentionally summarized.

---

# 7. Duplicate and conflict rules

## 7.1. Same entity ID in different stages

Allowed if:

- each file is in a different active stage folder;
- there is at most one file instance per `entity_id + stage_id`;
- the JSON `id` matches the logical entity ID;
- file paths are distinct.

This is required for future workflow progression.

## 7.2. Same entity ID twice in the same stage

Invalid.

Behavior:

- do not overwrite the first file instance;
- record app event `duplicate_entity_in_stage`;
- include it in scan errors/diagnostics;
- keep scan running.

## 7.3. Same file path changes entity ID

Invalid by default.

If a file path was registered as `entity-001` and now declares `entity-999`:

- do not silently mutate the existing identity;
- record app event `entity_id_changed_for_path`;
- do not overwrite the existing logical mapping unless an explicit future reset operation is implemented.

## 7.4. Target copy collision

When creating a managed copy, if target path already exists:

- if it represents the same `entity_id` and same checksum, treat as already existing / idempotent;
- if it represents the same `entity_id` but different checksum, do not overwrite automatically;
- if it represents another `entity_id`, fail the operation;
- record a clear app event.

No destructive overwrite in Stage 3.

---

# 8. Safe atomic file operations

## 8.1. File operation module

Create or extend a backend module for file operations.

Recommended module:

```text
src-tauri/src/file_ops/
```

or, if the codebase is cleaner with a different name:

```text
src-tauri/src/workspace_files/
```

Do not put file-copy logic directly into Tauri command handlers.

## 8.2. Atomic write/copy requirements

When writing a managed copy:

1. read source file;
2. build the target JSON content;
3. write to a temporary file in the target directory;
4. flush/sync best-effort where practical;
5. rename temporary file to final target path;
6. remove temporary file on failure if safe;
7. never leave partially written final JSON.

Temporary files should be clearly identifiable, for example:

```text
.beehive-tmp-<uuid-or-timestamp>.json
```

If adding a UUID dependency is unnecessary, timestamp/random suffix is acceptable.

## 8.3. JSON metadata update for managed copies

When creating a copy for a target stage, the copied JSON should be updated in memory before writing.

Required updates:

- `current_stage` = target stage ID;
- `status` = `pending` if target stage is executable/active;
- `status` = `blocked` if there is no valid next executable stage;
- `next_stage` = target stage's configured `next_stage`, if any;
- `meta.updated_at` = current timestamp;
- `meta.beehive.copy_source_stage` = source stage ID;
- `meta.beehive.copy_target_stage` = target stage ID;
- `meta.beehive.copy_created_at` = current timestamp.

Do not mutate the source JSON file during copy creation unless explicitly requested by a future runtime success operation.

## 8.4. Target filename rule

Default target filename:

- preserve original file name.

If target file already exists:

- do not overwrite destructively;
- return a structured collision result;
- allow idempotent success only when existing target content is compatible.

Do not invent complicated filename suffixing unless necessary. Determinism is more important.

---

# 9. Managed copy to next stage

## 9.1. Purpose

Stage 3 must implement the backend building block that later n8n runtime will call after a successful workflow.

The operation should be something like:

```text
create_next_stage_copy(workdir_path, entity_id, source_stage_id)
```

Exact command/function names are flexible, but the behavior must be clear.

## 9.2. Next stage resolution

Resolve the target stage in this order:

1. `next_stage` from the latest source file JSON if present and valid;
2. `next_stage` from the source stage definition;
3. no target stage -> terminal/blocked result.

If target stage ID exists but is inactive:

- do not copy as pending;
- return blocked/invalid target result;
- record app event.

## 9.3. Copy behavior if target stage exists and is active

When target stage exists and is active:

1. source file remains in place;
2. target directory is ensured;
3. JSON copy is written atomically;
4. copy JSON gets `status = pending`;
5. copy JSON gets `current_stage = target_stage_id`;
6. copy JSON gets target stage's `next_stage`;
7. DB registers/updates the new `entity_files` row;
8. DB creates/updates target `entity_stage_states` row with `pending`;
9. operation returns structured success with source and target paths.

## 9.4. Copy behavior if no target stage exists

If there is no next stage:

- do not create a fake target stage;
- return a terminal result with status `blocked`;
- optionally update source entity/stage state to `blocked` only if the operation explicitly represents terminal promotion;
- record an app event explaining that no next stage exists.

Because Stage 3 does not run workflows yet, avoid automatically marking source stage `done` unless this operation explicitly has an argument like `mark_source_done`.

Recommended Stage 3 default:

- copy operation does **not** mark source `done` automatically;
- it only creates/registers the next-stage pending copy.

## 9.5. Source state mutation

Do not mark source stage as `done` as part of generic file copy by default.

Reason: in the future, source `done` must be tied to successful n8n execution, not to the mere fact that a copy function was called.

If you add an internal option for future use, it should default to false and be documented.

---

# 10. Backend command requirements

Add or update Tauri commands with typed results.

Suggested commands:

```text
scan_workspace(path: String) -> ScanWorkspaceResult
ensure_stage_directories(path: String) -> StageDirectoryProvisionResult
list_entity_files(path: String, entity_id: Option<String>) -> EntityFilesResult
create_next_stage_copy(path: String, entity_id: String, source_stage_id: String) -> FileCopyResult
get_workspace_explorer(path: String) -> WorkspaceExplorerResult
```

You may choose different names if they fit the existing architecture better.

Requirements:

- command handlers must stay thin;
- business logic belongs in service/database/file modules;
- results must be structured;
- errors must include code/message/path where useful;
- frontend must not pass raw DB path if backend can derive it from workdir.

---

# 11. Frontend requirements

UI should expose Stage 3 functionality enough to verify and use the file management foundation, but do not over-polish.

## 11.1. Dashboard

Update Dashboard to show:

- scan/reconciliation summary;
- present file count;
- missing file count;
- managed copy count;
- invalid file count;
- last reconciliation timestamp.

Keep the `Scan workspace` button.

## 11.2. Entities page

Update Entities page to reflect logical entities and file instances.

Minimum:

- entity ID;
- current/latest stage;
- current status;
- file count;
- latest file path;
- updated at;
- validation indicator.

If the existing table still lists file-level rows, make the distinction clear in labels.

## 11.3. Entity Detail

Entity Detail must show:

- logical entity summary;
- all physical file instances for that entity;
- stage state rows;
- missing/present status per file;
- checksum and updated timestamp per file;
- source/managed copy metadata where available.

Optional but useful:

- a backend-backed button for `Create next-stage copy`.

If adding this button risks UI complexity, expose the backend command and document it; the UI can remain minimal.

## 11.4. Workspace Explorer

Workspace Explorer should show:

- stage groups;
- active/inactive stage lifecycle;
- present files;
- missing previously registered files;
- invalid/unregistered files from recent scan events;
- managed copies if identifiable.

A simple grouped list is enough.

## 11.5. Diagnostics

Diagnostics should show:

- schema version 3;
- stage folder provisioning info;
- last reconciliation summary;
- file lifecycle app events;
- copy operation events.

---

# 12. Scan result / summary expectations

Update scan summary to include at least:

- `scan_id`
- `scanned_file_count`
- `registered_file_count`
- `registered_entity_count`
- `updated_file_count`
- `unchanged_file_count`
- `missing_file_count`
- `restored_file_count`
- `invalid_count`
- `duplicate_count`
- `created_directory_count`
- `elapsed_ms`
- `latest_discovery_at`

Names can differ, but meaning should be represented.

---

# 13. Required tests

Add or update backend tests for the following.

## 13.1. Schema and migration

- fresh DB bootstrap creates schema v3;
- v2 database migrates to v3;
- existing Stage 2 entities migrate into `entity_files`;
- stage lifecycle fields survive migration;
- `PRAGMA user_version = 3` after migration.

## 13.2. Stage folder provisioning

- missing active stage input/output folders are created;
- inactive stage folders are not required for scan;
- provisioning is idempotent.

## 13.3. Logical entity and file instance model

- same entity ID in two different stages is allowed;
- same entity ID twice in the same stage is rejected/logged;
- same file path changing entity ID is rejected/logged;
- logical entity summary updates after multiple file instances.

## 13.4. Reconciliation

- valid file registers entity and entity_file;
- unchanged re-scan is idempotent;
- changed file updates checksum/mtime/updated_at;
- deleted file is marked missing, not deleted;
- missing file reappearing is restored;
- inactive stage is not scanned for new files;
- malformed JSON is recorded without crashing;
- missing `id` is recorded without registering;
- missing `payload` is recorded without registering.

## 13.5. Atomic copy / next-stage copy

- copy to active next stage creates a JSON file in target stage folder;
- copied JSON has updated `current_stage`, `status`, `next_stage`, and `meta.beehive` fields;
- DB registers the target file instance;
- target stage state becomes `pending`;
- source file is not mutated;
- source stage is not marked `done` by default;
- target collision with different content fails safely;
- repeated copy with compatible existing target is idempotent or returns a clear already-exists result;
- no target stage returns blocked/terminal result without fake stage creation.

## 13.6. Command-level tests if practical

If the project structure allows it, add tests around command/service boundary. If not, backend module tests are sufficient.

---

# 14. Technical verification requirements

Codex must run and report:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
```

```powershell
cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'
```

```powershell
npm.cmd run build
```

If any command fails, do not hide it. Report the command, failure, and what was done.

---

# 15. Documentation requirements

Create/update Stage 3 docs:

```text
docs/codex_stage3_execution_plan.md
docs/codex_stage3_progress.md
docs/codex_stage3_instruction_checklist.md
docs/codex_stage3_delivery_report.md
docs/codex_stage3_questions.md
```

Delivery report must include:

1. what was implemented;
2. files changed;
3. schema v3 and migration decision;
4. logical entity vs file instance model;
5. reconciliation behavior;
6. missing/deleted file behavior;
7. managed copy behavior;
8. collision rules;
9. UI changes;
10. tests added/updated;
11. technical verification commands/results;
12. known limitations;
13. remaining blockers.

Progress log must be updated as work proceeds.

Checklist must mark only what was actually verified.

---

# 16. Implementation guidance

## 16.1. Keep boundaries clean

Expected backend shape:

```text
src-tauri/src/
  bootstrap/
  commands/
  config/
  database/
  discovery/
  file_ops/            # or equivalent
  domain/
  workdir/
```

Do not create giant modules if the code starts becoming hard to review.

## 16.2. Preserve data

Default behavior should preserve historical data.

Do not delete:

- stage rows;
- entity rows;
- entity file rows;
- stage state rows;
- app events.

Mark things inactive/missing/archived instead.

## 16.3. Avoid hidden destructive behavior

No silent overwrites.
No silent identity mutation.
No silent deletion of file history.
No automatic source mutation during copy.

## 16.4. Design for future Stage 4

Future stages will need:

- task queue;
- n8n execution;
- source stage `done` after successful workflow;
- copy to next stage after workflow success;
- retry runtime;
- reconciliation after app restart.

Stage 3 should provide safe primitives for those future behaviors without implementing the n8n runtime yet.

---

# 17. Acceptance criteria

Stage 3 is complete only if:

- schema version is 3;
- v2 -> v3 migration works;
- `entity_files` or equivalent physical file-instance model exists;
- same logical entity can have files in multiple stages;
- duplicate same entity in same stage is handled explicitly;
- active stage folders are provisioned;
- scan/reconciliation detects new, changed, missing, restored, invalid, and duplicate files;
- missing files are marked, not deleted;
- managed next-stage copy exists as a safe backend operation;
- managed copy writes JSON atomically;
- managed copy updates target JSON metadata correctly;
- source file is not mutated by default;
- UI exposes enough information to inspect logical entity and file instances;
- backend tests cover the required scenarios;
- `cargo fmt`, Rust tests, and `npm.cmd run build` pass;
- Stage 3 docs are finalized and honest.

---

# 18. Response format for Codex

Return the result in this exact structure:

## A. What was implemented

## B. Files changed

## C. Schema v3 and migration behavior

## D. Logical entity vs file instance model

## E. Workdir reconciliation behavior

## F. Missing/restored file behavior

## G. Managed copy behavior

## H. Collision and duplicate handling

## I. UI changes

## J. Tests added/updated

## K. Technical verification results

Include commands and pass/fail.

## L. Smoke check result

Only minimal app-start/page-does-not-crash smoke, if performed.

## M. Known limitations

## N. Remaining blockers

---

# 19. Final instruction

Build Stage 3 as a reliable file lifecycle foundation. The important outcome is not a flashy UI; the important outcome is that beehive can safely understand, preserve, reconcile, and copy JSON file instances across stage folders without corrupting history or losing control of the workdir.
