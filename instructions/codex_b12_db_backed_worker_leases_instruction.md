# B12. DB-backed Worker Leases and Internal Worker Pools

## 0. Контекст

Beehive уже умеет создавать workspaces, stages, импортировать/регистрировать сущности, запускать selected pipeline waves и обрабатывать n8n/S3 manifest contract. B11 добавил foundation для параллелизма:

```text
stage.resource_class = default | local_llm
runtime.worker_pools.default.concurrency
runtime.worker_pools.local_llm.concurrency
```

B12 должен сделать следующий шаг: превратить эту модель в реальную внутреннюю очередь Beehive с leases, heartbeat и worker pools.

Важно: это не этап RabbitMQ/Kafka/Postgres. Сейчас делаем внутреннюю DB-backed очередь на текущей SQLite/state-machine базе. Beehive остаётся владельцем параллелизма. n8n остаётся исполнителем workflow.

## 1. Важные решения от владельца проекта

### 1.1 Старые документы

Старые docs/instructions были удалены владельцем проекта осознанно, чтобы не размывать контекст исполнителя. Не восстанавливать их. Не переносить их в archive. Не тратить время на это.

Актуальная документация должна быть компактной и соответствовать текущей S3/web/worker-pool архитектуре.

### 1.2 Workspace `itg_documents`

`itg_documents` — важный передовой workspace, не мусор. На нём уже начат большой прогон примерно 22 000 документов, около 1000 уже обработаны/разобраны.

B12 не должен удалять, переименовывать, сбрасывать или ломать этот workspace. Любые миграции должны быть backward-compatible и безопасны для уже накопленных данных.

Если B12 добавляет schema migration, она обязана корректно проходить на существующих SQLite DB, включая `itg_documents`.

### 1.3 Старые удалённые docs

Ничего не делать со старыми удалёнными docs. Считай это завершённым cleanup.

## 2. Главная цель B12

Добавить внутренний worker layer, который:

```text
1. Берёт pending/retry_wait задачи из DB.
2. Учитывает stage.resource_class.
3. Не даёт двум workers взять одну задачу.
4. Ограничивает параллелизм по runtime.worker_pools.
5. Поддерживает lease/heartbeat.
6. Восстанавливает stuck/expired leases.
7. Выполняет существующий n8n executor path.
8. Не ломает selected/manual runs.
```

Целевая модель:

```text
Default workers:    обрабатывают stages с resource_class=default
Local LLM workers:  обрабатывают stages с resource_class=local_llm
```

Пример:

```yaml
runtime:
  worker_pools:
    default:
      concurrency: 10
    local_llm:
      concurrency: 1
```

Это должно означать:

```text
до 10 default задач одновременно;
до 1 local_llm задачи одновременно;
local_llm задачи не захватываются default workers;
default задачи не захватываются local_llm workers, если явно не добавлен fallback mode, но в B12 fallback не нужен.
```

## 3. Перед началом работы

Перед кодом создай план:

```text
docs/beehive_s3_b12_worker_leases_plan.md
```

План должен содержать:

```text
1. Что понял из B12.
2. Какие B11 pieces используешь.
3. Где сейчас claim/runtime execution работает.
4. Где и как добавишь lease/heartbeat.
5. Как будешь фильтровать задачи по resource_class.
6. Как worker manager будет запускаться/останавливаться.
7. Как защитишь itg_documents от destructive changes.
8. Какие tests добавишь.
9. Какие smoke/verification commands запустишь.
10. Что точно не входит в B12.
```

Не писать runtime code до создания плана.

Обязательные checkpoints перечитывания ТЗ:

```text
after_plan
after_current_runtime_review
after_lease_schema_design
after_claim_logic_design
after_worker_manager_design
after_backend_changes
after_tests
after_smoke
before_feedback
```

В feedback добавь строку:

```text
ТЗ перечитано на этапах: after_plan, after_current_runtime_review, after_lease_schema_design, after_claim_logic_design, after_worker_manager_design, after_backend_changes, after_tests, after_smoke, before_feedback
```

## 4. Что прочитать

Обязательно прочитать:

