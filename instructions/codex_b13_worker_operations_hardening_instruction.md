# B13. Worker Operations Hardening, Queue UI, and Safe Production Pilot

## 0. Контекст

B11 добавил ресурсную модель:

```text
stage.resource_class = default | local_llm
runtime.worker_pools.default.concurrency
runtime.worker_pools.local_llm.concurrency
```

B12 добавил первый настоящий worker runtime:

```text
worker_leases
resource-class-aware claim
heartbeat
expired lease recovery
worker manager inside beehive-server
basic worker diagnostics
```

B12 принимается как foundation, но его ещё нельзя бездумно включать на большой workspace `itg_documents` с 22 000 документов. B13 должен сделать operational hardening: закрыть обход worker-pool лимитов, добавить UI для очереди/leases, добавить pause/resume, ручные действия с stuck leases, SQLite hardening и controlled pilot.

B13 не должен добавлять RabbitMQ, Kafka, Postgres, RBAC или новый n8n workflow manager. Это этап доведения текущей DB-backed worker модели до безопасного операционного уровня.

## 1. Обязательное правило

`itg_documents` — важный production-scale workspace. Его нельзя удалять, архивировать, сбрасывать, импортировать заново, чистить или использовать для destructive smoke.

Любые smoke/test/pilot действия должны выполняться на отдельном test workspace или на явно указанном small subset. Если агенту нужно проверить `itg_documents`, он может только читать metadata/summary, но не запускать массовую обработку без явного указания.

Старые удалённые документы не восстанавливать.

## 2. Как работать агенту

Перед кодом создать план:

```text
docs/beehive_s3_b13_worker_operations_plan.md
```

В плане описать:

```text
1. Что уже есть после B12.
2. Какие риски B12 закрывает B13.
3. Как будет закрыт bypass broad run actions.
4. Как будет устроен Worker/Queue UI.
5. Как будут работать pause/resume pools.
6. Как будут работать manual lease actions.
7. Как будет усилен SQLite.
8. Как будет выполнен controlled pilot.
9. Что не будет реализовано в B13.
10. Как защищается itg_documents.
```

Во время работы перечитывать ТЗ на checkpoints:

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

После работы создать:

```text
docs/beehive_s3_b13_worker_operations_feedback.md
docs/worker_operations_runbook.md
```

Feedback должен содержать checkpoint line:

```text
ТЗ перечитано на этапах: after_plan, after_b12_review, after_bypass_design, after_pause_resume_design, after_lease_actions_design, after_queue_ui_design, after_sqlite_hardening, after_tests, after_pilot, before_feedback
```

## 3. Главные цели B13

B13 считается успешным, если:

```text
1. Broad manual run actions больше не могут незаметно обходить worker-pool limits.
2. Оператор видит очередь и worker leases по resource_class.
3. Оператор может pause/resume worker pools.
4. Оператор может release/recover stuck/expired/anomalous leases.
5. SQLite подготовлен к нескольким worker threads.
6. Есть controlled pilot на малом subset, не на всём itg_documents.
7. Есть runbook, как безопасно включать workers.
```

## 4. Закрыть bypass worker-pool лимитов

### 4.1 Проблема

После B12 worker claims учитывают `stage.resource_class`, но старые ручные broad actions могут использовать старый claim path:

```text
run_due_tasks
run_pipeline_waves
run_small_batch
```

Если эти действия доступны в UI рядом с workers, оператор может случайно запустить `local_llm` stage вне `local_llm` pool и нарушить гарантию “не больше N local LLM задач”.

### 4.2 Требование

Добавить runtime policy:

```text
When workers are enabled for a workspace, broad manual run actions must not bypass worker pools.
```

Возможные допустимые реализации:

#### Вариант A — предпочтительный

Broad run endpoints становятся resource-aware и используют worker lease claim path.

```text
run_small_batch(resource_class?)
run_pipeline_waves(resource_class?)
```

