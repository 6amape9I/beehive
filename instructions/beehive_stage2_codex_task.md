# beehive — Stage 2 Codex Task

## Runtime Foundation: File Discovery, Entity Registration, and Safe Stage Metadata

This document is the **single source of truth** for Codex when implementing **Stage 2** of the desktop application **beehive**.

Do not rely on assumptions outside this document. If something is not explicitly included here, treat it as out of scope unless it is required to keep the Stage 2 foundation correct, testable, and consistent with Stage 1.

---

# 0. Critical carry-over concerns from Stage 1

Before starting Stage 2, you must address two concerns discovered during the Stage 1 review.

These are not optional cleanup items. They are part of Stage 2 because Stage 2 introduces persistent entities, stage states, and historical records. If these concerns are ignored now, the runtime model will become fragile.

---

## 0.1. Problem 1: hard delete of stages is dangerous once runtime history exists

### Current issue

At the end of Stage 1, stage synchronization was changed so that the SQLite `stages` table mirrors `pipeline.yaml` exactly. Stale stages are hard-deleted when they disappear from YAML.

That was acceptable for Stage 1 because no real entity processing history existed yet.

However, in Stage 2 this becomes risky because the application will start storing:

- discovered JSON entities;
- entity stage states;
- file-to-stage relationships;
- future stage run history;
- references from entities and state rows to stage IDs.

If a stage is hard-deleted after entities have already been registered against it, the system may lose historical context or hit foreign key problems.

### Required Stage 2 decision

Replace the Stage 1 hard-delete behavior with a safer stage lifecycle model.

The preferred solution is:

- keep `stages` rows stable once created;
- add an `is_active` column;
- add `archived_at` or `deactivated_at` column;
- during YAML sync:
  - upsert current YAML stages as active;
  - mark stages not present in YAML as inactive/archived;
  - do not hard-delete historical stage definitions by default.

### Required behavior

After Stage 2:

- if a stage exists in YAML, it is present in SQLite with `is_active = true`;
- if a stage was previously present but removed from YAML, it remains in SQLite with `is_active = false`;
- inactive stages must not be selected for new file discovery or processing;
- historical entity/state rows must remain readable;
- UI should distinguish active and inactive stages at least in Diagnostics or Stage Editor;
- tests must prove that removing a stage from YAML archives/deactivates it instead of deleting it.

### Acceptance criteria for this concern

- no silent hard-delete of `stages` during normal YAML sync;
- tests cover stage deactivation/removal from YAML;
- existing Stage 1 stage sync tests are updated to reflect the new behavior;
- documentation explains why inactive stages exist.

---

## 0.2. Problem 2: workdir location validation is currently too rough

### Current issue

Stage 1 added a protection that rejects relative paths and rejects workdirs inside the application directory. This fixed a dev-mode relaunch loop where a workdir under `src-tauri/` caused SQLite writes to trigger rebuilds.

The current check is good enough for Stage 1, but rough for Stage 2 because it may not handle:

- path normalization;
- `..` segments;
- symbolic links;
- mixed Windows path separators;
- case-insensitive Windows paths;
- non-existing target directories during initialization.

### Required Stage 2 decision

Improve the workdir path validation so it is based on canonical or safely normalized paths.

### Required behavior

After Stage 2:

- relative paths are still rejected;
- workdirs inside the application/project/runtime directory are still rejected;
- the validation should use canonicalization when the path exists;
- for a new workdir that does not exist yet, canonicalize the nearest existing parent directory and validate against that;
- avoid accepting paths that escape or disguise themselves through `..` traversal;
- return clear user-facing error messages;
- add tests for absolute path, relative path, nested path, and path containing `..`.

### Acceptance criteria for this concern

- existing good paths still work;
- bad paths fail with clear errors;
- tests cover path validation edge cases;
- README or diagnostics docs mention the absolute/outside-app-directory requirement.

---

# 1. Stage 2 goal

Stage 2 is **not** about running n8n workflows yet.

Stage 2 is about building the runtime foundation that makes workflow execution safe later.

The goal is to implement:

