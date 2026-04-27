# beehive

beehive is a local desktop operator tool for JSON stage pipelines.

Stage 5.5 stabilizes the Stage 1-5 foundation with a formal runtime state machine, atomic task claiming, file-stability guards, and terminal-stage handling.
Stage 6 adds the operator entity table and entity detail workflow on top of that foundation.

`eligible stage state -> queued -> in_progress -> n8n webhook -> stage_runs -> done/retry_wait/failed -> next-stage file`

## Stack

- Tauri v2
- React
- TypeScript
- Rust
- SQLite through `rusqlite`
- YAML through `serde_yaml`
- HTTP execution through `reqwest`

## Install

```powershell
npm.cmd install
```

In this PowerShell environment use `npm.cmd`, not `npm`.

## Run

```powershell
npm.cmd run tauri dev
```

On Windows, Rust/Tauri builds require Visual Studio C++ tools and a Windows SDK.

If the shell cannot find `link.exe` or `kernel32.lib`, run from a Developer Command Prompt or load MSVC/SDK paths with `vcvars64.bat`.

## Technical Verification

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'
npm.cmd run build
```

## Workdir Rules

Use an absolute path outside the application directory.

- relative paths are rejected;
- nested paths inside the app/runtime directory are rejected;
- path validation uses normalized/canonical checks to avoid disguised `..` traversal or case-mismatch issues.

Initializing a new workdir creates:

```text
pipeline.yaml
app.db
stages/
logs/
```

## Stage 4 Runtime Model

The app keeps the Stage 3 logical entity/file-instance model:

- `entities` = logical aggregate by `entity_id`
- `entity_files` = physical JSON file instances in stage folders
- `entity_stage_states` = logical per-stage runtime state linked to the latest relevant file instance
- `stage_runs` = audit history for n8n execution attempts

This allows one logical entity to exist in multiple stage folders at once:

```text
stages/incoming/entity-001.json
stages/normalized/entity-001.json
stages/enriched/entity-001.json
```

Still invalid:

- the same `entity_id` twice in the same stage;
- a registered file path changing to a different `entity_id`;
- destructive overwrite of an existing target file during managed copy.

## `pipeline.yaml`

The app expects `pipeline.yaml` with:

- `project`
- `runtime`
- `stages`

Each stage should define:

- `id`
- `input_folder`
- `output_folder` when `next_stage` is configured
- `workflow_url`
- `max_attempts`
- `retry_delay_sec`
- optional `next_stage`

Terminal stages, where `next_stage` is absent or empty, may omit `output_folder`. Internally this is normalized to an empty string in the current v4 schema and displayed as not required.

Runtime also supports:

- `runtime.request_timeout_sec` with default `30`
- `runtime.file_stability_delay_ms` with default `1000`

`workflow_url` must come from `pipeline.yaml`. Do not hardcode real n8n webhook URLs into application code. The known development n8n production webhook may be used manually or in docs as an example, but automated tests use local mock HTTP servers.

## Stage Editor

Stage 7 adds an operator UI for editing `pipeline.yaml` from the desktop app.

The editor uses a draft workflow:

```text
load pipeline.yaml -> edit draft -> validate -> save atomically -> sync SQLite stages
```

Save behavior:

- no YAML writes happen on keystroke;
- invalid drafts are rejected before write;
- the previous `pipeline.yaml` is moved to a timestamped backup;
- the new YAML is written through a same-directory temp file and rename;
- SQLite `stages` is synchronized after save;
- missing active stage input/output directories are provisioned;
- `pipeline_config_saved` is recorded in app events.

Stage rules:

- saved stage IDs are immutable in Stage 7;
- new draft stage IDs are editable until saved;
- removing a stage removes it from active YAML config only;
- removed stages become inactive/archived in SQLite;
- historical entity files, stage states, and stage runs are not deleted;
- terminal stages may omit `output_folder`;
- non-terminal stages with `next_stage` require `output_folder`.

Stage Editor does not call n8n, manage n8n workflows, move JSON files, delete runtime history, or provide a visual graph builder.

## JSON File Requirements

Eligible files must be:

- `.json` files, case-insensitive;
- regular readable files;
- inside an active stage input folder;
- JSON objects at the root;
- with non-empty `id`;
- with non-null `payload`.

Example:

```json
{
  "id": "entity-0001",
  "current_stage": "incoming",
  "next_stage": "normalized",
  "status": "pending",
  "payload": {
    "value": 1
  },
  "meta": {
    "source": "manual"
  }
}
```

## Reconciliation Scan

`Scan workspace` now performs reconciliation, not only discovery.

It:

1. loads active stages;
2. provisions missing input/output directories;
3. skips files that are too fresh or change while being read;
4. scans active input folders non-recursively;
5. registers new file instances;
6. updates changed file instances;
7. marks missing files instead of deleting rows;
8. restores missing files if they reappear;
9. recomputes logical entity summaries;
10. records file lifecycle app events.

Inactive stages are not scanned for new files, but their historical rows remain visible.
Fresh or changing files are recorded as `unstable_file_skipped`, not as permanent invalid JSON.

## Managed Next-Stage Copy

Stage 3 adds a safe backend-managed copy operation for future runtime use.

Behavior:

- source file stays unchanged;
- target stage is resolved from file `next_stage`, then stage config `next_stage`;
- target JSON is updated in memory before write;
- write uses a temporary file plus rename for atomicity;
- destructive overwrite is not allowed.

Managed copy updates target JSON fields:

- `current_stage`
- `status`
- `next_stage`
- `meta.updated_at`
- `meta.beehive.copy_source_stage`
- `meta.beehive.copy_target_stage`
- `meta.beehive.copy_created_at`

## n8n Execution

`Run due tasks` processes a bounded batch of eligible states. For Stage 4, `runtime.max_parallel_tasks` means the maximum number of tasks processed per manual run, not true parallel execution.

`Run this stage` / `run_entity_stage` is a manual debug path. It may execute a `retry_wait` state even when `next_retry_at` is still in the future. The production-like `Run due tasks` path stays strict and only executes `pending` or due `retry_wait` states.

Stage 5.5 routes runtime state changes through a formal state machine. SQLite `entity_stage_states` is the runtime source of truth; source JSON remains business input and is not mutated to mirror execution state.

Before execution, a task is atomically claimed in SQLite by moving from `pending` or due `retry_wait` to `queued`. Only successfully claimed rows can run. Stage-run audit creation and `queued -> in_progress` are committed together in one SQLite transaction before HTTP is sent. Stale `queued` claims are released back to `pending` without incrementing attempts; any legacy unfinished run created before start is marked reconciled with `error_type = claim_recovered_before_start`.

Before HTTP is sent, the registered source file is checked again. If it is missing, too fresh, changed during read, or no longer matches the DB snapshot, no HTTP request is sent, no `stage_runs` row is created, and attempts are not incremented. Run `Scan workspace` after the file is stable.

Execution sends:

- `POST`
- `Content-Type: application/json`
- `Accept: application/json`
- source payload and meta
- beehive execution metadata with `entity_id`, `stage_id`, `entity_file_id`, `attempt`, and `run_id`

Success requires:

- HTTP 2xx;
- valid JSON object response;
- response `success` is not `false`;
- response `payload` is an object when a next stage exists.

On success:

- source stage state becomes `done`;
- source JSON file is not mutated;
- target file is created from response `payload`;
- target stage state is `pending`.

For terminal stages with no `next_stage`, success marks the source state `done` and does not create a target file.

Reconciliation scans do not overwrite SQLite execution state from source JSON. For example, if a successful run leaves the source JSON with `"status": "pending"`, the SQLite stage state remains `done` after the next scan.

## Entity Detail JSON Editing

Stage 6 exposes a read/write editor for business JSON only:

- editable fields are `payload` and `meta`;
- `id`, runtime status, attempts, retry fields, and stage state are not edited through JSON;
- SQLite `entity_stage_states` remains the runtime source of truth.

The backend disables business JSON edits when the related runtime state is active or complete:

- locked: `queued`, `in_progress`, `done`;
- editable: `pending`, `retry_wait`, `failed`, `blocked`, `skipped`.

If a file has no matching runtime stage state, run `Scan workspace` before editing. Completed artifacts should not be edited silently; use an explicit reset/manual workflow when correction is required.

On failure:

- attempts are recorded in `stage_runs`;
- state becomes `retry_wait` if attempts remain;
- state becomes `failed` when max attempts are exhausted.

If HTTP succeeds but next-stage copy is structurally impossible, such as a missing or inactive target stage, the source state becomes `blocked`; the stage run is recorded as unsuccessful with `error_type = copy_blocked`; no retry is scheduled.

Stuck `in_progress` states older than `runtime.stuck_task_timeout_sec` are reconciled before each manual run. Retryable stuck tasks become `retry_wait` with an immediately due `next_retry_at`; exhausted stuck tasks become `failed`.

## Dashboard Overview

Stage 5 adds a read-only operator dashboard backed by a single SQLite read model.

The Dashboard shows:

- project/workdir context and last scan/run timestamps;
- summary cards for entities, stages, due tasks, in-progress, retry, failed, blocked, and errors;
- a static stage graph with active/inactive stages and invalid missing/inactive links;
- per-stage counters from `entity_stage_states`;
- active tasks, recent warning/error events, and recent stage runs.

Dashboard refresh is read-only. It does not scan files, run n8n tasks, or mutate execution state automatically.

Operator actions remain manual:

- `Refresh` reloads the overview only;
- `Scan workspace` runs reconciliation and then reloads the overview;
- `Run due tasks` runs eligible tasks and then reloads the overview;
- `Reconcile stuck` reconciles stale in-progress tasks and then reloads the overview.

## Entity Operations

Stage 6 provides an operator-oriented Entities table and Entity Detail page.

The Entities table is backed by SQLite pagination, filtering, search, and sorting. It does not load JSON payloads for table rows and it does not scan files, run n8n, or mutate runtime state.

Entity Detail shows:

- the logical entity summary;
- all physical file instances;
- stage timeline and state rows;
- stage run history with request/response snapshots;
- selected file JSON;
- backend-computed allowed manual actions.

Manual entity actions are backend mediated:

- `Retry now` is available for `pending` and `retry_wait`; it may bypass future `next_retry_at` only through the manual/debug path.
- `Reset to pending` is available for `failed`, `blocked`, `skipped`, and `retry_wait`; it clears retry/error fields and keeps `stage_runs`.
- `Skip` is available for `pending` and `retry_wait`; it does not call n8n, create a next-stage file, or advance the entity.

All manual runtime changes pass through the state machine and write `app_events`.

Open file/folder actions resolve registered entity file ids through the backend and reject unsafe paths. JSON editing is intentionally scoped to business `payload` and `meta`; `id`, runtime status, attempts, retry fields, and SQLite execution state are not editable through the JSON editor. Saves verify that the on-disk file still matches the DB snapshot before using an atomic temp-file rename.

## Workspace Explorer

Stage 8 expands Workspace Explorer into a read-only file tree and artifact-connectivity view.

It shows:

- workdir and stage folder context;
- active and inactive stages;
- registered JSON files from SQLite `entity_files`;
- runtime status from SQLite `entity_stage_states`, not JSON `status`;
- present and missing tracked files;
- invalid files recorded by the latest manual scan;
- managed copies and source-file relationships when known;
- entity trails across stage folders;
- safe `Open file` and `Open folder` actions through registered entity file ids;
- deep links to Entity Detail with `file_id` selected.

Workspace Explorer reads SQLite and selected folder metadata only. It does not edit JSON, move files, edit `pipeline.yaml`, run n8n, reconcile stuck tasks, or scan automatically. `Scan workspace` remains an explicit operator action.

Currently deferred: live display of present but unregistered JSON files before a scan. Run `Scan workspace` to register or report those files through the normal reconciliation path.

## UI

The app surfaces:

- Dashboard: Stage 5 overview, stage graph, counters, active tasks, recent errors/runs, and manual operational actions
- Entities: server-side paginated/sorted/filterable logical entity rows with attempts and last-error context
- Entity Detail: all file instances, stage timeline, allowed manual actions, safe payload/meta JSON editor, open file/folder actions, and stage run history
- Workspace Explorer: read-only stage tree, registered files, missing/invalid files, managed copies, and entity trails
- Settings / Diagnostics: schema v4, reconciliation summary, file lifecycle and execution events

## Intentionally Deferred

Stage 4 does not implement:

- background daemon
- n8n workflow management through the n8n REST API
- credential manager or authentication UI
- background watcher
- complex branch routing
- n8n execution polling
- rich JSON diff/version history or low-code editor
- advanced UI polish
