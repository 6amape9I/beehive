# B14. Worker Runtime Control, Depth-first Scheduling, SQLite Lock Hardening, and Retry Demotion

## 0. Context

Beehive has working S3/n8n orchestration, stage/entity/workspace CRUD, S3 JSON upload, selected pipeline execution, resource classes, DB-backed worker leases, worker heartbeat/recovery, and initial worker operations hardening.

B13 added pause/resume, manual lease release, worker diagnostics, broad-run bypass prevention, and SQLite WAL/busy_timeout. However, a real small production-scale check on `itg_documents` still exposed practical issues:

1. Operators still cannot start/stop/configure worker counts fully from the web UI.
2. Current scheduling behaves too much like breadth-first/FIFO; workers keep chewing early stages instead of pushing fresh descendants deeper through the pipeline.
3. Around 150 tasks produced multiple `database is locked` errors despite WAL and `busy_timeout = 5000`.
4. Error handling is too harsh in day-to-day operation: too many failures become blocked, while transient errors should usually retry later with lower queue priority.

B14 must turn B12/B13 worker infrastructure into a more operator-controlled and production-pilot-ready runtime.

Do not add RabbitMQ, Kafka, Postgres, RBAC, distributed workers, or a full new scheduler platform in B14.

## 1. Required working style

Before implementation, create:

```text
docs/beehive_s3_b14_worker_runtime_control_plan.md
```

Do not write runtime code before this plan exists.

During the task, reread this instruction at these checkpoints and list them in feedback:

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

After implementation, create:

```text
docs/beehive_s3_b14_worker_runtime_control_feedback.md
docs/worker_runtime_control_runbook.md
docs/worker_retry_policy.md
```

Feedback must be honest. Do not claim real pilot success if only unit tests ran.

## 2. Preserve and respect current production workspace

`itg_documents` is an important large workspace already used for a real run of about 22,000 documents. It is not test garbage.

B14 must not:

```text
reset itg_documents
archive itg_documents
delete itg_documents
truncate itg_documents DB
import destructive test data into itg_documents
run all pending itg_documents without explicit operator-controlled limits
```

Any pilot against `itg_documents` must be explicit, bounded, and documented.

## 3. Product goal

The operator flow should become:

```text
Open Beehive web app
→ choose workspace
→ upload/import entities if needed
→ configure worker counts by resource class
→ click Start workers
→ watch queue/backpressure
→ click Stop workers when needed
```

This must not remain terminal-only. Environment variables may remain useful for server bootstrap/admin, but normal operation should be possible from the web UI.

## 4. Worker runtime control from UI

### 4.1 Desired UI

In the selected workspace, add or improve a clear `Workers` / `Очередь` panel/page.

Required controls:

```text
Default workers: [number input]
Local LLM workers: [number input]
[Start workers]
[Stop workers]
[Pause default]
[Resume default]
[Pause local_llm]
[Resume local_llm]
[Recover expired leases]
```

Use operator-friendly text. Do not expose internal env variable names as the primary UI.

### 4.2 Semantics

`Start workers` means:

```text
set desired worker runtime state for this workspace to active;
set desired_concurrency per pool;
start or allow worker loops for those pools;
claim new tasks according to pool limits.
```

`Stop workers` means:

```text
stop claiming new tasks;
let already running n8n executions finish normally;
do not force-kill n8n requests by default;
show draining/running count until all active leases finish.
```

Do not implement force-kill in B14. If you add a placeholder, it must be disabled or clearly marked out of scope.

### 4.3 Runtime state storage

Move beyond env-only worker operation.

Add persistent per-workspace worker runtime control state in SQLite, reusing or extending `worker_pool_controls` if appropriate:

```text
resource_class
desired_concurrency
is_paused
is_started / desired_state
updated_at
updated_by optional
pause_reason optional
```

The exact schema can differ, but it must support:

```text
workspace-level start/stop
per-pool desired concurrency
per-pool pause/resume
readable summary for UI
```

### 4.4 Supervisor behavior

The current worker manager starts detached threads from env. B14 should add a runtime supervisor concept.

Acceptable MVP approach:

```text
beehive-server still starts a supervisor only when workers are enabled at server level;
UI controls per-workspace desired_concurrency and start/stop state;
supervisor loops check DB state and only claim work when workspace/pool is started and not paused.
```