1. stable stage metadata lifecycle;
2. stronger workdir path validation;
3. file discovery in active stage folders;
4. JSON entity registration in SQLite;
5. checksum/mtime tracking;
6. entity-stage state bootstrap;
7. table UI that displays registered entities;
8. basic entity detail view;
9. safe re-scan/reload behavior;
10. tests and manual verification proving the foundation works.

The end-to-end Stage 2 chain is:

```text
workdir -> pipeline.yaml -> active stages -> stage folders -> JSON files -> validation -> entity registration -> entity_stage_states -> UI visibility
```

---

# 2. What Stage 2 must not implement

Do **not** implement these features in Stage 2:

- n8n workflow execution;
- HTTP calls to workflow URLs;
- task queue execution;
- retry runtime engine;
- `queued` / `in_progress` processing runtime;
- file copying to the next stage after workflow execution;
- graph routing execution;
- automatic stage transition execution;
- advanced low-code stage editor;
- full JSON editor with saving;
- multi-user behavior;
- background daemon/service mode;
- complex locking across multiple application instances.

Placeholders are allowed where appropriate, but they must be clearly labeled as Stage 3+ functionality.

---

# 3. Existing Stage 1 foundation assumptions

The current project already has:

- Tauri v2 desktop shell;
- React + TypeScript frontend;
- Rust backend modules;
- SQLite via `rusqlite`;
- YAML parsing via `serde_yaml`;
- workdir initialization/open/reload;
- `pipeline.yaml` validation;
- initial SQLite schema;
- Dashboard;
- Stage Editor;
- Settings / Diagnostics;
- placeholder Entity and Workspace pages.

Stage 2 must extend this foundation without destroying the separation between:

- UI layer;
- Tauri command layer;
- bootstrap layer;
- config layer;
- database layer;
- workdir/filesystem layer;
- domain types.

---

# 4. Approved domain model

## 4.1. Stage statuses

The approved status set remains:

```text
pending
queued
in_progress
retry_wait
done
failed
blocked
skipped
```

Stage 2 should primarily use:

- `pending`
- `blocked`
- `failed` only for validation or registration-level failures if needed

Do not implement actual runtime progression into `queued`, `in_progress`, or `retry_wait` yet.

## 4.2. Entity concept

A JSON file discovered in an active stage input folder becomes an **entity**.

An entity is the application-level representation of one JSON file.

Each entity must have:

- stable entity ID;
- current file path;
- current stage;
- current status;
- checksum or equivalent content fingerprint;
- file modification timestamp;
- registration timestamp;
- update timestamp;
- validation state;
- optional parsed metadata from JSON.

## 4.3. Entity stage state concept

For each entity discovered in a stage folder, there must be a row representing the entity’s state on that stage.

At Stage 2, the first state should normally be `pending` if:

- the file is valid JSON;
- required fields are available or generated according to the chosen rule;
- the stage is active;
- the file belongs to that stage input folder.

If the file cannot be registered cleanly, the app must not crash. It should record a meaningful error or invalid-file event.

---

# 5. JSON file requirements for Stage 2

## 5.1. Expected JSON structure

The intended JSON structure is:

```json
{
  "id": "entity-0001",
  "current_stage": "ingest",
  "next_stage": "normalize",
  "status": "pending",
  "payload": {},
  "meta": {
    "created_at": "2026-04-23T12:00:00Z",
    "updated_at": "2026-04-23T12:00:00Z",
    "source": "manual"
  }
}
```

## 5.2. Required Stage 2 rule for IDs

Prefer reading `id` from the JSON file.

If `id` is missing, choose one of the following strategies and document it clearly:

### Preferred strategy

Reject registration as invalid and record a clear validation error.

### Acceptable alternative

Generate a deterministic ID based on file path or checksum, but only if this behavior is documented and tested.

For this project, the preferred behavior is to **require `id`**. That keeps the runtime model easier to reason about.

## 5.3. Required Stage 2 rule for `payload`

`payload` should be required for a normal valid entity.

If missing:

- the file should not be registered as a normal pending entity;
- the user should see a validation error;
- the app must not crash.

