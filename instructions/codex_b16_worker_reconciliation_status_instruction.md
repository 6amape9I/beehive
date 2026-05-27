# B16. Worker Lease Reconciliation, Cap UX, and Effective Entity File Status

## 0. Context

Beehive workers are now generally functional after B15. Real testing on `itg_documents` showed that workers can run and stages can complete, but three operator-facing problems remain:

1. The UI can show `max/desired workers = 1` even when the server was started with `BEEHIVE_WORKER_DEFAULT_CONCURRENCY=10`. This is confusing because the effective limit is the minimum of server env, `runtime.worker_pools.*.concurrency`, and UI desired count.
2. A `local_llm` worker can become idle while a local-LLM task remains stuck in `in_progress`. This suggests stale or inconsistent worker lease/stage state handling.
3. Entity Detail shows file instances as `Pending` even though the stage timeline says the same stages are `Done`. This makes successful pipeline progress look broken.

B16 must make the worker state explainable and recoverable, and must fix the misleading Entity Detail file status display.

Do not implement RabbitMQ, Kafka, Postgres, a new scheduler, or a full UI rewrite.

Do not run a full production-sized `itg_documents` batch. Use only bounded observation or tests unless explicitly authorized.

## 1. Required process

Before code, create:

```text
docs/beehive_s3_b16_worker_reconciliation_status_plan.md
```

After code, create:

```text
docs/beehive_s3_b16_worker_reconciliation_status_feedback.md
```

The plan must explicitly cover:

```text
1. Worker max/desired/effective cap diagnosis.
2. Stuck `in_progress` / stale lease diagnosis.
3. The Entity Detail file-status mismatch.
4. Backend changes.
5. UI changes.
6. Tests and smoke.
7. What will not be changed in B16.
```

Reread this instruction at checkpoints:

```text
after_plan
after_cap_ux_design
after_lease_reconciliation_design
after_entity_file_status_design
after_backend_changes
after_ui_changes
after_tests
after_smoke
before_feedback
```

## 2. Worker cap UX: explain why requested workers are capped

### 2.1 Problem

Users can start the server with:

```bash
BEEHIVE_WORKER_DEFAULT_CONCURRENCY=10
```

but the UI still shows max/desired/default as `1`.

This is usually not a server env bug. Effective worker concurrency is capped by:

```text
min(server env max, runtime.worker_pools.<pool>.concurrency, UI desired concurrency)
```

If `pipeline.yaml` contains:

```yaml
runtime:
  worker_pools:
    default:
      concurrency: 1
```

then the effective max is still 1 even when the env says 10.

### 2.2 Requirements

Extend `WorkerPoolRuntimeSummary` and frontend types to expose enough cap details:

```text
resource_class
configured_concurrency        # YAML/runtime.worker_pools limit
env_concurrency_limit         # from BEEHIVE_WORKER_*_CONCURRENCY, nullable
requested_desired_concurrency # what the user last requested, if available
desired_concurrency           # value actually stored/applied
effective_concurrency         # active claim cap
active_leases
is_started
is_paused
```

If `requested_desired_concurrency` is too invasive to persist, at least return a warning when a requested value is capped:

```json
{
  "code": "worker_desired_concurrency_capped",
  "message": "Requested 3 default workers, but applied 1 because runtime.worker_pools.default.concurrency is 1."
}
```

### 2.3 UI behavior

In Workers & Queue, show:

```text
Default pool:
  Requested: 3
  Applied desired: 1
  YAML limit: 1
  Env limit: 10
  Effective: 1
```

If requested > applied, show a clear warning:

```text
Requested 3 default workers, but Beehive applied 1.
Increase runtime.worker_pools.default.concurrency in pipeline.yaml or Stage/Runtime settings.
```

Do not leave the user guessing.

### 2.4 Optional but useful

Allow editing `runtime.worker_pools.default.concurrency` and `runtime.worker_pools.local_llm.concurrency` from the Runtime/Worker settings UI, or add a direct link to the correct settings page.

If editing YAML config is too large for B16, add explicit documentation and UI hint.

## 3. Stuck `in_progress` and stale lease reconciliation

### 3.1 Problem

