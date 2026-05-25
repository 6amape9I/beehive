# B13 Worker Operations Hardening Plan

## 1. What Exists After B12

- B12 added DB-backed `worker_leases` with `active`, `done`, `failed`, `expired`, and `released` statuses.
- Worker claims are resource-class aware: `default` workers claim only `default` stages, and `local_llm` workers claim only `local_llm` stages.
- Active leases are protected by a partial unique index on `worker_leases(state_id)`.
- Worker threads are disabled by default and start only when `BEEHIVE_WORKERS_ENABLED=1` and `BEEHIVE_WORKER_WORKSPACES` are set.
- Basic HTTP diagnostics exist for worker summary and expired lease recovery.
- The UI has a minimal Worker Pools diagnostics panel, but no full queue view or pool control surface.

## 2. B12 Risks Closed By B13

- Broad manual run actions can still use old claim paths and bypass worker-pool limits.
- Operators cannot pause/resume workers without stopping the process.
- Operators cannot safely release anomalous active leases where the attached run already finished.
- Summary data is not rich enough to answer queue pressure questions by `resource_class`.
- SQLite connections need explicit contention hardening before multi-threaded pilot runs.

## 3. Bypass Broad Run Actions

B13 will use the safer Variant B from the instruction.

When workers are enabled for a workspace, broad run actions will return a clear error instead of silently using the old non-lease claim path:

```json
{
  "errors": [
    {
      "code": "workers_enabled_broad_run_disabled",
      "message": "Workers are enabled for this workspace. Use selected run or worker pools instead."
    }
  ]
}
```

This policy will be enforced in the workspace service path used by HTTP and Tauri-by-id commands:

- `run_small_batch`
- `run_pipeline_waves`

The UI will disable/hide broad actions when workers are enabled and show the required warning. Selected pipeline waves remain available as an explicit operator/debug action, while existing active-lease checks continue to prevent selected work from stealing a leased root.

## 4. Worker/Queue UI

The existing workspace view will grow an explicit `Workers & Queue` section instead of leaving workers hidden in diagnostics.

Minimum B13 UI:

- Default pool: pending, running, expired, paused, configured concurrency.
- Local LLM pool: pending, running, expired, paused, configured concurrency.
- Recent leases table with lease id, worker id, entity, stage, resource class, status, lease timing, run id, release reason, and safe actions.
- Visible recover-expired-leases action.
- Visible pause/resume all and pause/resume per-pool actions.
- Warnings for disabled workers, disabled broad runs, and full `local_llm` pool.

## 5. Pause/Resume Pools

B13 will add a per-workspace DB control table:

```text
worker_pool_controls(
  resource_class,
  is_paused,
  pause_reason,
  updated_at
)
```

The DB is already per-workspace, so no `workspace_id` column is needed inside the workspace DB.

Behavior:

- Worker loops check pause state before claiming.
- Paused pools do not claim new tasks.
- Running leases continue.
- `paused_all` pauses all pools by setting both known resource classes.
- Resume clears pause state for all or one resource class.

API:

- `POST /api/workspaces/{workspace_id}/workers/pause`
- `POST /api/workspaces/{workspace_id}/workers/resume`
- `POST /api/workspaces/{workspace_id}/workers/pools/{resource_class}/pause`
- `POST /api/workspaces/{workspace_id}/workers/pools/{resource_class}/resume`

## 6. Manual Lease Actions

B13 will add a safe release endpoint:

```text
POST /api/workspaces/{workspace_id}/workers/leases/{lease_id}/release
```

Minimal safe release rules:

- Release active lease if its attached `stage_run` is already finished.
- Release active lease if it is expired.
- Reject fresh active unfinished leases.
- No dangerous force release in B13.
- Releasing a lease only marks the lease released and records a reason; it does not mark the stage state successful or failed.

Expired lease recovery remains the normal action for stale active work that still owns unfinished state.

## 7. SQLite Hardening

Every connection opened through the workspace DB helper will apply:

```sql
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
```

B13 will also enable WAL for writable workspace DB connections if it is safe in the current code path:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
```

If any test or deployment constraint shows WAL is unsafe, B13 will keep `busy_timeout` as the acceptance minimum and document the reason in feedback.

## 8. Controlled Pilot

`itg_documents` will not be used for destructive smoke, broad execution, reset, import, cleanup, or archive actions.

Pilot preference:

- Use a separate test workspace with 20-100 files if one exists or can be created safely.
- Run conservative settings: default concurrency 3-5, `local_llm` concurrency 1.
- Record processed counts, failures, blocked states, max observed active leases, throughput, errors, and whether real S3/n8n calls were made.

If a safe test workspace is not available in this turn, B13 will create `docs/beehive_s3_b13_worker_pilot_report.md` explaining the exact blocker instead of using `itg_documents`.

## 9. Non-Goals For B13

B13 will not implement:

- RabbitMQ, Kafka, or Postgres migration.
- RBAC.
- New scheduler architecture.
- Large 22k `itg_documents` production run.
- n8n REST workflow editor.
- Full visual dashboard polish.
- Automatic scaling or priority queues.
- Unsafe mass retry/release actions.

## 10. Protection For `itg_documents`

- No command in B13 will run workers against `itg_documents`.
- No smoke or pilot will target `itg_documents` unless explicitly scoped by the user later.
- Any read-only inspection, if needed, will be limited to metadata/summary.
- `BEEHIVE_WORKER_WORKSPACES=all` will be documented as unsafe for production pilot.
- The runbook will require explicit workspace scope and conservative concurrency for any future `itg_documents` pilot.

## Checkpoints

Planned rereads:

```text
after_plan
after_b12_review
after_bypass_design
after_pause_resume_design
after_lease_actions_design
after_queue_ui_design
after_sqlite_hardening
after_tests
after_pilot
before_feedback
```