## 5.4. Required Stage 2 rule for `current_stage`

The stage can be determined from the folder where the file was discovered.

If the JSON contains `current_stage` and it conflicts with the folder’s stage, the system must flag this clearly.

For Stage 2, choose one deterministic rule:

### Recommended rule

The folder stage is authoritative for discovery, but mismatched `current_stage` produces a warning or validation issue.

Do not silently ignore mismatches without surfacing them.

## 5.5. Required Stage 2 rule for `next_stage`

`next_stage` can be read from JSON, but Stage 2 does not execute transitions yet.

For Stage 2:

- store it if present;
- validate that it refers to a known active stage if possible;
- if missing, fall back to YAML `next_stage` for display/metadata;
- do not execute transitions.

---

# 6. SQLite schema updates required in Stage 2

Stage 2 must evolve the SQLite schema carefully.

Do not rely on deleting and recreating the database manually.

Add a small migration/bootstrap mechanism if necessary.

## 6.1. Required stage lifecycle columns

Update `stages` to include at least:

- `is_active INTEGER NOT NULL DEFAULT 1`
- `archived_at TEXT NULL`

Optional but useful:

- `last_seen_in_config_at TEXT`
- `config_hash TEXT`

Stage sync must mark missing YAML stages inactive instead of deleting them.

## 6.2. Required entity columns

Update or create `entities` so it can support Stage 2 discovery.

Minimum recommended fields:

- `entity_id TEXT PRIMARY KEY`
- `file_path TEXT NOT NULL`
- `file_name TEXT NOT NULL`
- `stage_id TEXT NOT NULL`
- `current_stage TEXT`
- `next_stage TEXT`
- `status TEXT NOT NULL`
- `checksum TEXT NOT NULL`
- `file_mtime TEXT NOT NULL`
- `file_size INTEGER NOT NULL`
- `payload_json TEXT`
- `meta_json TEXT`
- `validation_status TEXT NOT NULL`
- `validation_errors_json TEXT NOT NULL DEFAULT '[]'`
- `discovered_at TEXT NOT NULL`
- `updated_at TEXT NOT NULL`

Important:

- decide whether `entity_id` is globally unique across workdir;
- for this project, treat `entity_id` as globally unique across the workdir;
- if the same `entity_id` appears in multiple stage folders, do not silently overwrite without recording that the entity has multiple file locations or duplicated IDs.

## 6.3. Required entity stage state columns

Update or create `entity_stage_states` so it can support later runtime.

Minimum recommended fields:

- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `entity_id TEXT NOT NULL`
- `stage_id TEXT NOT NULL`
- `file_path TEXT NOT NULL`
- `status TEXT NOT NULL`
- `attempts INTEGER NOT NULL DEFAULT 0`
- `max_attempts INTEGER NOT NULL`
- `last_error TEXT`
- `last_http_status INTEGER`
- `next_retry_at TEXT`
- `last_started_at TEXT`
- `last_finished_at TEXT`
- `created_child_path TEXT`
- `discovered_at TEXT NOT NULL`
- `updated_at TEXT NOT NULL`

Add a unique constraint that prevents duplicate state rows for the same entity/stage/file identity.

Recommended:

```sql
UNIQUE(entity_id, stage_id, file_path)
```

If you choose a different uniqueness model, document why.

## 6.4. Invalid files and app events

Invalid JSON files should not disappear silently.

At minimum, record invalid discovery events in `app_events`.

Recommended app event fields already exist or should be compatible with:

- level;
- code/event_type;
- message;
- context_json;
- created_at.

For Stage 2, invalid file cases can be visible in Diagnostics or Workspace Explorer.

---

# 7. Migration/versioning requirements

Stage 1 uses `PRAGMA user_version = 1`.

Stage 2 must bump schema version to `2` if the schema changes.

Required:

- implement idempotent migration from schema version 1 to 2;
- support a fresh database created directly at schema version 2;
- do not require the user to manually delete `app.db`;
- tests must cover fresh DB bootstrap and existing DB migration if possible.