Real logs showed a `local_llm` worker repeatedly reporting:

```text
worker_context_loaded
worker_claim_idle
```

while an entity remained `in_progress` in the UI.

This means the worker is alive and idle, but the DB still contains a state that is not eligible for claim. Claims only consider:

```text
pending
retry_wait where next_retry_at <= now
```

An `in_progress` row will not be reclaimed unless recovery moves it.

### 3.2 Required diagnostics

Add summary diagnostics for these anomaly classes:

```text
in_progress_without_active_lease
queued_without_active_lease
active_lease_with_finished_run
active_lease_expired
active_lease_without_recent_heartbeat
active_lease_for_missing_state
active_lease_for_state_not_running
```

Expose counts in `WorkerSummary`.

Expose recent anomaly records with:

```text
state_id
lease_id
entity_id
stage_id
resource_class
state_status
lease_status
worker_id
run_id
lease_until
heartbeat_at
last_started_at
last_finished_at
diagnosis
recommended_action
```

### 3.3 Recovery behavior

Extend `recover_expired_leases` or add a new endpoint:

```text
POST /api/workspaces/{workspace_id}/workers/reconcile-stuck
```

The action must be safe and deterministic.

Rules:

#### Active expired lease + unfinished run

Existing behavior is mostly correct:

```text
lease.status = expired
state queued -> pending
state in_progress -> retry_wait if attempts remain, else failed
app event worker_lease_expired
```

Keep this behavior.

#### Active lease + finished stage_run

Current code can skip this as an anomaly. B16 must repair it.

If attached `stage_run` is finished:

```text
if stage_run.success = true:
    state -> done
    lease -> done or released with release_reason=finished_run_reconciled
else:
    state -> retry_wait/failed/blocked according to error_type and attempts
    lease -> failed or released with release_reason=finished_run_reconciled
```

Do not leave active lease blocking the pool.

#### In-progress state without active lease

If `entity_stage_states.status = in_progress` and there is no active lease:

```text
if there is an unfinished stage_run younger than worker_lease_sec:
    leave it alone and report "recent_unleased_in_progress"
else:
    move to retry_wait if attempts remain, else failed
    add app_event "in_progress_without_active_lease_reconciled"
```

#### Queued state without active lease

If `queued` without active lease and no unfinished run:

```text
state -> pending
app_event "queued_without_active_lease_reconciled"
```

### 3.4 Internal executor safety net

Audit `run_worker_task` / `execute_task`.

If `execute_task` returns `Err` after a state has been moved to `in_progress`, the system must not leave:

```text
entity_stage_states.status = in_progress
worker_leases.status = active
unfinished stage_runs
```

Add a best-effort safety finalizer:

```rust
finish_worker_task_internal_error(...)
```

It should:

```text
1. Find active lease by lease_id/state_id.
2. Find attached run_id if any.
3. If run exists and unfinished, finish it with error_type = worker_internal_error.
4. If state is queued/in_progress, move it to retry_wait if attempts remain, else failed.
5. Finish/release lease as failed.
6. Insert app_event with error details.
```

This finalizer must be called when `execute_task` returns `Err` from `run_worker_task`.

### 3.5 UI behavior

In Workers & Queue show a visible warning if anomalies exist:

```text
Worker state needs attention:
- 1 in_progress task has no active lease
- 1 active lease has a finished run
[Reconcile stuck worker states]
```

Do not hide this in a low-level diagnostics table.

## 4. Entity Detail file status mismatch

### 4.1 Problem

Entity Detail currently shows File Instances with file-level `status`, while Stage Timeline shows stage runtime status.

In the observed UI:

```text
File Instances:
  stage_1_3 Pending
  stage_2   Pending
  stage_3   Pending
  stage_4   Pending

Stage Timeline:
  stage_1_3 Done
  stage_2   Done
  stage_3   Done
  stage_4   Done
```

This is misleading. The stage timeline is the runtime source of truth; the file row is an artifact record and may still contain its original registration status.

### 4.2 Root cause to verify

`EntityFileInstances` currently renders:

```tsx
<StatusBadge status={file.status} />
```

But `EntityDetailPayload` also contains `stage_states`, where `EntityStageStateRecord.status` is the actual execution status for a file/stage.

