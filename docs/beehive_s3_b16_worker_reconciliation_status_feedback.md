# B16 Worker Reconciliation And Status Feedback

## 1. Worker Cap Confusion Root Cause

The confusing `max/desired = 1` view was caused by three separate limits being collapsed into one visible value.
Effective worker capacity is the minimum of server env, `runtime.worker_pools.<pool>.concurrency`, and requested UI desired concurrency.
When YAML sets `runtime.worker_pools.default.concurrency: 1`, `BEEHIVE_WORKER_DEFAULT_CONCURRENCY=10` cannot raise the applied or effective value above 1.

B16 now exposes `env_concurrency_limit` and `requested_desired_concurrency` next to configured YAML, applied desired, effective concurrency, active leases, started, and paused state.
The UI renders requested/applied/YAML/env/effective fields and warns when requested desired workers are capped.

## 2. Idle Worker With In-Progress Task Root Cause

Worker claims only select `pending` and due `retry_wait` states.
If a task remains `in_progress` without a valid active lease, a live worker can correctly report idle because that row is no longer claimable.

B16 adds worker summary diagnostics for stale state/lease combinations and a deterministic reconcile action for stuck states.

## 3. Stale Leases Or States Found

No production `itg_documents` data was inspected, reset, imported, deleted, or broadly executed.
The stale states found during B16 were synthetic test fixtures:

- active lease with finished successful run;
- active lease with finished failed run;
- old `in_progress` state without active lease;
- `queued` state without active lease;
- internal worker execution error after run start.

All synthetic stale cases are repaired by tests, including idempotent second reconciliation.

## 4. Reconciliation Behavior

`POST /api/workspaces/{workspace_id}/workers/reconcile-stuck` calls the new DB reconciliation path and returns `{ reconciled, summary, errors }`.

The reconciler repairs:

- active lease with finished successful run -> state `done`, lease `done`;
- active lease with finished failed run -> state `retry_wait`, `failed`, or `blocked` according to error type and attempts, lease `failed`;
- old `in_progress` without active lease -> `retry_wait` or `failed`, with unfinished run closed;
- `queued` without active lease -> `pending`.

Existing expired active lease recovery remains unchanged.
`run_worker_task` also calls a best-effort `finish_worker_task_internal_error` finalizer when `execute_task` returns `Err` after a worker-owned task has started.
The finalizer finds the active lease by `lease_id` first and falls back to `state_id`, then closes any unfinished run with `error_type = worker_internal_error`, moves the state to retry/failed, and marks the lease failed.

## 5. Entity Detail Effective File Status

Entity Detail File Instances now displays `Runtime status`, derived from `detail.stage_states`:

1. match by `stage_state.file_instance_id == file.id`;
2. fallback by `entity_id + stage_id`;
3. fallback to `file.status`.

When the runtime status differs from the artifact record status, the UI shows the artifact status as secondary text.
The DB `entity_files.status` value is not mass-updated.

## 6. Commands Run

- `cargo fmt --manifest-path src-tauri/Cargo.toml`: passed, no output.
- `cargo test --manifest-path src-tauri/Cargo.toml`: passed, `224 passed; 0 failed; 3 ignored`; binary/doc test targets passed with 0 tests.
- `npm run build`: passed, `tsc && vite build`, 88 modules transformed.
- `VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build`: passed, `tsc && vite build`, 88 modules transformed.
- `python3 scripts/lint_n8n_workflows.py docs/n8n_workflows`: passed, no output.
- `git diff --check`: passed, no output.

Additional focused checks:

- `cargo test --manifest-path src-tauri/Cargo.toml worker_start_summary`: passed, 3 tests.
- `cargo test --manifest-path src-tauri/Cargo.toml reconcile_unleased_in_progress_and_queued_states_is_idempotent`: passed.
- `cargo test --manifest-path src-tauri/Cargo.toml worker_loop_once_reaches_claim_and_calls_webhook_without_bootstrap_loop -- --nocapture`: passed and logged `worker_claimed_task`, `worker_task_started`, `worker_task_finished outcome=succeeded`.

## 7. Smoke / Manual Results

Bounded automated smoke was used instead of production `itg_documents` execution.

- Mock worker smoke claimed one task from a temp workspace, called the mock webhook, and finished successfully.
- Reconciliation smoke verified that old unleased `in_progress` and unleased `queued` states are repaired and that the second reconcile run returns 0.
- Frontend compile smoke verified the new worker cap/anomaly UI and Entity Detail status derivation type-check.

No browser-driven visual smoke was run.
No full `itg_documents` batch was run.

## 8. Remaining Risks

- UI visibility was verified by TypeScript production build, not by Playwright/browser screenshots.
- `requested_desired_concurrency` is request-scoped for start/update responses; a later summary without a fresh request still shows applied/YAML/env/effective values but may not have the original requested value.
- Reconciliation does not force-kill real in-flight n8n executions; it only repairs DB state after leases/runs are stale or internally inconsistent.

## 9. B17 Recommendations

- Add a small browser smoke for Workers & Queue and Entity Detail status rendering.
- Add a safe operator-only observation script for a bounded `itg_documents` subset, with explicit dry-run/reconcile confirmation.
- Consider exposing a Runtime settings shortcut for editing `runtime.worker_pools.default/local_llm.concurrency`.
- Add a worker anomaly event history panel if operators need more than recent summary records.

ТЗ перечитано на этапах: after_plan, after_cap_ux_design, after_lease_reconciliation_design, after_entity_file_status_design, after_backend_changes, after_ui_changes, after_tests, after_smoke, before_feedback