```text
instructions/00_beehive_worker_pools_global_vision.md
instructions/01_codex_agent_working_rules.md
instructions/02_b11_resource_classes_worker_pool_config_requirements.md

docs/beehive_s3_b11_resource_classes_worker_pools_feedback.md
docs/worker_pools_architecture.md

docs/beehive_s3_b10_runtime_contract_hardening_feedback.md
docs/n8n_s3_manifest_contract.md

docs/beehive_s3_b9_entities_upload_simplified_crud_feedback.md

docs/beehive_s3_b8_crud_ui_feedback.md
```

Кодовые зоны:

```text
src-tauri/src/domain/mod.rs
src-tauri/src/config/mod.rs
src-tauri/src/database/mod.rs
src-tauri/src/executor/mod.rs
src-tauri/src/services/selected_runner.rs
src-tauri/src/services/runtime.rs, если есть
src-tauri/src/http_api/mod.rs
src-tauri/src/bin/beehive-server.rs
src-tauri/src/http_server.rs
src/pages/WorkspaceExplorerPage.tsx
src/pages/StageEditorPage.tsx
src/types/domain.ts
src/lib/apiClient/*
src/lib/runtimeApi.ts
```

Если каких-то docs после cleanup больше нет, не восстанавливай старые файлы. Работай по текущему актуальному коду и актуальным B10/B11 docs.

## 5. Non-goals B12

Не делать:

```text
RabbitMQ
Kafka
Postgres migration
full async manifest polling
n8n REST workflow editor
RBAC/auth redesign
production 22k run from tests
UI redesign
large README rewrite
full cleanup of all legacy code
```

B12 — это internal worker pools, leases, heartbeat, recovery, tests.

## 6. Lease model

### 6.1 Рекомендуемый подход

Добавить отдельную таблицу для leases, не смешивать lease полностью с `entity_stage_states`.

Пример schema:

```sql
CREATE TABLE worker_leases (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  lease_id TEXT NOT NULL UNIQUE,
  state_id INTEGER NOT NULL,
  entity_id TEXT NOT NULL,
  entity_file_id INTEGER NOT NULL,
  stage_id TEXT NOT NULL,
  resource_class TEXT NOT NULL,
  worker_id TEXT NOT NULL,
  status TEXT NOT NULL,
  run_id TEXT,
  leased_at TEXT NOT NULL,
  lease_until TEXT NOT NULL,
  heartbeat_at TEXT NOT NULL,
  released_at TEXT,
  release_reason TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

Допустимые lease status:

```text
active
done
failed
expired
released
```

Можно упростить, если текущая DB architecture требует более компактную схему, но должны быть:

```text
lease_id
state_id
entity_file_id
stage_id
resource_class
worker_id
lease_until
heartbeat_at
status/release reason
```

### 6.2 Почему lease нужен

Нельзя просто ставить `status=in_progress` и надеяться, что worker завершится. Если процесс умер, задача должна восстановиться.

Lease позволяет:

```text
1. Защитить от двойного claim.
2. Понять, какой worker взял задачу.
3. Продлевать долгую задачу heartbeat’ом.
4. Восстановить задачу после падения worker’а.
5. Показать оператору stuck/running jobs.
```

## 7. Claim logic

### 7.1 Resource-aware claim

Добавить claim function, которая принимает:

```rust
resource_class: ResourceClass
limit: u64
worker_id: &str
lease_ttl_sec: u64
now: DateTime<Utc>
```

Она должна выбирать только eligible задачи, где stage.resource_class совпадает с requested pool.

Eligible states:

```text
pending
retry_wait where next_retry_at <= now
```

Не брать:

```text
done
in_progress
queued with active lease
blocked
failed
skipped
archived entities
missing files
inactive stages
stages without workflow_url
```

### 7.2 Atomic claim

Claim должен быть атомарным:

```text
transaction start
select candidates
update state status queued/in_progress or leased marker
insert worker_lease
commit
```

Нельзя, чтобы два workers получили одну и ту же state.

### 7.3 Status transition

Можно оставить текущую модель:

```text
pending/retry_wait -> queued -> in_progress -> done/retry_wait/failed/blocked
```

Но lease должен быть связан с queued/in_progress.

Предпочтительно:

```text
claim: pending/retry_wait -> queued + active lease
execution start: queued -> in_progress
finish: in_progress -> done/retry_wait/failed/blocked + lease done/failed/released
```

Если executor уже переводит status, не ломай его. Встраивай lease вокруг существующего пути минимально.

## 8. Heartbeat

Worker, выполняющий долгий n8n call, должен heartbeat’ить lease.

Минимально:

```text
heartbeat interval = runtime.worker_heartbeat_sec или default 30 sec
lease_until = now + worker_lease_sec
heartbeat_at = now
```

Добавить в runtime config, если нет:

```yaml
runtime:
  worker_lease_sec: 1800
  worker_heartbeat_sec: 30
