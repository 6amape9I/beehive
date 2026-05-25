# B12 DB-backed Worker Leases Feedback

## 1. What Was Implemented

- Added SQLite schema v10 with `worker_leases`.
- Added DB-backed active lease claim for worker pools.
- Added resource-class-aware claim for `default` and `local_llm`.
- Added lease heartbeat.
- Added expired lease recovery.
- Added worker manager startup in `beehive-server`, gated by env.
- Added worker diagnostics HTTP routes.
- Added minimal Worker Pools diagnostics panel in Workspace Explorer.
- Added `runtime.worker_lease_sec` and `runtime.worker_heartbeat_sec` with backward-compatible defaults.
- Preserved existing manual/selected flows and added active lease refusal.

## 2. Files Changed

Main backend:

- `src-tauri/src/domain/mod.rs`
- `src-tauri/src/config/mod.rs`
- `src-tauri/src/database/mod.rs`
- `src-tauri/src/executor/mod.rs`
- `src-tauri/src/http_api/mod.rs`
- `src-tauri/src/http_server.rs`
- `src-tauri/src/services/mod.rs`
- `src-tauri/src/services/workers.rs`
- `src-tauri/src/pipeline_editor/mod.rs`

Frontend/API:

- `src/types/domain.ts`
- `src/lib/runtimeApi.ts`
- `src/lib/apiClient/types.ts`
- `src/lib/apiClient/httpClient.ts`
- `src/lib/apiClient/tauriClient.ts`
- `src/pages/WorkspaceExplorerPage.tsx`

Docs:

- `docs/beehive_s3_b12_worker_leases_plan.md`
- `docs/worker_leases_runtime_contract.md`
- `docs/beehive_s3_b12_worker_leases_feedback.md`

Test/helper updates for new runtime config fields:

- `src-tauri/src/s3_reconciliation.rs`
- `src-tauri/src/services/selected_runner.rs`

## 3. How B11 Resource Class Is Used

Worker claim filters by `stages.resource_class`.

- `ResourceClass::Default` claims only `default` stages.
- `ResourceClass::LocalLlm` claims only `local_llm` stages.
- There is no fallback mode in B12.
- `runtime.worker_pools.*.concurrency = 0` disables spawning that pool.

## 4. Lease Schema and Migration Details

Schema version is now `10`.

New table:

```sql
worker_leases(
  lease_id,
  state_id,
  entity_id,
  entity_file_id,
  stage_id,
  resource_class,
  worker_id,
  status,
  run_id,
  leased_at,
  lease_until,
  heartbeat_at,
  released_at,
  release_reason,
  created_at,
  updated_at
)
```

Lease statuses:

- `active`
- `done`
- `failed`
- `expired`
- `released`

DB-level double-claim guard:

```sql
CREATE UNIQUE INDEX idx_worker_leases_one_active_state
ON worker_leases(state_id)
WHERE status = 'active';
```

Old DBs migrate v9 -> v10 additively. Existing stage/file/state/run rows are not deleted or rewritten.

## 5. Claim Logic

Worker claim transaction:

1. Selects eligible states for one `resource_class`.
2. Excludes archived entities, missing files, inactive stages, empty workflow URLs, exhausted attempts, and active leases.
3. Moves state to `queued`.
4. Inserts one `active` lease.
5. Returns the queued task with `lease_id` and `worker_id`.

Existing manual/specific claim now refuses active worker leases before it can skip the queued state.

## 6. Heartbeat Logic

Runtime config:

```yaml
runtime:
  worker_lease_sec: 1800
  worker_heartbeat_sec: 30
```

Defaults:

- `worker_lease_sec = max(request_timeout_sec + 300, 1800)`
- `worker_heartbeat_sec = 30`

Worker execution starts a heartbeat loop for leased tasks. Heartbeat succeeds only for the active lease owned by the same `worker_id`.

## 7. Recovery Logic

Recovery:

- finds `active` leases where `lease_until < now`;
- skips leases whose attached `stage_run` is already finished;
- marks expired leases as `expired`;
- returns `queued` states to `pending`;
- returns `in_progress` states to `retry_wait` if attempts remain, otherwise `failed`;
- writes `worker_lease_expired` app events.

The old stuck-task reconciliation now skips states with active worker leases, so lease recovery owns worker task recovery.

## 8. Worker Manager Env/Config

Workers are started from `beehive-server` only through env:

```bash
BEEHIVE_WORKERS_ENABLED=1
BEEHIVE_WORKER_WORKSPACES=workspace_id
BEEHIVE_WORKER_DEFAULT_CONCURRENCY=10
BEEHIVE_WORKER_LOCAL_LLM_CONCURRENCY=1
```

Defaults:

- workers disabled;
- no workspace scope means no workers start;
- `BEEHIVE_WORKER_WORKSPACES=all` is explicit and supported.

Effective concurrency does not exceed `runtime.worker_pools.*.concurrency`.

## 9. How itg_documents Was Protected

- No command targeted `itg_documents`.
- No smoke/test used `itg_documents`.
- Workers are disabled by default.
- Worker scope is explicit.
- Schema migration is additive.
- Old deleted docs were not restored.
- No reset/delete/archive/import operation was run against production workspaces.

## 10. Whether Workers Are Disabled By Default

Yes. `beehive-server` logs disabled worker manager state unless `BEEHIVE_WORKERS_ENABLED=1` is set. Even then, workers do not start without `BEEHIVE_WORKER_WORKSPACES`.

## 11. Tests Run and Exact Results

- `cargo fmt --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo test --manifest-path src-tauri/Cargo.toml`: passed, `196 passed; 0 failed; 3 ignored`.
- `npm run build`: passed.
- `VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build`: passed.
- `python3 scripts/lint_n8n_workflows.py docs/n8n_workflows`: passed.
- `rg "@tauri-apps/api/core|invoke\(" src -n`: passed; only `src/lib/apiClient/tauriClient.ts` imports `invoke`.
- `git diff --check`: passed.

## 12. Smoke Results

Used Rust worker smoke harness instead of JS smoke script.

Command:

```bash
cargo test --manifest-path src-tauri/Cargo.toml worker_
```

Result:

```text
9 passed; 0 failed; 0 ignored; 190 filtered out
```

Covered:

- worker pool config tests;
- worker HTTP route parsing;
- default/local_llm claim separation;
- `concurrency=0` represented by zero claim limit;
- double claim protection;
- manual claim active-lease refusal;
- heartbeat extension and wrong-worker rejection;
- expired lease recovery;
- finished-run recovery skip.

## 13. Known Risks

- Worker manager runs detached threads and relies on process shutdown; B13 should add richer operational control.
- Effective env concurrency is capped by YAML config; operators must set `runtime.worker_pools` intentionally.
- Recovery intentionally skips active leases whose `stage_run` is already finished, leaving that anomalous lease active for manual/diagnostic handling.
- Full queue UI is minimal in B12 and should be expanded in B13.

## 14. What Should Be Done in B13

- Dedicated Worker/Queue page.
- Pause/resume workers.
- Manual release/retry stuck lease actions.
- Queue metrics by `resource_class`.
- Backpressure display.
- Controlled pilot against a small explicit subset before any larger `itg_documents` run.

## 15. Checkpoints

ТЗ перечитано на этапах: after_plan, after_current_runtime_review, after_lease_schema_design, after_claim_logic_design, after_worker_manager_design, after_backend_changes, after_tests, after_smoke, before_feedback
