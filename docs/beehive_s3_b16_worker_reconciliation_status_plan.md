# B16 Worker Reconciliation And Status Plan

## 1. Worker Cap Diagnosis

Current worker cap data is split across runtime YAML, server env, stored pool control state, and UI input.
The backend already returns `configured_concurrency`, `desired_concurrency`, `effective_concurrency`, `active_leases`, `is_started`, and `is_paused`, but it does not expose the server env cap or whether a request was capped.

B16 will extend the worker summary model so each pool can explain:

- `configured_concurrency`: `runtime.worker_pools.<pool>.concurrency` from `pipeline.yaml`.
- `env_concurrency_limit`: `BEEHIVE_WORKER_DEFAULT_CONCURRENCY` / `BEEHIVE_WORKER_LOCAL_LLM_CONCURRENCY`, if set.
- `requested_desired_concurrency`: the most recent request value for start/update calls when that request is capped.
- `desired_concurrency`: the applied/stored DB value.
- `effective_concurrency`: the active claim cap.
- `active_leases`, `is_started`, `is_paused`.

If a requested value is reduced, the API response will expose both requested and applied values.
The UI will render requested/applied/YAML/env/effective values directly in Workers & Queue and derive a clear capped-request warning from those fields.

## 2. Stuck In-Progress And Stale Lease Diagnosis

Current worker claims only consider `pending` and due `retry_wait`.
An `in_progress` or `queued` state with no valid active lease is invisible to claimers and can make a live worker look idle while the UI still shows work in progress.

B16 will add worker anomaly diagnostics to the summary:

- `in_progress_without_active_lease`
- `queued_without_active_lease`
- `active_lease_with_finished_run`
- `active_lease_expired`
- `active_lease_without_recent_heartbeat`
- `active_lease_for_missing_state`
- `active_lease_for_state_not_running`

The summary will expose counts and recent anomaly records with state/lease/entity/stage/resource/run/timestamp fields, a diagnosis, and a recommended action.

## 3. Entity Detail File-Status Mismatch

`EntityFileInstances` currently renders `file.status`, which is the artifact registration status.
The runtime source of truth is `detail.stage_states`.

B16 will keep DB data intact and compute display status in the frontend:

1. Match `stage_state.file_instance_id == file.id`.
2. Fallback to matching `stage_state.entity_id + stage_state.stage_id`.
3. Fallback to `file.status`.

The table label will become `Runtime status`, and the artifact status will be shown as secondary text so the operator can see why old rows may still say `pending`.

## 4. Backend Changes

- Extend domain structs for worker cap fields, anomaly counts, anomaly records, and reconcile result payload.
- Add database diagnostics queries for stale lease/state anomaly classes.
- Add a deterministic reconcile function that repairs:
  - active lease with finished successful run -> state `done`, lease `done` or released;
  - active lease with finished failed run -> `retry_wait`/`failed`/`blocked` according to run details and attempts;
  - old `in_progress` without active lease -> `retry_wait` or `failed`;
  - `queued` without active lease -> `pending`.
- Add `services::workers::reconcile_stuck` and `POST /api/workspaces/{workspace_id}/workers/reconcile-stuck`.
- Add a best-effort executor finalizer for `run_worker_task` internal errors after claim/start, so no permanent `in_progress + active lease` remains.
- Preserve existing worker start/stop/pause/resume behavior.

## 5. UI Changes

- In Workers & Queue, show cap explanation per pool: requested, applied desired, YAML limit, env limit, effective.
- Show capped-request warnings from the requested/applied cap fields in worker start/update responses.
- Show a visible worker-state attention panel when anomalies exist, with a `Reconcile stuck worker states` action.
- Keep recent leases table, but add anomaly diagnostics as an operator-facing warning rather than hiding it in details.
- In Entity Detail File Instances, show effective runtime status and secondary artifact record status.

## 6. Tests And Smoke

Backend tests:

- env 10 + YAML 1 + requested 3 -> applied/effective 1 and capped warning/cap fields.
- env 10 + YAML 10 + requested 3 -> applied/effective 3.
- env absent + YAML 5 + requested 10 -> applied/effective 5.
- active lease + finished successful run -> state done and lease finished/released.
- active lease + finished failed run -> retry/failed/blocked according to attempts and run details.
- in_progress without active lease and old unfinished run -> retry_wait or failed.
- queued without active lease -> pending.
- internal executor error after start -> no active lease and no permanent in_progress.
- reconcile stuck action is idempotent.

Frontend coverage:

- Add a small pure helper for effective file status and use TypeScript build as verification if no frontend test runner is configured.

Smoke:

- Use a small test workspace or automated mock-style smoke.
- Do not run a full `itg_documents` batch.
- Do not reset/import/delete `itg_documents`.

## 7. What Will Not Change In B16

B16 will not implement RabbitMQ, Kafka, Postgres, a distributed worker registry, force-killing n8n executions, a large production run, a full Entity Detail redesign, or a full README rewrite.

## Checkpoints

Instruction rereads are scheduled for:

`after_plan`, `after_cap_ux_design`, `after_lease_reconciliation_design`, `after_entity_file_status_design`, `after_backend_changes`, `after_ui_changes`, `after_tests`, `after_smoke`, `before_feedback`.
