# beehive

beehive is a local desktop operator tool for JSON stage pipelines.

Stage 4 turns the Stage 3 file lifecycle foundation into a manually triggered execution foundation:

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
- `output_folder`
- `workflow_url`
- `max_attempts`
- `retry_delay_sec`
- optional `next_stage`

Runtime also supports:

- `runtime.request_timeout_sec` with default `30`

`workflow_url` must come from `pipeline.yaml`. Do not hardcode real n8n webhook URLs into application code. The known development n8n production webhook may be used manually or in docs as an example, but automated tests use local mock HTTP servers.

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
3. scans active input folders non-recursively;
4. registers new file instances;
5. updates changed file instances;
6. marks missing files instead of deleting rows;
7. restores missing files if they reappear;
8. recomputes logical entity summaries;
9. records file lifecycle app events.

Inactive stages are not scanned for new files, but their historical rows remain visible.

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

Reconciliation scans do not overwrite SQLite execution state from source JSON. For example, if a successful run leaves the source JSON with `"status": "pending"`, the SQLite stage state remains `done` after the next scan.

On failure:

- attempts are recorded in `stage_runs`;
- state becomes `retry_wait` if attempts remain;
- state becomes `failed` when max attempts are exhausted.

If HTTP succeeds but next-stage copy is structurally impossible, such as a missing or inactive target stage, the source state becomes `blocked`; the stage run is recorded as unsuccessful with `error_type = copy_blocked`; no retry is scheduled.

Stuck `in_progress` states older than `runtime.stuck_task_timeout_sec` are reconciled before each manual run. Retryable stuck tasks become `retry_wait` with an immediately due `next_retry_at`; exhausted stuck tasks become `failed`.

## UI

Stage 4 surfaces:

- Dashboard: reconciliation summary, present/missing files, managed copy count, execution status counts, Run due tasks
- Entities: logical entity rows
- Entity Detail: all file instances plus stage states, managed copy action, stage run history, Run this stage
- Workspace Explorer: present/missing/invalid/managed-copy file visibility
- Settings / Diagnostics: schema v4, reconciliation summary, file lifecycle and execution events

## Intentionally Deferred

Stage 4 does not implement:

- background daemon
- n8n workflow management through the n8n REST API
- credential manager or authentication UI
- background watcher
- complex branch routing
- n8n execution polling
- full JSON editor
- advanced UI polish
