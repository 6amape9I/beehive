# B12 DB-backed Worker Leases Plan

## 1. Что понял из B12

B12 должен превратить B11 resource classes and worker pools в реальный внутренний worker layer на текущей SQLite DB:

- задачи берутся из `pending` / due `retry_wait`;
- claim учитывает `stage.resource_class = default | local_llm`;
- два workers не могут взять один `entity_stage_states` row;
- параллелизм ограничивается `runtime.worker_pools`;
- активная работа держится через DB lease и heartbeat;
- expired leases восстанавливаются безопасно;
- выполнение использует существующий executor/n8n/S3 manifest path;
- workers выключены по умолчанию и запускаются только по явному env + workspace scope.

B12 не должен запускать production прогон `itg_documents`, сбрасывать его состояния или делать destructive cleanup.

## 2. Какие B11 pieces использую

Из B11 уже есть foundation:

- Rust `ResourceClass` enum in `src-tauri/src/domain/mod.rs`;
- `StageDefinition.resource_class`, `StageRecord.resource_class`, `WorkspaceStageTree.resource_class`;
- `runtime.worker_pools.default.concurrency` and `runtime.worker_pools.local_llm.concurrency`;
- parser validation for known pools and `0..=128`;
- SQLite `stages.resource_class` in schema v9;
- S3 stage create/update support through `uses_local_llm`;
- UI/resource badges and B11 config tests.

B12 будет использовать эти поля как source of truth для queue partitioning. `concurrency=0` станет не только валидным config value, но и реально отключит соответствующий worker pool.

## 3. Где сейчас claim/runtime execution работает

Current runtime path:

- `src-tauri/src/services/runtime.rs` loads registered workspace context and calls executor.
- `src-tauri/src/executor/mod.rs::run_due_tasks` reconciles stuck tasks, calls `database::claim_eligible_runtime_tasks`, then executes each returned task.
- `src-tauri/src/database/mod.rs::claim_eligible_runtime_tasks` transactionally moves eligible `entity_stage_states` rows from `pending` / due `retry_wait` to `queued`.
- `executor::execute_task` requires `task.status == "queued"`, creates `stage_runs`, moves `queued -> in_progress`, then finishes as `done` / `retry_wait` / `failed` / `blocked`.
- S3 execution is already inside the same executor path via `execute_s3_task`.
- `src-tauri/src/services/selected_runner.rs` calls `executor::run_entity_stage`, which uses `claim_specific_runtime_task`.
- `src-tauri/src/bin/beehive-server.rs` currently starts only the HTTP server; there is no internal worker manager yet.

Important B12 constraint: selected/manual paths must not take a task with an active worker lease. I will make their claim path refuse active leases rather than adding a separate manual worker pool.

## 4. Где и как добавлю lease/heartbeat

Database:

- add schema v10 with `worker_leases`;
- add indexes for active leases by state/resource class/expiry;
- keep the lease as a separate table, not embedded in `entity_stage_states`.

Core fields:

- `lease_id`;
- `state_id`;
- `entity_id`;
- `entity_file_id`;
- `stage_id`;
- `resource_class`;
- `worker_id`;
- `status`;
- `run_id`;
- `leased_at`;
- `lease_until`;
- `heartbeat_at`;
- `released_at`;
- `release_reason`;
- `created_at`;
- `updated_at`.

Lease statuses:

- `active`;
- `done`;
- `failed`;
- `expired`;
- `released`.

Executor integration:

- extend `RuntimeTaskRecord` with optional `lease_id` / `worker_id`;
- active worker claim creates lease and sets state to `queued`;
- `start_claimed_stage_run` will attach the new `run_id` to the active lease;
- finish paths will release the lease as `done`, `failed`, or `released` alongside the existing state transition;
- preflight skips that return `queued -> pending` will release lease with `released`;
- heartbeat updates `heartbeat_at` and `lease_until` only for an active lease owned by the same worker.

For long blocking n8n calls, I will add a small heartbeat thread around executor execution for worker-owned tasks. Manual/synchronous runs will not need this thread unless they own a lease.

## 5. Как буду фильтровать задачи по resource_class

Add a resource-aware claim function that accepts:

```rust
resource_class: ResourceClass
limit: u64
worker_id: &str
lease_ttl_sec: u64
now: DateTime<Utc>
```

Eligible rows:

