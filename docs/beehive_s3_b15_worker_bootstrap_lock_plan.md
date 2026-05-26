# B15 Worker Bootstrap Lock Fix Plan

## 1. Current Call Path

`services::workers::worker_loop` currently calls:

```text
runtime::load_workspace_context(workspace_id)
```

on every loop iteration.

`runtime::load_workspace_context` resolves the registered workspace, reads `pipeline.yaml`, parses it, and then calls:

```text
database::bootstrap_database(&workspace.database_path, &config)
```

`bootstrap_database` opens SQLite, ensures schema, and syncs configured stages through `sync_stages`. Stage sync upserts stage rows, archives removed stages, and writes events/settings. This is correct for explicit bootstrap/admin paths, but it is a write-heavy operation and can hit `database is locked` under worker concurrency.

## 2. Root Problem

Workers need a read-mostly runtime context before claim. They do not need to upsert stages on every idle/claim loop.

Current repeated worker bootstrap can fail before:

```text
claim_worker_runtime_tasks
worker_leases insert
n8n webhook call
```

Observed failure:

```text
Failed to upsert stage 'stage_0': database is locked
```

## 3. Context Split

Keep the existing heavy function:

```text
runtime::load_workspace_context
```

for workspace open/admin/reconcile/stage mutation paths that need bootstrap/sync.

Add a lightweight worker function:

```text
runtime::load_worker_runtime_context
```

This function will:

- read workspace registry;
- resolve `workdir_path`;
- verify `pipeline.yaml` exists;
- parse `pipeline.yaml`;
- return `workdir_path`, `database_path`, and config;
- verify the SQLite DB exists and has the current schema;
- not call `database::bootstrap_database`;
- not call `sync_stages`;
- return `workspace_not_bootstrapped_for_workers` if the DB/schema is missing.

## 4. Schema Check Without Stage Sync

Add a database helper such as:

```text
database::verify_worker_runtime_database
```

It should open the DB read-only and check `PRAGMA user_version`.

If the DB is missing or schema is too old, worker context returns a clear error. Heavy explicit bootstrap paths remain responsible for creating/migrating/syncing the DB.

## 5. Worker Changes

`start_workspace_workers` may keep one heavy context load before spawning loops. This bootstraps/syncs once at server startup for the workspace.

`worker_loop` will switch to `runtime::load_worker_runtime_context`, so normal loop iterations no longer upsert stages.

Worker summary/control endpoints will use the lightweight context where safe:

- summary
- start/stop desired state
- update desired pool
- pause/resume
- recover expired leases
- release lease

These endpoints operate on existing DB state and should not resync stages on every refresh.

## 6. Logging

Add structured worker lifecycle logs:

- `worker_loop_started`
- `worker_context_loaded`
- `worker_context_error`
- `worker_claim_idle`
- `worker_claimed_task`
- `worker_task_started`
- `worker_task_finished`

Idle/context-loaded logs should be rate-limited to avoid one log line per second forever.

## 7. Tests

Tests will prove observable behavior:

- heavy context still bootstraps/syncs stage rows;
- lightweight worker context does not change stage `updated_at`;
- lightweight context returns `workspace_not_bootstrapped_for_workers` for a missing DB;
- worker summary/control endpoints do not change stage `updated_at`;
- worker claim can operate from a pre-bootstrapped DB through lightweight context.

If direct worker-loop testing is too invasive because it is an infinite loop, test the extracted one-iteration helper or the lightweight context plus `run_worker_task` path.

## 8. Smoke

Use a small temporary workspace and mock webhook only. Do not run full `itg_documents`.

Smoke target:

```text
start server with workers enabled for test workspace
start workers through API
observe worker_claimed_task / worker_task_started
mock webhook receives at least one request
no stage upsert lock error
```