```

Default:

```text
worker_lease_sec = max(request_timeout_sec + 300, 1800)
worker_heartbeat_sec = 30
```

Если не хочешь менять YAML в B12, можно задать internal defaults, но лучше добавить config поля с backward compatibility.

## 9. Lease recovery

Добавить recovery function:

```text
recover_expired_worker_leases(now)
```

Она должна:

```text
1. Найти active leases where lease_until < now.
2. Пометить lease expired.
3. Если state всё ещё queued/in_progress и stage_run не завершён, перевести state в retry_wait или pending согласно retry policy.
4. Записать app_event worker_lease_expired.
```

Важно: не делать recovery агрессивным. У LLM stages могут быть долгие executions. Поэтому heartbeat обязателен, а lease timeout должен быть достаточно большим.

## 10. Worker manager

### 10.1 Запуск

Добавить internal worker manager в `beehive-server`.

Предложение по env:

```bash
BEEHIVE_WORKERS_ENABLED=1
BEEHIVE_WORKER_DEFAULT_CONCURRENCY=10
BEEHIVE_WORKER_LOCAL_LLM_CONCURRENCY=1
```

Если env не задан:

```text
workers disabled by default
```

Почему disabled by default: сейчас есть production-scale `itg_documents`, и нельзя случайно при запуске сервера начать молотить 22 000 документов без явного согласия.

### 10.2 Workspace scope

Workers должны работать по workspace.

Варианты:

```text
BEEHIVE_WORKER_WORKSPACES=itg_documents
BEEHIVE_WORKER_WORKSPACES=all
```

Default для B12:

```text
no workspace selected -> workers do not start
```

Это важно, чтобы test/dev server не начал работать по всем workspace.

### 10.3 Worker loop

Каждый worker loop:

```text
while server running:
  recover expired leases periodically
  claim one task for its resource_class
  if no task: sleep short interval
  if task: execute existing executor path with lease heartbeat
  finish lease
```

Не писать новый n8n executor. Использовать существующий proven execution path.

### 10.4 Worker IDs

Worker id формат:

```text
{hostname_or_process}-{resource_class}-{index}-{short_uuid}
```

Не обязательно идеально, но в logs/UI должно быть понятно, какой worker взял задачу.

## 11. Pool concurrency enforcement

На B12 нужно гарантировать:

```text
default pool не превышает configured concurrency
local_llm pool не превышает configured concurrency
```

Если `local_llm.concurrency = 1`, то Beehive не должен одновременно выполнять две задачи со stage.resource_class=local_llm.

Если `concurrency = 0`, pool считается disabled.

B11 уже парсит 0..=128. В B12 `0` должен реально отключать pool.

## 12. Existing manual/selected runs

Не ломать:

```text
Run selected pipeline waves
Run small batch
Manual entity stage run
S3 reconcile
Entity import
```

В B12 эти действия могут оставаться синхронными/manual. Worker manager — отдельный режим.

Важно: если worker уже держит lease на задачу, manual selected run не должен взять эту же задачу.

Если manual run запускает задачу, которую worker мог бы взять, нужно либо:

```text
1. manual run uses the same lease mechanism;
```

или:

```text
2. manual run refuses if active lease exists.
```

Лучше вариант 1, но если объём большой, вариант 2 допустим для B12. Обязательно задокументировать.

## 13. Retry/manual recovery

B12 должен сохранить или добавить возможность оператору вручную вернуть failed/blocked в retry/pending.

Минимум:

```text
failed -> pending
blocked -> pending
retry_wait -> pending now
```

Через существующие manual reset/retry endpoints, если они уже есть.

Добавить защиту:

```text
cannot manually reset task with active lease
cannot manually reset in_progress task unless force_release_stuck and lease expired
```

## 14. UI expectations for B12

B12 не обязан делать полноценную красивую Queue UI, но должен дать минимальную видимость.

Минимум в Workspace/Diagnostics или отдельной простой панели:

```text
Worker pools:
  default concurrency configured
  local_llm concurrency configured
  active leases by resource_class
  expired leases count
  last recovery at