Better approach, if feasible:

```text
supervisor can spawn additional pool threads up to desired_concurrency and let excess workers exit when desired_concurrency is lowered.
```

Do not overbuild. It is acceptable if B14 only controls claim behavior and uses a fixed max number of worker threads, as long as UI Start/Stop and desired counts are reflected in actual claims.

### 4.5 Env compatibility

Keep env as bootstrap/admin safety:

```text
BEEHIVE_WORKERS_ENABLED
BEEHIVE_WORKER_WORKSPACES
BEEHIVE_WORKER_DEFAULT_CONCURRENCY
BEEHIVE_WORKER_LOCAL_LLM_CONCURRENCY
```

But document that env values are upper bounds / bootstrap guardrails, while UI desired state controls actual work inside the selected workspace.

Example:

```text
effective_pool_limit = min(env_max, pipeline_yaml_pool_concurrency, ui_desired_concurrency)
```

If this exact formula is not implemented, document the actual formula clearly.

## 5. Depth-first scheduling

### 5.1 Problem

The current queue ordering is too close to breadth-first/FIFO. In a large workspace, workers process many stage_0/stage_1 tasks before pushing a smaller number of entities deeper into later stages.

For pipeline UX and early validation, Beehive should prefer moving fresh descendants deeper through the pipeline.

### 5.2 Desired policy

Add scheduling policy support:

```yaml
runtime:
  scheduling_policy: depth_first
```

Allowed values for B14:

```text
depth_first
fifo
```

Default for new S3 workspaces should be:

```text
depth_first
```

Old configs without this field should default safely. Prefer `depth_first` for S3 workspaces and `fifo` for legacy local mode if needed. Document the behavior.

### 5.3 Depth-first ordering

Implement a better ordering for eligible task claim.

Minimum acceptable B14 ordering:

```text
prefer tasks whose entity_file.producer_run_id IS NOT NULL;
then prefer more recently updated/discovered tasks;
then stable id ordering.
```

This prioritizes child artifacts created by previous stages and helps push a subset through the pipeline.

Suggested SQL ordering:

```sql
ORDER BY
  CASE WHEN file.producer_run_id IS NOT NULL THEN 0 ELSE 1 END ASC,
  state.updated_at DESC,
  state.id DESC
```

If the current schema has a better timestamp for “fresh child artifact”, use it and document why.

### 5.4 Avoid starvation

Depth-first can starve old source files if new child tasks keep appearing.

B14 should include a simple anti-starvation mechanism or at least a clear design for one.

Acceptable MVP options:

1. Add `fifo`/`depth_first` switch, and document that `fifo` can be used for bulk complete processing.
2. Add a simple aging rule:
   ```text
   tasks older than N minutes get boosted
   ```
3. Add a small breadth quota:
   ```text
   after K depth-first claims, claim one oldest pending source task
   ```

If only option 1 is implemented in B14, explain the remaining starvation risk in feedback.

### 5.5 Tests

Add tests showing:

```text
fifo claims older stage_0 before newer children;
depth_first claims fresh child/downstream task before old source task;
depth_first still respects resource_class;
depth_first still excludes archived entities/missing files/active leases.
```

## 6. SQLite lock hardening

### 6.1 Problem

Real small run reported about 10 `database is locked` errors around 150 tasks. B13 added WAL and `busy_timeout = 5000`, but that is not enough under worker concurrency.

### 6.2 Required changes

Add configurable SQLite busy timeout:

```text
BEEHIVE_SQLITE_BUSY_TIMEOUT_MS=30000
```

Default should be at least:

```text
30000
```

B13's 5000ms is too small for concurrent writer pressure.

Apply it consistently to writable and readonly workspace connections.

### 6.3 Write retry/backoff

Add a small helper for transient SQLite lock errors around critical write transactions.

Preferred behavior:

```text
if error is SQLITE_BUSY / database is locked:
  sleep small backoff
  retry limited times
else:
  return error
```

Suggested defaults:

```text
max retries: 5
backoff: 50ms, 100ms, 200ms, 400ms, 800ms + jitter if easy
```

Do not retry every arbitrary DB error. Only retry clear lock/busy cases.

### 6.4 Optional workspace write gate