Minimum acceptable approach:

- read `PRAGMA user_version`;
- apply missing migrations sequentially;
- set `PRAGMA user_version = 2`;
- keep migration SQL in a clear module/function.

Do not build a heavy migration framework unless necessary.

---

# 8. File discovery requirements

## 8.1. Discovery scope

Stage 2 discovery must scan active stages only.

For each active stage:

- use `input_folder` from YAML/SQLite;
- resolve it relative to workdir;
- scan for JSON files in that folder.

For Stage 2, recursive scanning is optional. Choose one of these and document it:

### Preferred for Stage 2

Non-recursive scan of each stage input folder.

This is simpler and enough for the first runtime foundation.

### Optional

Recursive scan if implemented cleanly with clear limits.

Do not overbuild.

## 8.2. Discovery trigger

Implement manual discovery first.

Required UI action:

- a button such as `Scan workspace` or `Discover JSON files`.

Optional:

- automatic scan after opening/initializing workdir;
- but manual scan must still be available.

Do not implement continuous watcher in Stage 2 unless it is trivial and does not complicate the architecture.

## 8.3. File eligibility

A file is eligible for discovery if:

- it has `.json` extension;
- it is a regular file;
- it is inside an active stage input folder;
- it is readable;
- it parses as JSON;
- it satisfies minimum entity validation rules.

## 8.4. Partially written files

Stage 2 does not need a full file stability watcher.

However, avoid obviously unsafe behavior:

- if reading/parsing fails, record a validation/discovery error instead of crashing;
- do not treat unreadable or malformed files as pending;
- leave room in the design for future file stability checks.

---

# 9. Checksum and change tracking

Stage 2 must track file changes.

Minimum required tracking:

- file size;
- file modified timestamp;
- checksum/hash of file contents.

Recommended checksum:

- SHA-256.

If SHA-256 requires adding a dependency, that is acceptable.

Behavior:

- if a discovered file is new, insert it;
- if the same file was already registered and checksum/mtime changed, update its record;
- preserve `discovered_at` if reasonable;
- update `updated_at` when metadata changes;
- record changes in `app_events` if useful.

---

# 10. Duplicate entity ID handling

This is important.

Since entity IDs are intended to be globally unique in a workdir, Stage 2 must handle duplicates explicitly.

Scenarios:

1. Same `entity_id`, same file path, unchanged file:
   - no duplicate row;
   - idempotent re-scan.

2. Same `entity_id`, same file path, changed file:
   - update checksum/metadata;
   - keep one entity row.

3. Same `entity_id`, different file path:
   - do not silently overwrite;
   - record a duplicate entity issue;
   - decide whether to keep the first entity and log the second as invalid, or add a separate duplicate table/event.

Recommended Stage 2 behavior:

- keep `entities.entity_id` globally unique;
- if duplicate ID is found at a different path, record an `app_events` error and do not overwrite the existing entity;
- show duplicate-related issues somewhere in Diagnostics or Workspace Explorer.

Document and test the chosen behavior.

---

# 11. Entity registration behavior

When a valid JSON file is discovered:

1. parse the file;
2. extract `id`, `payload`, `current_stage`, `next_stage`, `meta`;
3. determine stage from the active stage folder;
4. compute checksum and file metadata;
5. insert or update `entities`;
6. create or update corresponding `entity_stage_states` row;
7. set initial state to `pending` unless validation requires another status;
8. update Dashboard/Entities UI.

Do not mutate the JSON file in Stage 2 unless explicitly needed to repair missing fields. The preferred behavior is **read-only discovery**.

---

# 12. UI requirements for Stage 2

Stage 2 must turn the placeholder runtime screens into useful views.

## 12.1. Dashboard

Update Dashboard to show:

- current project name;
- current workdir;
- config status;
- database status;
- number of active stages;
- number of inactive stages;
- total registered entities;
- entities by status;
- latest discovery timestamp if available;
- number of discovery errors.

## 12.2. Entities Table

Replace the placeholder with a real table.

Minimum columns:

