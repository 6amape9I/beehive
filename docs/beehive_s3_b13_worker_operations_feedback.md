# B13 Worker Operations Hardening Feedback

## 1. What Changed

- Added B13 plan, runbook, pilot report, and updated worker lease contract docs.
- Added schema v11 with `worker_pool_controls`.
- Added worker pause/resume state for all pools and individual `default` / `local_llm` pools.
- Added safe manual worker lease release.
- Extended worker summary with per-resource queue counts, pause state, last error, oldest pending age, and average duration.
- Disabled broad workspace run actions when workers are enabled for that workspace.
- Expanded the workspace UI into an explicit `Workers & Queue` panel with pool controls, warnings, queue metrics, recent leases, recovery, and release action.
- Added SQLite `busy_timeout` and WAL pragmas for writable workspace connections.

## 2. Files Changed

Backend:

- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/database/mod.rs`
- `src-tauri/src/domain/mod.rs`
- `src-tauri/src/http_api/mod.rs`
- `src-tauri/src/services/runtime.rs`
- `src-tauri/src/services/workers.rs`
- `src-tauri/src/services/workspaces.rs`

Frontend:

- `src/app/styles.css`
- `src/components/dashboard/DashboardActions.tsx`
- `src/lib/apiClient/httpClient.ts`
- `src/lib/apiClient/tauriClient.ts`
- `src/lib/apiClient/types.ts`
- `src/lib/runtimeApi.ts`
- `src/pages/DashboardPage.tsx`
- `src/pages/WorkspaceExplorerPage.tsx`
- `src/types/domain.ts`

Docs:

- `docs/beehive_s3_b13_worker_operations_plan.md`
- `docs/beehive_s3_b13_worker_operations_feedback.md`
- `docs/beehive_s3_b13_worker_pilot_report.md`
- `docs/worker_leases_runtime_contract.md`
- `docs/worker_operations_runbook.md`

## 3. Broad-Run Bypass Prevention

B13 uses the safer Variant B.

When `BEEHIVE_WORKERS_ENABLED=1` and `BEEHIVE_WORKER_WORKSPACES` includes the workspace id or `all`, workspace broad runs return:

```text
workers_enabled_broad_run_disabled
Workers are enabled for this workspace. Use selected run or worker pools instead.
```

This is enforced for workspace-id service paths used by HTTP and Tauri-by-id commands:

- `run_small_batch`
- `run_pipeline_waves`

Registered-workspace path-based Tauri broad commands also resolve their workdir back to a workspace id and return the same error when that workspace is in the worker scope.

The Dashboard disables broad `Run due tasks` when worker summary says broad runs are disabled. Workspace Explorer keeps selected pipeline waves visible for targeted operator debugging.

## 4. Pause/Resume

Pause state is stored per workspace DB in `worker_pool_controls`.

Endpoints:

- `POST /api/workspaces/{workspace_id}/workers/pause`
- `POST /api/workspaces/{workspace_id}/workers/resume`
- `POST /api/workspaces/{workspace_id}/workers/pools/{resource_class}/pause`
- `POST /api/workspaces/{workspace_id}/workers/pools/{resource_class}/resume`

Worker loop and worker claim both check pause state before claiming new work. Running leases are not interrupted.

## 5. Manual Lease Release

Endpoint:

```text
POST /api/workspaces/{workspace_id}/workers/leases/{lease_id}/release
```

B13 release rules:

- active lease with finished attached `stage_run`: allowed;
- fresh active unfinished lease: rejected;
- force release is not exposed;
- release only changes `worker_leases.status` to `released` and records `release_reason`;
- stage state is not marked successful or failed by release.

Expired unfinished work should use `recover-expired-leases`.

## 6. Worker Summary Counts

Summary counts are computed per `resource_class` with joins through `entity_stage_states`, `stages`, `entity_files`, and `entities`.

Counts respect:

- `stage.resource_class`
- `stage.is_active = 1`
- `entity.is_archived = 0`
- `state.file_exists = 1`
- `entity_files.file_exists = 1`

Reported per pool:

- pending
- retry_wait due
- retry_wait not due
- queued
- in_progress
- blocked
- failed
- active leases
- expired leases
- pause state
- oldest pending age
- average duration
- last error

## 7. SQLite Hardening

Writable workspace connections now apply:

```sql
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
```

Readonly connections apply:

```sql
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
```

## 8. WAL Decision

WAL is enabled for writable workspace DB connections.

Reason: B13 introduces multiple worker threads writing to the same workspace DB, and WAL with `synchronous = NORMAL` is the preferred local SQLite mode for reducing writer/reader contention in this deployment. The test suite confirms `journal_mode = wal` and `busy_timeout = 5000` on opened workspace DB connections.

## 9. Tests Run

- `cargo fmt --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo test --manifest-path src-tauri/Cargo.toml worker_ -- --nocapture`: passed, 13 passed; 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml`: passed, 203 passed; 0 failed; 3 ignored.
- `npm run build`: passed.
- `VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build`: passed.
- `python3 scripts/lint_n8n_workflows.py docs/n8n_workflows`: passed.
- `rg "@tauri-apps/api/core|invoke\(" src -n`: passed; only `src/lib/apiClient/tauriClient.ts` imports `invoke`.
- `git diff --check`: passed.
- `git diff --check --cached`: passed.

## 10. Pilot Result

Real pilot status: blocked.

Reason: no explicitly prepared safe test workspace with 20-100 files and known safe mock/real n8n endpoint was provided in this turn. B13 does not support subset-limited worker execution for `itg_documents`, so it was not used.

Pilot report:

- `docs/beehive_s3_b13_worker_pilot_report.md`

No real S3/n8n calls were made.

## 11. itg_documents

`itg_documents` was not modified.

No worker process, destructive smoke, reset, archive, cleanup, import, or broad execution was run against `itg_documents`.

## 12. Remaining Risks

- Path-based legacy Tauri broad commands still exist for unregistered/local workdir mode. Workers only start for registered workspaces, so this path is not expected to bypass a running worker pool.
- Manual release intentionally avoids force release, so some pathological active leases still require diagnosis or expired lease recovery.
- Retry from failed/blocked is available through existing entity/stage manual actions, not a bulk queue action.
- Real pilot remains pending until a safe test workspace and endpoint are prepared.

## 13. B14 Recommendations

- Add a dedicated `/workspaces/{workspace_id}/workers` route with deeper filtering and run-output shortcuts.
- Add a safe pilot harness that creates a temporary workspace and mock webhook automatically.
- Add subset-limited worker execution for production pilot workflows before considering `itg_documents`.
- Add optional audited force-release with strong confirmation if operators need it.
- Add queue retry actions scoped to selected failed/blocked states with explicit attempt-reset semantics.

## 14. Checkpoints

ТЗ перечитано на этапах: after_plan, after_b12_review, after_bypass_design, after_pause_resume_design, after_lease_actions_design, after_queue_ui_design, after_sqlite_hardening, after_tests, after_pilot, before_feedback
