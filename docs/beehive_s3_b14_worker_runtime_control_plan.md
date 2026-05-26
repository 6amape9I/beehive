# B14 Worker Runtime Control Plan

## 1. Current B13 Baseline

- Workers remain guarded by server-level env: `BEEHIVE_WORKERS_ENABLED` and `BEEHIVE_WORKER_WORKSPACES`.
- B13 added `worker_pool_controls` with pause/resume state per `resource_class`.
- B13 added UI-visible worker summary, recent leases, pause/resume, expired recovery, and safe manual release.
- Broad workspace run actions are disabled when workers are enabled for the workspace.
- SQLite writable connections use WAL, `synchronous = NORMAL`, and `busy_timeout = 5000`.
- Real B13 pilot was blocked because no safe test workspace and endpoint were provided.

## 2. B14 Risks To Close

- Worker operation is still effectively terminal/env driven; UI cannot start/stop worker runtime or set desired counts.
- Worker claim ordering is still FIFO-like and does not push fresh child artifacts deeper through the pipeline.
- `busy_timeout = 5000` was too low for concurrent worker pressure.
- Retry behavior needs clearer split between transient errors and deterministic contract errors.
- Due retry work should be demoted behind healthy pending work.

## 3. Runtime Control Model

B14 will extend `worker_pool_controls` rather than adding a second control table.

Planned fields:

- `desired_concurrency`
- `is_started`
- existing `is_paused`
- existing `pause_reason`
- `updated_at`

Workspace-level Start will:

- set both known pools to `is_started = true`;
- set desired concurrency from UI request;
- preserve B13 pause semantics.

Workspace-level Stop will:

- set both pools to `is_started = false`;
- not kill in-flight n8n/S3 executions;
- make worker loops stop claiming new tasks and show draining through active lease counts.

Effective claim behavior:

```text
claim allowed when server env scope allows workspace
AND pool is_started
AND pool is not paused
AND desired_concurrency > active_leases for that pool
```

Env/YAML remain guardrails:

```text
effective worker loop upper bound = min(env_max, runtime.worker_pools.*.concurrency)
effective desired count = min(ui_desired_concurrency, effective worker loop upper bound)
```

The MVP supervisor will keep B13 fixed worker threads spawned from env/YAML upper bounds. UI state controls whether those threads claim work and whether active lease counts are below desired concurrency.

## 4. API/UI Plan

Backend endpoints:

- `GET /api/workspaces/{workspace_id}/workers/summary`
- `POST /api/workspaces/{workspace_id}/workers/start`
- `POST /api/workspaces/{workspace_id}/workers/stop`
- `PATCH /api/workspaces/{workspace_id}/workers/pools/default`
- `PATCH /api/workspaces/{workspace_id}/workers/pools/local_llm`
- existing pause/resume/recover/release endpoints remain.

Frontend `Workers & Queue` panel will add:

- number inputs for default and local LLM desired workers;
- Start workers;
- Stop workers;
- runtime status: stopped/running/draining/paused;
- desired and active counts per pool;
- scheduling policy display and depth-first explanation.

## 5. Scheduling Policy

Add config field:

```yaml
runtime:
  scheduling_policy: depth_first
```

Allowed values:

- `depth_first`
- `fifo`

Default:

- `depth_first` for configs without an explicit value.

Claim ordering:

```text
pending before due retry_wait
then scheduling policy
then stable id ordering
```

For `depth_first`:

```sql
CASE WHEN file.producer_run_id IS NOT NULL THEN 0 ELSE 1 END ASC,
state.updated_at DESC,
state.id DESC
```

For `fifo`:

```sql
state.updated_at ASC,
state.id ASC
```

Anti-starvation MVP:

- B14 will document that `fifo` can be selected for bulk completion if depth-first starves old source work.
- If low-risk during implementation, add a simple aging boost for very old source tasks; otherwise record the remaining risk honestly.

## 6. SQLite Lock Hardening

Changes:

- Add `BEEHIVE_SQLITE_BUSY_TIMEOUT_MS`.
- Default to `30000`.
- Apply to writable and readonly workspace connections.
- Add lock/busy detection helper.
- Add bounded retry/backoff helper for critical SQLite writes.

Retry helper behavior:

```text
only retry SQLITE_BUSY / SQLITE_LOCKED / "database is locked" / "database is busy"
max retries: 5
backoff: 50ms, 100ms, 200ms, 400ms, 800ms
```

Initial application target:

- worker claim transaction;
- lease heartbeat/finish/recovery/control writes where lock contention is most operationally visible.

An in-process per-workspace write gate will be a B15 candidate unless tests show the retry helper is insufficient or implementation is straightforward without holding the gate across n8n/S3 calls.

## 7. Retry Policy

Preserve deterministic contract errors as `blocked`:

- invalid manifest root/schema;
- workspace/run/source mismatch;
- unsafe or unknown `save_path`;
- forbidden output cardinality;
- missing required S3 metadata;
- invalid stage config / missing workflow URL.

Transient errors should go to `retry_wait` when attempts remain:

- network timeout;
- n8n timeout;
- HTTP 5xx;
- S3 transient errors;
- SQLite busy/locked;
- temporary worker failure.

MVP backoff:

- keep existing `retry_delay_sec`;
- demote due `retry_wait` behind `pending` in claim ordering;
- keep existing manual reset/retry controls and document them.

## 8. Tests

Backend tests to add/update:

- worker start/stop state prevents/allows claim;
- desired concurrency caps claims with active leases;
- env/YAML limits remain guardrails;
- fifo claims old source before newer child;
- depth-first claims fresh child before old source;
- pending claims before due retry_wait;
- depth-first still respects `resource_class`, archived/missing filters, and active leases;
- busy timeout env parsing and connection pragma;
- SQLite busy/locked detection;
- write retry helper retries busy and stops after limit;
- transient timeout / HTTP 5xx -> `retry_wait`;
- manifest/cardinality contract errors remain `blocked`.

Frontend/build:

- both existing build commands must pass.

## 9. Pilot Or Smoke

`itg_documents` will not be reset, archived, truncated, imported into, or run unbounded.

B14 will run a bounded smoke only if a safe test workspace or existing mock workspace is available. If no safe workspace exists, feedback will say pilot/smoke was blocked and why.

No full `itg_documents` run will be started without explicit operator-limited controls and user confirmation.

## 10. Deliverables

- `docs/beehive_s3_b14_worker_runtime_control_plan.md`
- `docs/beehive_s3_b14_worker_runtime_control_feedback.md`
- `docs/worker_runtime_control_runbook.md`
- `docs/worker_retry_policy.md`

## 11. Checkpoints

Planned rereads:

```text
after_plan
after_b13_review
after_worker_runtime_control_design
after_depth_first_scheduler_design
after_sqlite_lock_design
after_retry_policy_design
after_backend_changes
after_ui_changes
after_tests
after_pilot_or_smoke
before_feedback
```