- Entity ID;
- File name;
- Stage;
- Status;
- Validation status;
- File path;
- Checksum short form;
- Updated at;
- Errors indicator.

Minimum interactions:

- filter by stage;
- filter by status;
- filter by validation status;
- search by entity ID or filename;
- click row to open Entity Detail.

Do not add heavy UI libraries unless already planned or necessary. If using a basic table first is faster and clean, that is acceptable.

## 12.3. Entity Detail

Implement basic entity detail view:

- entity ID;
- file path;
- stage;
- status;
- checksum;
- file size;
- modified timestamp;
- validation errors;
- parsed JSON preview;
- associated stage state rows.

Read-only JSON preview is enough for Stage 2.

## 12.4. Stage Editor

Update Stage Editor so it shows:

- active/inactive stage status;
- stage input/output folders;
- workflow URL;
- retry settings;
- next stage;
- count of entities discovered for each active stage if available.

No full CRUD editor required in Stage 2.

## 12.5. Workspace Explorer

Improve Workspace Explorer enough to show:

- workdir root;
- active stage folders;
- JSON files discovered under each stage;
- invalid/unregistered JSON files if tracked.

A simple grouped list is acceptable. A fancy tree is not required yet.

## 12.6. Settings / Diagnostics

Add diagnostics for:

- schema version;
- active/inactive stage count;
- last scan/discovery time;
- discovery errors;
- app events related to invalid files or duplicates;
- workdir path validation state.

---

# 13. Required Tauri commands / backend API

Design the exact command names cleanly. Suggested commands:

- `scan_workspace(path: String) -> ScanResult`
- `list_entities(path: String, filters?) -> EntityListResult`
- `get_entity(path: String, entity_id: String) -> EntityDetailResult`
- `list_app_events(path: String, limit: u32) -> AppEventsResult`
- `get_stage_summary(path: String) -> StageSummaryResult`

You may choose different names, but they must be consistent, typed, and documented.

Command results should include structured errors where appropriate.

Avoid passing raw SQLite paths from the frontend if the backend can derive them from workdir.

---

# 14. Backend architecture expectations

Keep backend responsibilities separated.

Recommended modules:

```text
src-tauri/src/
  bootstrap/
  commands/
  config/
  database/
  discovery/
  domain/
  workdir/
```

Add `discovery` or similarly named module for scanning and registration.

Do not put scan logic inside Tauri command handlers directly.

Expected backend flow:

```text
command -> service/orchestration function -> discovery module -> database module -> typed result
```

---

# 15. Frontend architecture expectations

Keep frontend responsibilities separated.

Recommended areas:

```text
src/
  app/
  components/
  features/
    bootstrap/
    workdir/
    entities/
    discovery/
    stages/
  lib/
  pages/
  types/
```

Add typed API wrappers in `src/lib/` for new Tauri commands.

Avoid mixing `invoke(...)` calls directly inside page components when a wrapper would keep the code cleaner.

---

# 16. App events and diagnostics

Stage 2 should start using `app_events` more seriously.

Record events for:

- scan started;
- scan completed;
- invalid JSON file;
- missing required field;
- duplicate entity ID;
- stage sync deactivation;
- schema migration;
- unexpected read/parse/database errors.

Do not spam events for every successful file unless useful. Summary events are enough.

---

# 17. Testing requirements

Add tests for the new Stage 2 behavior.

## 17.1. Required backend tests

Add or update tests for:

- schema migration from v1 to v2;
- fresh DB bootstrap at v2;
- stage removed from YAML becomes inactive, not deleted;
- inactive stages are not scanned;
- valid JSON entity is registered;
- malformed JSON is recorded as discovery error and does not crash;
- JSON missing `id` is handled according to the documented rule;
- JSON missing `payload` is handled according to the documented rule;
- duplicate entity ID in different file paths is detected;
- re-scan is idempotent for unchanged files;
- re-scan updates checksum/mtime for changed files;
- workdir path canonicalization rejects nested paths.

## 17.2. Frontend/build checks

At minimum:

- TypeScript build passes;
- no type errors;
- UI can call scan and list entities commands;
- basic navigation still works.

