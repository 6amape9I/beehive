# Worker Runtime Control Runbook

## Scope

B14 keeps the server-level worker supervisor as the safety guard and moves per-workspace runtime control into SQLite.

Server bootstrap still decides which worker loops exist. The web UI decides whether a workspace/pool may claim work and how many active leases are desired.

## Start Workers

1. Start `beehive-server` with workers enabled and scoped to explicit workspace ids or `all`.
2. Open the workspace in the web UI.
3. In `Workers & Queue`, set:
   - Default workers
   - Local LLM workers
4. Press `Start workers`.

Start writes `worker_pool_controls.is_started = true` and `desired_concurrency` for both pools. It does not create unlimited threads. The actual ceiling is:

```text
min(server env max, runtime.worker_pools.*.concurrency, UI desired concurrency)
```

## Stop Workers

Press `Stop workers`.

Stop writes `is_started = false` for both pools. Running n8n/S3 executions are not force-killed in B14. Existing active leases finish normally, and the summary can show `draining` until active leases reach zero.

## Pause And Resume

Pause/resume is per pool or all pools.

Pause is for temporary maintenance. It prevents new claims for that pool but does not alter desired concurrency. Resume allows claims again if the pool is still started.

## Scheduling

`pipeline.yaml` supports:

```yaml
runtime:
  scheduling_policy: depth_first
```

Allowed values:

- `depth_first`
- `fifo`

`depth_first` prefers pending child artifacts with `producer_run_id` and newer state timestamps. This moves a subset of entities deeper through the pipeline sooner.

`fifo` prefers older pending work. Use it for bulk completion if depth-first appears to starve older source files.

Both policies place due `retry_wait` behind normal `pending` work.

## Retry Demotion

Transient execution failures go to `retry_wait` while attempts remain. Due retry rows are eligible only after `next_retry_at`, and even then they are lower priority than healthy pending rows.

Deterministic contract/config errors become `blocked` and require an operator fix before manual retry/reset.

## Investigating Database Locks

SQLite connections use:

```text
BEEHIVE_SQLITE_BUSY_TIMEOUT_MS
```

Default: `30000`.

If `database is locked` appears:

1. Check worker summary active leases and recent errors.
2. Lower desired worker counts from UI.
3. Confirm the server is not running duplicate supervisors for the same workspace.
4. Inspect app events for repeated busy/locked messages.
5. If locks persist, use a smaller pilot and consider an in-process workspace write gate as the next hardening step.

## Safe Pilot Procedure For itg_documents

Do not run the full workspace from B14 controls unless an explicit bounded subset mechanism is available.

Safe procedure:

1. Use a dedicated test workspace or copied subset.
2. Set `scheduling_policy: depth_first`.
3. Start with default workers `3`, local LLM workers `1`.
4. Stop after 300-1000 claimed tasks.
5. Record succeeded, retry_wait, blocked, failed, active lease max, and lock count.

B14 does not add subset-limited background workers, so an unrestricted `itg_documents` pilot is out of scope.