Если resource_class не указан, broad action должен либо:

```text
- запускать только default tasks, но не local_llm;
- либо отказывать с понятной ошибкой when workers enabled.
```

#### Вариант B — проще и безопаснее для B13

Если workers enabled for workspace, broad manual run endpoints возвращают ошибку:

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

UI должен скрыть/задизейблить broad actions и оставить:

```text
Run selected pipeline waves
Workers enabled controls
```

Выбор варианта — за агентом, но он должен объяснить в plan/feedback.

### 4.3 Selected run

`Run selected pipeline waves` можно оставить как operator debug/manual action, но он должен:

```text
- отказывать, если selected root уже имеет active worker lease;
- явно показывать warning, если selected run запускает local_llm stage while workers enabled;
- не запускать unrelated pending tasks.
```

Если проще, в B13 selected run при workers enabled можно разрешить только для roots без active lease и с confirmation.

## 5. Pause/resume worker pools

### 5.1 Модель

Добавить server-side состояние pause/resume по workspace и resource_class.

Минимально допустимо хранить в SQLite settings:

```text
worker_pool.default.paused = true|false
worker_pool.local_llm.paused = true|false
workers.paused_all = true|false
```

Или в отдельной таблице:

```text
worker_pool_controls(
  workspace_id? optional if per-db,
  resource_class,
  is_paused,
  pause_reason,
  updated_at
)
```

Так как БД уже per-workspace, можно хранить без workspace_id внутри workspace DB.

### 5.2 Поведение

Worker loop перед claim должен проверять pause state.

Если pool paused:

```text
- новые tasks не claim'ятся;
- уже выполняющиеся leases продолжают работу;
- UI показывает paused;
- resume возобновляет новые claims.
```

Нужны действия:

```text
Pause all workers
Resume all workers
Pause default
Resume default
Pause local_llm
Resume local_llm
```

### 5.3 API

Добавить endpoints:

```text
GET  /api/workspaces/{workspace_id}/workers/summary
POST /api/workspaces/{workspace_id}/workers/pause
POST /api/workspaces/{workspace_id}/workers/resume
POST /api/workspaces/{workspace_id}/workers/pools/{resource_class}/pause
POST /api/workspaces/{workspace_id}/workers/pools/{resource_class}/resume
```

Request body для pause:

```json
{
  "reason": "manual maintenance"
}
```

## 6. Worker/Queue UI

Добавить или расширить страницу/секцию:

```text
/workspaces/{workspace_id}/workers
```

Если отдельная route слишком велика, добавить явную панель `Workers & Queue` в workspace, но не прятать полностью в Diagnostics.

### 6.1 Что показывать

По каждому pool/resource_class:

```text
resource_class
configured_concurrency
active_leases
pending_count
retry_wait_count
blocked_count
failed_count
expired_leases
paused/resumed
oldest_pending_age
average_duration if available
last_error
```

Минимум для B13:

```text
Default pool: pending/running/expired/paused/concurrency
Local LLM pool: pending/running/expired/paused/concurrency
Recent leases table
```

### 6.2 Recent leases table

Колонки:

```text
lease_id short
worker_id short
entity_id
stage_id
resource_class
status
leased_at
lease_until
heartbeat_at
run_id
release_reason
actions
```

Actions:

```text
Release active lease
Recover expired leases
Open entity
Open stage run outputs if run_id exists
```

### 6.3 UI предупреждения

Если `local_llm` active leases >= concurrency:

```text
Local LLM pool is full. New local LLM tasks will wait in Beehive.
```

Если workers disabled:

```text
Workers are disabled. Set BEEHIVE_WORKERS_ENABLED=1 and BEEHIVE_WORKER_WORKSPACES=<workspace_id> to start background processing.
```

Если broad runs disabled:

```text
Broad manual runs are disabled while workers are enabled. Use selected run for debug or let workers process the queue.
```

## 7. Manual lease actions

### 7.1 Recover expired leases