## 17.3. Manual verification scenarios

Codex must personally verify:

1. fresh app launch;
2. open existing Stage 1 workdir;
3. initialize new workdir;
4. create valid JSON in active stage folder;
5. run manual scan;
6. verify entity appears in Entities Table;
7. open Entity Detail;
8. edit JSON file manually and rescan;
9. verify checksum/update timestamp changes;
10. create malformed JSON and verify error appears;
11. create JSON missing `id` and verify expected invalid behavior;
12. remove a stage from YAML and verify it becomes inactive in SQLite/UI;
13. verify inactive stage folder is not scanned;
14. verify `npm.cmd run build` passes;
15. verify Rust tests pass through the working Windows toolchain command.

Manual verification must be reported honestly.

Do not write “should work” or “not tested but implemented”.

Use:

- “verified manually”;
- “reproduced”;
- “failed, fixed, re-tested”;
- “not verified because ...” only if there is a real blocker.

---

# 18. Documentation updates

Update documentation after implementation.

Required docs:

- `README.md`
- Stage 2 delivery report under `docs/`
- Stage 2 progress log under `docs/`
- Stage 2 checklist under `docs/`
- optionally Stage 2 questions/resolutions under `docs/`

Recommended filenames:

```text
docs/codex_stage2_execution_plan.md
docs/codex_stage2_progress.md
docs/codex_stage2_instruction_checklist.md
docs/codex_stage2_delivery_report.md
docs/codex_stage2_questions.md
```

Delivery report must include:

- what was implemented;
- files changed;
- schema changes;
- stage sync lifecycle decision;
- workdir path validation decision;
- scan behavior;
- entity registration behavior;
- tests added;
- manual verification results;
- known limitations;
- what remains for Stage 3.

---

# 19. README requirements

Update README to explain:

- Stage 2 scope;
- how to run the app;
- how to run tests;
- workdir path requirements;
- expected `pipeline.yaml` structure;
- where JSON files should be placed;
- how to scan workspace;
- how entities appear in UI;
- what is intentionally deferred.

---

# 20. `.gitignore` requirements

Review `.gitignore` again after Stage 2.

Ensure generated runtime artifacts are ignored:

- `*.db`
- `*.db-shm`
- `*.db-wal`
- `*.sqlite`
- `*.sqlite3`
- `test-workdirs/`
- `tmp/`
- logs
- generated workdirs

Do not ignore source fixtures if you add test fixtures intentionally.

Recommended pattern:

- ignore runtime workdirs;
- keep small static test fixtures if they are used by tests.

---

# 21. Performance expectations for Stage 2

Stage 2 does not need to handle 10,000 files perfectly yet, but the design must not be obviously hostile to that future.

Minimum expectations:

- scanning 100–200 JSON files should feel instant or near-instant;
- re-scan should be idempotent;
- database inserts/updates should use transactions;
- do not open/close a SQLite connection per file if avoidable;
- avoid doing all expensive work in React.

Nice to have:

- scan summary including number of scanned, registered, updated, invalid, duplicate files;
- basic elapsed time in scan result.

---

# 22. Error handling expectations

Errors must be structured and visible.

Do not let file parsing, invalid JSON, duplicate IDs, or DB sync errors crash the app.

Every meaningful error should be either:

- returned in a command result;
- stored in `app_events`;
- displayed in Diagnostics or entity-related UI.

---

# 23. Security and data safety expectations

Stage 2 must not send data to n8n or any external system.

Stage 2 must not mutate user JSON files by default.

Stage 2 must not delete user JSON files.

Stage 2 must not move files between folders.

All discovery behavior should be read-only toward JSON files.

Only SQLite and internal app metadata should be updated.

---

# 24. Suggested implementation phases for Codex

Follow these phases unless you have a better reasoned plan.

## Phase 1 — Re-read and document plan

- Read this task.
- Re-read Stage 1 docs and current code.
- Create `docs/codex_stage2_execution_plan.md`.
- Create/update Stage 2 checklist.

## Phase 2 — Fix Stage 1 carry-over issues

