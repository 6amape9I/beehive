# Worker Leases Runtime Contract

## Scope

B12 adds an internal SQLite-backed worker queue for Beehive server workers. Beehive remains the concurrency owner. n8n remains the workflow executor.

Workers are disabled by default. They start only when both conditions are true:

- `BEEHIVE_WORKERS_ENABLED=1`
- `BEEHIVE_WORKER_WORKSPACES` is set to explicit workspace ids or `all`

No worker startup path targets every workspace implicitly.

## Schema

Schema v10 adds `worker_leases`.

Required lease fields:

- `lease_id`
- `state_id`
- `entity_id`
- `entity_file_id`
- `stage_id`
- `resource_class`
- `worker_id`
- `status`
- `run_id`
- `leased_at`
- `lease_until`
- `heartbeat_at`
- `released_at`
- `release_reason`
- `created_at`
- `updated_at`

Lease statuses:

- `active`
- `done`
- `failed`
- `expired`
- `released`

Indexes include active lease lookup by state/resource class/expiry and a partial unique index:

```sql
CREATE UNIQUE INDEX idx_worker_leases_one_active_state
ON worker_leases(state_id)
WHERE status = 'active';
```

This is the DB-level double-claim guard.

## Claim

Worker claim is resource-class aware. A default worker only claims `stage.resource_class = default`. A local LLM worker only claims `stage.resource_class = local_llm`.

Eligible states:

- `pending`
- `retry_wait` when `next_retry_at <= now`

Excluded rows:

- `done`, `in_progress`, `queued`, `blocked`, `failed`, `skipped`
- archived entities
- missing source files
- inactive stages
- stages with empty `workflow_url`
- any state with an `active` worker lease

The claim transaction:

1. Selects eligible candidates for a single resource class.
2. Updates `entity_stage_states.status` to `queued`.
3. Inserts an `active` `worker_leases` row.
4. Commits before execution starts.

Existing manual/selected claim paths refuse active worker leases.

## Execution

Worker execution reuses the existing executor path:

```text
pending/retry_wait -> queued + active lease
queued -> in_progress + stage_run.run_id attached to lease
in_progress -> done/retry_wait/failed/blocked + lease done/failed/released
```

No new n8n executor exists in B12.

## Heartbeat

Runtime config fields:

```yaml
runtime:
  worker_lease_sec: 1800
  worker_heartbeat_sec: 30
```

Defaults:

- `worker_lease_sec = max(request_timeout_sec + 300, 1800)`
- `worker_heartbeat_sec = 30`

Heartbeat updates only an `active` lease owned by the same `worker_id`:

- `heartbeat_at = now`
- `lease_until = now + worker_lease_sec`

Wrong-worker or non-active heartbeat fails safely.

## Recovery

`recover_expired_worker_leases`:

1. Finds `active` leases where `lease_until < now`.
2. Skips leases whose attached `stage_run` is already finished.
3. Marks the lease `expired`.
4. For a still-`queued` state, returns it to `pending`.
5. For a still-`in_progress` state, returns it to `retry_wait` if attempts remain, otherwise `failed`.
6. Writes app event `worker_lease_expired`.

Recovery is not broad cleanup. It only touches expired active leases and their still-active states.

## Worker Manager Env

```bash
BEEHIVE_WORKERS_ENABLED=1
BEEHIVE_WORKER_WORKSPACES=workspace_a,workspace_b
BEEHIVE_WORKER_DEFAULT_CONCURRENCY=10
BEEHIVE_WORKER_LOCAL_LLM_CONCURRENCY=1
```

`BEEHIVE_WORKER_WORKSPACES=all` is accepted but still requires the explicit env value.

Effective concurrency never exceeds `runtime.worker_pools.*.concurrency`. `concurrency=0` disables the pool.

Optional tuning:

```bash
BEEHIVE_WORKER_IDLE_SLEEP_MS=1000
BEEHIVE_WORKER_RECOVERY_INTERVAL_SEC=30
```

## HTTP Diagnostics

```text
GET  /api/workspaces/{workspace_id}/workers/summary
POST /api/workspaces/{workspace_id}/workers/recover-expired-leases
```

The summary reports configured pool concurrency, active leases, expired leases, lease timing defaults, last recovery event time, and recent leases.