Existing recovery exists. Expose it clearly in UI/API:

```text
POST /api/workspaces/{workspace_id}/workers/recover-expired-leases
```

Already present in B12; improve UI visibility and result summary.

### 7.2 Release anomalous active lease

Problem from B12 feedback: recovery may skip active lease whose `stage_run` is already finished, leaving active lease for manual diagnostics.

Add endpoint:

```text
POST /api/workspaces/{workspace_id}/workers/leases/{lease_id}/release
```

Request:

```json
{
  "reason": "manual_release_after_finished_run"
}
```

Rules:

```text
- If lease active and attached run is finished: release allowed.
- If lease active and heartbeat is fresh and run unfinished: reject unless force=true.
- If force=true: require reason and return strong warning.
- Releasing lease must not mark successful/failed stage state by itself unless recovery rules apply.
```

For B13, avoid dangerous force release if too much. Minimal safe version: release only active lease with finished run or expired lease.

### 7.3 Retry from failed/blocked

If already implemented from earlier stages, ensure it is visible from worker/queue context. If not, add minimal actions:

```text
Retry entity/stage
Retry selected failed/blocked
```

Rules:

```text
failed/blocked -> pending or retry_wait due now
attempts reset? Prefer explicit reset attempts if needed.
```

If changing attempts is too risky in B13, expose existing manual reset action and document limitations.

## 8. SQLite hardening

### 8.1 Problem

B12 introduces several worker threads writing to the same SQLite DB. Without write contention tuning, larger runs can hit:

```text
database is locked
```

### 8.2 Requirements

In `open_connection` or DB bootstrap, configure:

```sql
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000; -- or 10000
```