- Add stage lifecycle fields.
- Replace hard delete with deactivate/archive behavior.
- Improve canonical workdir validation.
- Add tests.

## Phase 3 — Schema v2 migration

- Add migration logic.
- Update schema bootstrap.
- Update tests for fresh v2 DB and v1-to-v2 migration.

## Phase 4 — Discovery backend

- Implement file scanning for active stage input folders.
- Parse JSON files.
- Validate entity minimum requirements.
- Compute checksum and file metadata.
- Register/update entities.
- Create/update entity stage states.
- Record invalid file events.

## Phase 5 — Tauri commands and frontend API

- Add typed backend commands.
- Add frontend wrappers.
- Add state handling for scan results and entity list.

## Phase 6 — UI implementation

- Dashboard counts.
- Entities Table.
- Entity Detail.
- Stage Editor active/inactive info.
- Workspace Explorer grouped stage/file display.
- Diagnostics app events and scan state.

## Phase 7 — Verification and docs

- Run frontend build.
- Run Rust tests.
- Run manual verification scenarios.
- Update README and Stage 2 docs.
- Produce delivery report.

---

# 25. Acceptance criteria

Stage 2 is complete only if all of the following are true:

## Stage lifecycle

- removed YAML stages become inactive, not hard-deleted;
- active/inactive stage behavior is tested;
- inactive stages are not scanned.

## Workdir validation

- absolute path requirement remains;
- nested-under-app paths are rejected using stronger normalization/canonicalization;
- path validation edge cases are tested.

## Discovery

- manual scan exists;
- active stage folders are scanned;
- valid JSON files are registered as entities;
- invalid JSON files are handled without crashing;
- duplicate entity IDs are handled explicitly;
- re-scan is idempotent.

## Database

- schema version is bumped to 2;
- fresh DB bootstrap works;
- v1-to-v2 migration works or is explicitly supported by bootstrap logic;
- entity and state records are stored correctly.

## UI

- Dashboard shows meaningful runtime counts;
- Entities Table shows real registered entities;
- Entity Detail shows real entity data;
- Stage Editor shows active/inactive stages;
- Diagnostics shows useful scan/discovery information.

## Tests and verification

- `npm.cmd run build` passes;
- Rust tests pass;
- manual verification scenarios are completed and documented;
- docs are updated.

---

# 26. What will be considered a bad Stage 2 implementation

Do not do any of the following:

- implement n8n calls before entity discovery is reliable;
- keep hard-deleting stages after entities/states exist;
- silently overwrite duplicate entity IDs;
- store all discovered entities only in React state;
- skip SQLite migration and require manual DB deletion;
- hide invalid JSON files without diagnostics;
- scan inactive stages;
- mutate JSON files during discovery without explicit requirement;
- mix scan/database/UI code into one giant file;
- report manual verification that was not actually performed.

---

# 27. Required final response format from Codex

When finished, return your result in exactly this structure:

## A. What was implemented

Clear bullet list.

## B. Files changed

Group by backend, frontend, docs, config.

## C. Stage lifecycle decision

Explain how removed YAML stages are handled and why.

## D. Workdir validation decision

Explain how path validation now works.

## E. SQLite schema and migration

Explain schema version, new columns/tables, and migration behavior.

## F. Discovery and entity registration behavior

Explain scan scope, valid/invalid files, duplicates, checksum, re-scan behavior.

## G. UI changes

Explain what screens now show real data.

## H. Tests added/updated

List backend/frontend tests and what they cover.

## I. Manual verification performed

List exact scenarios personally verified.

## J. Known limitations

List what remains intentionally out of scope.

## K. Remaining blockers for Stage 2

Say clearly: none, or list exact blockers.

---

# 28. Final instruction

Implement Stage 2 as a runtime foundation, not as a workflow executor.

The goal is to make beehive safely know:

- which stages exist;
- which stages are active;
- which JSON files exist;
- which entities are valid;
- which files need operator attention;
- what is ready for future processing.

Only after this foundation is solid should the project move to n8n execution and retry runtime.