If lock errors persist or if implementation is straightforward, add an in-process per-workspace write gate/mutex for short critical DB write sections.

Important: do not hold this mutex while waiting for n8n or S3. Only hold it for local SQLite writes.

This preserves parallel n8n/LLM execution while serializing short DB writes that SQLite would serialize anyway.

If not implemented in B14, document it as a B15 candidate.

### 6.5 Tests

Add tests for:

```text
busy timeout env parsing;
SQLite connection uses configured busy_timeout;
lock/busy detection helper recognizes SQLITE_BUSY/database is locked;
write retry helper retries busy errors and stops after limit.
```

If true concurrency DB tests are flaky, prefer deterministic unit tests around retry helper and one integration smoke.

## 7. Retry policy with queue demotion

### 7.1 User expectation

The current punishment for many errors feels too harsh: too many tasks become `blocked`. For production-scale runs, transient errors should generally retry later, and retry tasks should be lowered in queue priority so healthy tasks go first.

### 7.2 Important distinction

Do not make everything retry forever.

Use this split:

#### Retryable/transient

```text
network timeout
n8n timeout
HTTP 5xx
local LLM timeout/overload
S3 transient error
SQLite database is locked / busy
temporary worker failure
```

These should go to:

```text
retry_wait
```

and be demoted behind normal pending work.

#### Deterministic/contract/operator-fix needed

```text
invalid manifest root
wrong schema
workspace_id/run_id/source mismatch
unsafe save_path
unknown save_path route
output cardinality violation when stage config forbids it
missing required S3 metadata
stage has no workflow URL
invalid stage config
```

These should still become:

```text
blocked
```

because retrying the same broken contract usually wastes LLM time.

### 7.3 Empty outputs nuance

Empty outputs are not always an error.

Current stage cardinality rules should remain:

```text
allow_zero_outputs=true  => zero outputs can be success
allow_zero_outputs=false => zero outputs violates stage contract
```

If zero output violates stage contract, default B14 behavior should remain blocked, not retry, because this is usually a workflow/stage configuration mismatch.

However, the UI must make manual retry/reset easy after the operator changes stage settings or fixes n8n.

### 7.4 Retry demotion ordering

Retry tasks should not immediately compete with healthy pending tasks.

Claim ordering should prefer:

```text
1. pending tasks
2. due retry_wait tasks
```

Within each group, apply scheduling policy (`depth_first` or `fifo`).

Suggested SQL concept:

```sql
ORDER BY
  CASE WHEN state.status = 'pending' THEN 0 ELSE 1 END ASC,
  ...scheduling policy...
```

This satisfies the product expectation:

```text
first let normal tasks work, then come back to retry tasks.
```

### 7.5 Retry backoff

Keep existing `retry_delay_sec`, but add or document exponential backoff if not too invasive.

MVP acceptable:

```text
retry_wait uses existing retry_delay_sec;
retry_wait is demoted behind pending work;
manual retry/reset can move back to pending.
```

Better:

```text
next_retry_at = now + retry_delay_sec * attempt_no or exponential capped delay
```

Do not overbuild if it risks breaking B12/B13 stability.

### 7.6 Manual retry controls

Make it easy for operator to retry after fixing things.

Expose in UI, if not already clear:

```text
Retry selected failed/blocked
Reset selected failed/blocked to pending
Retry this entity/stage
```

All manual retry actions must create app events with operator-visible reason/comment if available.

### 7.7 Tests

Add tests for:

```text
transient timeout -> retry_wait;
HTTP 5xx -> retry_wait;
manifest blocked route -> blocked;
output cardinality violation -> blocked;
SQLite busy in write helper -> retry write, not blocked task if eventually succeeds;
claim ordering puts pending before due retry_wait;
manual reset from blocked -> pending still works.
```

## 8. UI expectations

### 8.1 Worker controls

In workspace UI, show:

```text
Worker runtime status: stopped / running / draining / paused
Default workers desired/active
Local LLM workers desired/active
Pending default
Pending local_llm
Running default
Running local_llm
Retry_wait default/local_llm
Blocked/failed default/local_llm
```

Primary buttons:

```text
Start workers
Stop workers
```

Secondary controls:

```text
Pause default
Resume default
Pause local_llm
Resume local_llm
Recover expired leases
```