B16 must verify this by inspecting the code and tests.

### 4.3 Required behavior

File Instances should display effective runtime status, not stale file registration status.

Preferred implementation:

```text
effective_status = matching stage_state.status where stage_state.file_instance_id == file.id
fallback = matching stage_state by entity_id + stage_id
fallback = file.status
```

UI label should become one of:

```text
Runtime status
Execution status
```

Optionally add a secondary column or tooltip:

```text
Artifact record status: pending
```

Do not confuse the operator.

### 4.4 Backend option

If the backend can cheaply return `runtime_status` per file, add it to an Entity Detail view model.

Options:

1. Add `runtime_status` to `EntityFileRecord`.
2. Add a separate `EntityFileRuntimeStatus` map/list in `EntityDetailPayload`.
3. Derive in frontend from `detail.stage_states`.

Option 3 is acceptable for B16 and avoids schema churn.

### 4.5 Important rule

Do not blindly update all `entity_files.status` to done unless you understand downstream effects.

The safer B16 change is display-layer effective status based on `entity_stage_states`.

## 5. Tests

Add Rust tests for reconciliation:

```text
active lease + finished successful run -> state done, lease done/released
active lease + finished failed run -> state retry_wait/failed according to attempts
in_progress without active lease and old unfinished run -> retry_wait/failed
queued without active lease -> pending
execute_task internal error after start -> no active lease and no permanent in_progress
recover stuck action is idempotent
```

Add frontend or unit-level tests where available. If there is no frontend test harness, add TypeScript-safe helper tests where possible or document coverage by build.

At minimum, create pure helper function for effective file status and test it if the repo has a test runner. If no test runner exists, keep helper simple and rely on `npm run build`.

Add/extend backend tests for worker cap summaries:

```text
env 10 + yaml 1 + requested 3 -> applied/effective 1 with warning or exposed cap fields
env 10 + yaml 10 + requested 3 -> applied/effective 3
env absent + yaml 5 + requested 10 -> applied/effective 5
```

## 6. Smoke / manual verification

Do not run full `itg_documents`.

Do a bounded check:

```text
1. Start server with workers enabled for a small workspace or controlled itg_documents observation.
2. Start default=1, local_llm=1.
3. Confirm worker summary cap explanation is visible.
4. Confirm no task remains in_progress without active lease after recover stuck.
5. Confirm Entity Detail File Instances shows Done when Stage Timeline shows Done.
```

If using `itg_documents`, do not reset/import/delete; only inspect and reconcile if explicitly safe.

## 7. Required commands

Run and report:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
git diff --check
```

## 8. Feedback

Create:

```text
docs/beehive_s3_b16_worker_reconciliation_status_feedback.md
```

Feedback must include:

```text
1. Root cause of worker cap confusion.
2. Root cause of local_llm idle while task was in_progress.
3. Whether any stale leases/states were found in tests or manual checks.
4. How reconciliation works.
5. How Entity Detail effective file status works.
6. Commands run and exact results.
7. Smoke/manual results.
8. Remaining risks.
9. B17 recommendations.
```

Required checkpoint line:

```text
ТЗ перечитано на этапах: after_plan, after_cap_ux_design, after_lease_reconciliation_design, after_entity_file_status_design, after_backend_changes, after_ui_changes, after_tests, after_smoke, before_feedback
```

## 9. Acceptance criteria

B16 is accepted only if:

```text
1. UI explains why worker desired/effective concurrency is capped.
2. Operator can see YAML limit and env limit or a clear equivalent warning.
3. Worker summary detects stale/inconsistent worker state.
4. Recover/reconcile can repair in_progress without active lease.
5. Recover/reconcile can repair active lease with finished run.
6. run_worker_task internal errors cannot leave permanent in_progress + active lease.
7. Entity Detail File Instances shows runtime status consistent with Stage Timeline.
8. Existing worker start/stop/pause/resume still works.
9. No destructive operation is run on itg_documents.
```

## 10. Non-goals

Do not implement:

```text
RabbitMQ
Kafka
Postgres
distributed worker registry
force-kill running n8n executions
large production run
full Entity Detail redesign
full README rewrite
```