Consider WAL for server/workspace DB:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
```

Use WAL only if safe in the current deployment. If not enabling WAL, explain why in feedback.

Acceptance minimum:

```text
busy_timeout is set for every connection.
```

Preferred:

```text
busy_timeout + WAL for writable workspace DBs.
```

### 8.3 Tests

Add a lightweight concurrency test or at least unit test that connection pragmas are applied.

If true multi-thread SQLite test is flaky, keep it small:

```text
spawn several threads inserting app events / leases with busy_timeout enabled
assert no database locked under small load
```

## 9. Worker summary metrics

Extend worker summary to include queue counts per resource_class.

Needed counts:

```text
pending
retry_wait_due
retry_wait_not_due
queued
in_progress
blocked
failed
active_leases
expired_leases
```

The counts should respect:

```text
stage.resource_class
stage.is_active
entity.is_archived = 0
file exists
```

UI should use these counts.

## 10. Controlled pilot

Do not run all `itg_documents`.

B13 pilot options:

### Option A — test workspace

Create/use test workspace with 20–100 files.

Run:

```text
default concurrency 3-5
local_llm concurrency 1
```

### Option B — itg_documents read-only + explicit subset

Only if explicitly safe and already prepared:

```text
select 50-200 known pending docs from itg_documents
run with workers scoped to itg_documents
local_llm=1
default=3-5
stop after subset is done
```

If B13 code does not yet support subset-limited worker processing, do not use `itg_documents` for execution. Use a test workspace.

Pilot report must include:

```text
workspace_id
worker env
pool config
number of tasks processed
success/retry/failed/blocked
local_llm max observed active leases
default max observed active leases
throughput
errors
whether S3/n8n real calls were made
```

Create:

```text
docs/beehive_s3_b13_worker_pilot_report.md
```

If no real pilot is possible, say blocked and explain exact missing input.

## 11. API changes

Add/update these routes:

```text
GET  /api/workspaces/{workspace_id}/workers/summary
POST /api/workspaces/{workspace_id}/workers/recover-expired-leases
POST /api/workspaces/{workspace_id}/workers/pause
POST /api/workspaces/{workspace_id}/workers/resume
POST /api/workspaces/{workspace_id}/workers/pools/{resource_class}/pause
POST /api/workspaces/{workspace_id}/workers/pools/{resource_class}/resume
POST /api/workspaces/{workspace_id}/workers/leases/{lease_id}/release
```

Do not expose unsafe mass actions without confirmation.

## 12. Environment/runbook

Create/update:

```text
docs/worker_operations_runbook.md
```

Include exact safe startup examples:

```bash
BEEHIVE_WORKERS_ENABLED=1 \
BEEHIVE_WORKER_WORKSPACES=test_worker_pilot \
BEEHIVE_WORKER_DEFAULT_CONCURRENCY=5 \
BEEHIVE_WORKER_LOCAL_LLM_CONCURRENCY=1 \
cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
```

For `itg_documents`, include warning:

```text
Do not use BEEHIVE_WORKER_WORKSPACES=all for production pilot.
Use BEEHIVE_WORKER_WORKSPACES=itg_documents only when ready.
Start with conservative concurrency.
```

## 13. Tests required

Backend tests:

```text
workers disabled by default
workers require explicit workspace scope
pool pause prevents claim
pool resume allows claim
pause default does not pause local_llm
pause local_llm does not pause default
active lease prevents broad/manual claim or returns clear error
broad run disabled/resource-aware when workers enabled
release finished active lease succeeds
release fresh active unfinished lease rejects
recover expired lease result exposed via API
worker summary includes queue counts by resource_class
busy_timeout pragma applied
```

Existing tests must still pass:

```text
resource_class parse/default
worker leases claim separation
double claim protection
heartbeat wrong-worker rejection
selected runner
entity upload
stage CRUD
manifest partial registration
```

Frontend/build:

```bash
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
```

Rust:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

Other:

```bash
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
rg "@tauri-apps/api/core|invoke\(" src -n
git diff --check
```

## 14. Feedback requirements

Create:

```text
docs/beehive_s3_b13_worker_operations_feedback.md
```

Feedback must include:

```text
1. What changed.
2. Files changed.
3. How broad-run bypass is prevented.
4. How pause/resume works.
5. How manual lease release works.
6. How worker summary counts are computed.
7. What SQLite hardening was applied.
8. Whether WAL was enabled or not, and why.
9. Tests run and exact results.
10. Pilot result or reason pilot was blocked.
11. Whether itg_documents was touched.
12. Remaining risks.
13. What should be done in B14.
```

Required checkpoint line:

```text
ТЗ перечитано на этапах: after_plan, after_b12_review, after_bypass_design, after_pause_resume_design, after_lease_actions_design, after_queue_ui_design, after_sqlite_hardening, after_tests, after_pilot, before_feedback
```

## 15. Acceptance criteria

B13 is accepted only if:

```text
1. Broad manual runs cannot silently bypass worker-pool limits when workers are enabled.
2. Operator can see worker/queue state by default/local_llm.
3. Operator can pause/resume all workers and individual pools.
4. Operator can recover expired leases from UI/API.
5. Operator can safely release anomalous active leases with finished runs.
6. SQLite busy_timeout is applied; WAL decision is documented.
7. Worker summary includes pending/running/expired counts by resource_class.
8. itg_documents is not modified by tests/smoke unless explicitly part of a controlled pilot.
9. Workers remain disabled by default.
10. Existing B12 worker execution still works.
11. Tests/build pass.
12. Pilot report exists or clearly explains why pilot was blocked.
```

## 16. Non-goals

Do not implement in B13:

```text
RabbitMQ
Kafka
Postgres migration
RBAC
new scheduler architecture from scratch
large 22k production run
n8n REST workflow editor
full visual queue dashboard polish
automatic scaling
complex priority system
```

## 17. Product principle

B13 is about operational safety.

Before Beehive processes thousands of documents, the operator must be able to answer:

```text
What is running?
Why is it waiting?
Which pool is full?
Can I pause it?
Can I recover stuck work?
Can I retry safely?
Am I accidentally bypassing local_llm limits?
```