- `state.status = pending`;
- `state.status = retry_wait` with `next_retry_at <= now`;
- `stage.resource_class` equals requested pool;
- active stage;
- non-empty `workflow_url`;
- present source file;
- non-archived entity;
- attempts remain.

Excluded rows:

- `done`, `in_progress`, `queued` with active lease, `blocked`, `failed`, `skipped`;
- archived entities;
- missing files;
- inactive stages;
- stages without `workflow_url`;
- rows already having an active unexpired lease.

The current `claim_eligible_runtime_tasks` can either delegate to the same internal claim without leases for legacy manual batch behavior, or be adjusted to refuse active leases. Worker-manager claims will always use the lease-creating claim path.

## 6. Как worker manager будет запускаться/останавливаться

Add an internal worker manager under `src-tauri/src/services` or a dedicated module, then wire it from `beehive-server`.

Env behavior:

- `BEEHIVE_WORKERS_ENABLED=1` is required;
- no enabled flag means workers are disabled;
- `BEEHIVE_WORKER_WORKSPACES` is required and accepts explicit ids or `all`;
- missing workspace scope means no workers start;
- `BEEHIVE_WORKER_DEFAULT_CONCURRENCY` and `BEEHIVE_WORKER_LOCAL_LLM_CONCURRENCY` may override config for server startup, but effective pool concurrency must respect `runtime.worker_pools` and `0` disables the pool.

Loop shape:

- periodically recover expired leases for scoped workspaces;
- start N worker threads per resource class per workspace according to effective concurrency;
- each worker claims one task for its resource class;
- if idle, sleep briefly;
- if claimed, execute existing executor path with lease heartbeat;
- finish/release lease through database helpers.

Stopping:

- B12 can rely on process shutdown for server workers;
- no public start/stop endpoints in B12;
- no endpoint that starts workers across all workspaces implicitly.

## 7. Как защищу itg_documents от destructive changes

- No tests or smoke scripts will use the real `itg_documents` workspace.
- Schema migration will be additive and backward-compatible.
- Workers will be disabled by default.
- Worker scope is explicit; no scope means no workers.
- I will not reset, delete, archive, rename, or import into `itg_documents`.
- Recovery will only touch expired active leases and related active states; it will not sweep unrelated historical rows.
- I will not restore old deleted docs or instructions.

## 8. Какие tests добавлю

Rust tests:

- schema creates/migrates `worker_leases`;
- default claim gets only `default` stages;
- `local_llm` claim gets only `local_llm` stages;
- default does not take `local_llm`, and `local_llm` does not take default;
- pool claim with concurrency/effective limit `0` returns no tasks;
- two claim attempts cannot lease the same state;
- exactly one active lease exists for a claimed state;
- manual/specific claim refuses a task with active lease;
- heartbeat extends `lease_until` and updates `heartbeat_at`;
- heartbeat for non-active or wrong-worker lease fails safely;
- expired active lease is marked `expired`;
- expired active state returns to `pending` or `retry_wait` according to retry policy;
- non-expired lease is not recovered;
- completed/released lease is not recovered;
- existing selected-run, entity import, stage creation, and B11 config parsing tests continue to pass.

HTTP/API tests:

- `GET /api/workspaces/{workspace_id}/workers/summary`;
- `POST /api/workspaces/{workspace_id}/workers/recover-expired-leases`.

## 9. Какие smoke/verification commands запущу

Required commands:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
rg "@tauri-apps/api/core|invoke\(" src -n
git diff --check
```

Smoke:

- if feasible, add `scripts/worker_pool_smoke.mjs` against a temp workspace/server;
- otherwise add a Rust integration/test harness that creates temp data, claims default/local_llm tasks, verifies pool separation, and recovers a deliberately expired lease.

No smoke will touch `itg_documents`.

## 10. Что точно не входит в B12

- RabbitMQ;
- Kafka;
- Postgres migration;
- full async S3 manifest polling;
- n8n REST workflow editor;
- RBAC/auth redesign;
- production 22k run from tests;
- UI redesign or large queue page;
- large README rewrite;
- cleanup/restoration of old deleted docs;
- automatic worker start for every workspace;
- destructive changes to `itg_documents`;
- full pause/resume operational UI, which belongs to B13.

## Checkpoints

Planned reread checkpoints:

- after_plan
- after_current_runtime_review
- after_lease_schema_design
- after_claim_logic_design
- after_worker_manager_design
- after_backend_changes
- after_tests
- after_smoke
- before_feedback