```

Если UI слишком много для B12, хотя бы добавить API endpoint:

```text
GET /api/workspaces/{workspace_id}/workers/summary
```

И простую диагностику в UI можно оставить на B13.

## 15. HTTP/API endpoints

Добавить backend routes или service functions:

```text
GET  /api/workspaces/{workspace_id}/workers/summary
POST /api/workspaces/{workspace_id}/workers/recover-expired-leases
```

Если workers управляются env-only, start/stop endpoints можно отложить. Но summary нужен для проверки.

Не добавлять public endpoint, который случайно запускает workers на всех workspace без явного config.

## 16. Tests

Обязательные Rust tests:

### Claim and resource class

```text
claim default pool gets only default stages
claim local_llm pool gets only local_llm stages
default claim does not take local_llm
local_llm claim does not take default
concurrency=0 disables pool
```

### Double claim protection

```text
two claim attempts cannot lease same state
claimed state has exactly one active lease
manual claim refuses active lease or uses same lease mechanism
```

### Heartbeat

```text
heartbeat extends lease_until
heartbeat updates heartbeat_at
heartbeat for non-active lease fails safely
```

### Recovery

```text
expired active lease is marked expired
state returns to pending/retry_wait according to policy
non-expired lease is not recovered
finished run lease is not recovered
```

### Existing flows

```text
selected-run path still passes
entity import path still passes
stage creation resource_class still passes
B11 config parsing tests still pass
```

## 17. Smoke

Add a smoke script if feasible:

```text
scripts/worker_pool_smoke.mjs
```

Or a Rust integration test if JS smoke is too hard.

Smoke scenario:

```text
1. Create temp workspace.
2. Create stage_default and stage_local_llm.
3. Import/register a few fake source artifacts.
4. Start worker manager with default=2 local_llm=1 or call worker loop test harness.
5. Verify no more than 1 local_llm active lease at once.
6. Verify default and local_llm claims are separated.
7. Verify lease recovery works on a deliberately expired lease.
```

Do not use production `itg_documents` in automated smoke.

## 18. Verification commands

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

If any command cannot run, say exactly why.

## 19. Docs and feedback

Create:

```text
docs/beehive_s3_b12_worker_leases_plan.md
docs/beehive_s3_b12_worker_leases_feedback.md
docs/worker_leases_runtime_contract.md
```

Feedback must include:

```text
1. What was implemented.
2. Which files changed.
3. How B11 resource_class is used.
4. Lease schema and migration details.
5. Claim logic.
6. Heartbeat logic.
7. Recovery logic.
8. Worker manager env/config.
9. How itg_documents was protected from destructive changes.
10. Whether workers are disabled by default.
11. Tests run and exact results.
12. Smoke results.
13. Known risks.
14. What should be done in B13.
15. Required reread checkpoint line.
```

## 20. Acceptance criteria

B12 is accepted only if:

```text
1. DB schema supports worker leases.
2. Claim is resource-class aware.
3. Default workers do not claim local_llm stages.
4. Local_llm workers do not claim default stages.
5. Concurrency=0 disables a pool.
6. Two workers cannot claim the same task.
7. Heartbeat extends an active lease.
8. Expired leases can be recovered safely.
9. Existing selected/manual run flows are not broken.
10. Workers are disabled by default unless explicitly enabled.
11. Worker scope is explicit; no accidental processing of all workspaces.
12. itg_documents is not deleted, reset, or destructively changed.
13. Tests pass.
14. Feedback is honest.
```

## 21. What remains for B13

B13 should focus on UI and operational control:

```text
Worker/Queue page
pause/resume workers
manual retry/release stuck leases
queue metrics by resource_class
backpressure display
large-batch pilot on controlled subset
```

B12 should provide the reliable backend foundation for that.
