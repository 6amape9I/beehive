# beehive

beehive is a local desktop operator tool for JSON stage pipelines.

Stage 9 prepares the app for demo and first internal use with a resettable demo workdir, one-action launch scripts, multi-output n8n support, release verification, and operator documentation.

`eligible stage state -> queued -> in_progress -> n8n webhook -> stage_runs -> done/retry_wait/failed -> next-stage file`

S3 mode is now storage-aware: Beehive sends n8n only technical S3 pointers, validates the returned artifact manifest, and stores S3 pointer rows in SQLite. Business JSON stays in S3; Beehive keeps control-plane state, routing, attempts, lineage, and operator visibility.

## Current S3 Production Contract

Current S3 production contract: JSON body envelope, headers deprecated.

Beehive calls n8n with `Content-Type: application/json; charset=utf-8` and a technical `beehive.s3_control_envelope.v1` body. The envelope contains the claimed source bucket/key, source entity/artifact ids, `run_id`, `stage_id`, `manifest_prefix`, `workspace_prefix`, `target_prefix`, and `save_path`.

The body is control-plane metadata only. Beehive must not send business JSON, source document text, `payload_json`, content blocks, or `raw_article` in the webhook request.

S3 object keys must be read from the JSON body fields `source_bucket` and `source_key`. The old empty-body plus `X-Beehive-*` header pointer mode is deprecated and must not be used for production S3 source keys.

Real S3 smoke tests are opt-in:

```bash
cargo test --manifest-path src-tauri/Cargo.toml real_s3_n8n_smoke_one_artifact -- --ignored --nocapture
BEEHIVE_REAL_S3_BATCH_SMOKE=1 BEEHIVE_SMOKE_BATCH_LIMIT=3 cargo test --manifest-path src-tauri/Cargo.toml real_s3_n8n_smoke_batch_small -- --ignored --nocapture
```

The batch smoke defaults to 3 artifacts and clamps the limit to 3-5. Set `BEEHIVE_SMOKE_BATCH_LIMIT=5` to run 5.

The batch smoke writes `/tmp/beehive_s3_batch_smoke_workdir/batch_smoke_report.json` with run ids, source keys, output keys, final states, and S3 output existence.

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
npm.cmd run app
```

On Windows, Rust/Tauri builds require Visual Studio C++ tools and a Windows SDK.

If the shell cannot find `link.exe` or `kernel32.lib`, run from a Developer Command Prompt or load MSVC/SDK paths with `vcvars64.bat`.

## Demo

```powershell
npm.cmd run demo:reset
npm.cmd run app
```

Then open `demo/workdir` in the app.

Useful Stage 9 scripts:

- `npm.cmd run app` starts the desktop app in dev/demo mode.
- `npm.cmd run demo:reset` restores the demo workdir baseline.
- `npm.cmd run demo` resets demo and starts the app.
- `npm.cmd run demo:generate -- --count 1000` generates volume demo files for manual load testing.
- `npm.cmd run verify` runs local verification helper commands.
- `npm.cmd run release` runs the Tauri release build.

Operator docs:

- [User Guide](docs/user_guide.md)
- [Demo Guide](docs/demo_guide.md)
- [Release Checklist](docs/release_checklist.md)
- [Stage 9 Manual QA Checklist](docs/stage9_manual_qa_checklist.md)

## Technical Verification

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'
npm.cmd run build
npm.cmd run release
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
- `input_folder` for local stages, or `input_uri` for S3 stages
- `output_folder` when `next_stage` is configured
- `workflow_url`
- `max_attempts`
- `retry_delay_sec`
- optional `next_stage`
- optional `save_path_aliases`
- optional `allow_empty_outputs`

Terminal stages, where `next_stage` is absent or empty, may omit `output_folder`. Internally this is normalized to an empty string in the current v6 schema and displayed as not required.

In S3 mode, `allow_empty_outputs` defaults to `false`. A success manifest with zero outputs is accepted only when the source stage explicitly sets `allow_empty_outputs: true`.

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
- S3 stages may use empty `input_folder` when `input_uri` is configured;
- S3 config fields `input_uri`, `save_path_aliases`, and `allow_empty_outputs` are preserved by draft validation and save.

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
- valid JSON object or root array response;
- response `success` is not `false` when using wrapper object form;
- when a next stage exists, response output is one or more JSON payload objects.

On success:

- source stage state becomes `done`;
- source JSON file is not mutated;
- every n8n output object is wrapped into a child beehive JSON file in the target stage;
- child ids are deterministic and collision-safe;
- target child stage states are `pending`.

Supported Stage 9 response forms:

```json
[
  { "entity_name": "child one" },
  { "entity_name": "child two" }
]
```

```json
{
  "success": true,
  "payload": [
    { "entity_name": "child one" },
    { "entity_name": "child two" }
  ],
  "meta": {}
}
```

The older `{ "success": true, "payload": { ... } }` form is still accepted as one output item.

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

The Entities table is backed by SQLite pagination, filtering, search, and sorting. It shows a lightweight business name from the latest registered file payload for demo/operator recognition, and search covers entity id, file path/name, and that payload text. It does not run n8n or mutate runtime state; `Scan workspace` is still an explicit operator action.

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

Workspace Explorer reads SQLite and selected folder metadata, can run S3 reconciliation, can manually register one S3 source artifact, and can run a small due-task batch. It does not edit JSON, move files, edit `pipeline.yaml`, reconcile stuck tasks, or scan automatically. `Scan workspace`, `Reconcile S3`, and `Run small batch` remain explicit operator actions.

Currently deferred: live display of present but unregistered JSON files before a scan. Run `Scan workspace` to register or report those files through the normal reconciliation path.

## UI

The app surfaces:

- Dashboard: Stage 5 overview, stage graph, counters, active tasks, recent errors/runs, and manual operational actions
- Entities: server-side paginated/sorted/filterable logical entity rows with attempts and last-error context
- Entity Detail: all file instances, stage timeline, allowed manual actions, safe payload/meta JSON editor, open file/folder actions, and stage run history
- Workspace Explorer: stage tree, registered files, missing/invalid files, managed copies, entity trails, S3 pointer metadata, S3 reconciliation, manual S3 source registration, and small batch execution
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
- config repair mode
