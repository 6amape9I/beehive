# Worker Operations Runbook

## Scope

This runbook covers the DB-backed worker model introduced in B12 and hardened in B13.

Workers are disabled by default. They should process only explicitly scoped workspaces.

## Safe Startup

Start workers on a test workspace first:

```bash
BEEHIVE_WORKERS_ENABLED=1 \
BEEHIVE_WORKER_WORKSPACES=test_worker_pilot \
BEEHIVE_WORKER_DEFAULT_CONCURRENCY=5 \
BEEHIVE_WORKER_LOCAL_LLM_CONCURRENCY=1 \
cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
```

Optional tuning:

```bash
BEEHIVE_WORKER_IDLE_SLEEP_MS=1000
BEEHIVE_WORKER_RECOVERY_INTERVAL_SEC=30
```

Effective worker concurrency is capped by `runtime.worker_pools.*.concurrency` in the workspace `pipeline.yaml`.

## Production Workspace Warning

Do not use `BEEHIVE_WORKER_WORKSPACES=all` for production pilot.

Use `BEEHIVE_WORKER_WORKSPACES=itg_documents` only when ready, after a small pilot has passed and the operator has confirmed concurrency, workflow endpoints, and recovery behavior.

Start conservatively for `itg_documents`:

```bash
BEEHIVE_WORKERS_ENABLED=1 \
BEEHIVE_WORKER_WORKSPACES=itg_documents \
BEEHIVE_WORKER_DEFAULT_CONCURRENCY=3 \
BEEHIVE_WORKER_LOCAL_LLM_CONCURRENCY=1 \
cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
```

Do not run a large 22k production pass as a smoke test.

## Broad Runs

When workers are enabled for a workspace, broad manual run actions are disabled:

- `run_small_batch`
- `run_pipeline_waves`

The expected error code is:

```text
workers_enabled_broad_run_disabled
```

Use selected pipeline waves only for targeted operator debugging. Selected work still refuses roots with active worker leases.

## Pause And Resume

Use the Workers & Queue panel or HTTP API:

```text
POST /api/workspaces/{workspace_id}/workers/pause
POST /api/workspaces/{workspace_id}/workers/resume
POST /api/workspaces/{workspace_id}/workers/pools/default/pause
POST /api/workspaces/{workspace_id}/workers/pools/default/resume
POST /api/workspaces/{workspace_id}/workers/pools/local_llm/pause
POST /api/workspaces/{workspace_id}/workers/pools/local_llm/resume
```

Pause body:

```json
{
  "reason": "manual maintenance"
}
```

Paused pools do not claim new tasks. Already running leases continue.

## Queue And Lease Inspection

Use:

```text
GET /api/workspaces/{workspace_id}/workers/summary
```

The summary reports per-pool:

- configured concurrency
- active and expired leases
- pending, retry-wait due, retry-wait not due, queued, in-progress, blocked, and failed counts
- pause state and reason
- oldest pending age
- average duration if available
- last error

The UI shows the same data in `Workers & Queue`.

## Recovery

Recover expired active leases:

```text
POST /api/workspaces/{workspace_id}/workers/recover-expired-leases
```

Recovery returns queued states to `pending`, and in-progress states to `retry_wait` or `failed` according to attempts.

## Manual Lease Release

Use manual release only for anomalous active leases where the attached stage run already finished:

```text
POST /api/workspaces/{workspace_id}/workers/leases/{lease_id}/release
```

Body:

```json
{
  "reason": "manual_release_after_finished_run"
}
```

B13 does not expose dangerous force release. Fresh active unfinished leases are rejected. Expired unfinished work should go through `recover-expired-leases`.

## SQLite Runtime Settings

Every writable workspace DB connection applies:

```sql
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
```

Readonly connections apply `foreign_keys` and `busy_timeout`.

## Pilot Checklist

Before a production pilot:

- Use a test workspace with 20-100 files.
- Use default concurrency 3-5 and local LLM concurrency 1.
- Confirm no unexpected real S3/n8n calls happen.
- Confirm active local LLM leases never exceed 1.
- Confirm pause/resume works for each pool.
- Confirm expired recovery returns states to a runnable status.
- Confirm broad manual runs return `workers_enabled_broad_run_disabled`.