Do not bury Start/Stop in diagnostics.

### 8.2 Queue ordering explanation

Show current scheduling policy:

```text
Scheduling: depth-first
```

Small help text:

```text
Depth-first prioritizes fresh child artifacts so a subset of entities moves deeper through the pipeline sooner.
```

### 8.3 Error and retry visibility

When tasks enter retry:

```text
show retry count
show next_retry_at
show last error
show that retry tasks are lower priority than normal pending tasks
```

## 9. API expectations

Add or extend endpoints as needed.

Recommended API shape:

```text
GET  /api/workspaces/{workspace_id}/workers/summary
POST /api/workspaces/{workspace_id}/workers/start
POST /api/workspaces/{workspace_id}/workers/stop
PATCH /api/workspaces/{workspace_id}/workers/pools/default
PATCH /api/workspaces/{workspace_id}/workers/pools/local_llm
POST /api/workspaces/{workspace_id}/workers/pause
POST /api/workspaces/{workspace_id}/workers/resume
POST /api/workspaces/{workspace_id}/workers/pools/{resource_class}/pause
POST /api/workspaces/{workspace_id}/workers/pools/{resource_class}/resume
POST /api/workspaces/{workspace_id}/workers/recover-expired-leases
```

Request for start:

```json
{
  "default_workers": 5,
  "local_llm_workers": 1
}
```

Stop request can be empty or:

```json
{
  "mode": "drain"
}
```

Do not expose force kill in B14.

## 10. Production pilot expectations

B14 should include a bounded pilot or a clearly documented reason if it cannot run.

Preferred pilot:

```text
workspace: dedicated test workspace or explicitly bounded itg_documents subset
workers: default=3..5, local_llm=1
scheduling_policy=depth_first
run size: 300-1000 claimed tasks max
measure: database lock count, succeeded, retry_wait, blocked, failed, throughput, max active leases
```

If using `itg_documents`, do not run the full workspace. Use explicit controls/selection/subset. If subset-limited workers are not implemented, do not run production pilot and state why.

## 11. Documentation

Create/update:

```text
docs/beehive_s3_b14_worker_runtime_control_plan.md
docs/beehive_s3_b14_worker_runtime_control_feedback.md
docs/worker_runtime_control_runbook.md
docs/worker_retry_policy.md
```

Runbook must explain:

```text
how to start workers from UI;
how to stop workers safely;
how desired concurrency interacts with env/YAML;
how depth-first scheduling works;
how retries are demoted;
how to investigate database is locked;
safe pilot procedure for itg_documents.
```

## 12. Verification commands

Run and report exact results:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
rg "@tauri-apps/api/core|invoke\(" src -n
git diff --check
```

If any command cannot run, explain exactly why.

## 13. Acceptance criteria

B14 is accepted only if:

```text
1. Operator can set desired default/local_llm worker counts from web UI.
2. Operator can Start workers from web UI.
3. Operator can Stop workers from web UI without killing running tasks by default.
4. Worker claim respects desired worker counts and resource_class.
5. Existing env/YAML limits still act as guardrails.
6. Scheduler supports depth_first and fifo.
7. Depth-first prioritizes fresh child/downstream tasks.
8. Pending tasks are preferred over retry_wait tasks.
9. SQLite busy_timeout is configurable and defaults higher than B13.
10. SQLite busy/locked write errors get limited retry/backoff where appropriate.
11. `database is locked` frequency is measured in smoke/pilot or feedback states why it could not be measured.
12. Transient errors go to retry_wait where appropriate.
13. Deterministic contract/config errors still become blocked.
14. Manual retry/reset from failed/blocked remains available.
15. Broad-run bypass remains closed while workers are enabled.
16. itg_documents is not accidentally fully run/reset/damaged.
```

## 14. Non-goals

Do not implement in B14:

```text
RabbitMQ
Kafka
Postgres migration
force-kill of running n8n executions
full distributed worker fleet
RBAC
large unbounded 22k production run
n8n workflow editor
full priority system with arbitrary per-task priorities
```

## 15. Product principle

The operator should not think in waves anymore.

The operator should think:

```text
How many workers do I want?
Should local LLM be limited?
Is the queue healthy?
Should I stop/pause/retry?
```

Beehive should do the queue mechanics.
