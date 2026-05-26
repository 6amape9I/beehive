# B15 Worker Bootstrap Lock Fix Feedback

## Root Cause

The worker loop loaded runtime context through `runtime::load_workspace_context` on every iteration.
That heavy path called `database::bootstrap_database`, and bootstrap called `sync_stages`, which upserts stage rows.
When SQLite was already busy, workers could fail on stage upsert before reaching `worker_leases` claim and before calling n8n.

## Changes

- Added `runtime::load_worker_runtime_context` as the lightweight worker path.
- Kept `runtime::load_workspace_context` as the heavy UI/admin/bootstrap path.
- Split shared registry/workdir/pipeline parsing into `load_workspace_context_parts`.
- Added `database::verify_worker_runtime_database`, returning `workspace_not_bootstrapped_for_workers` if the DB file or schema is not ready.
- Wrapped legitimate `sync_stages` bootstrap writes in the existing SQLite busy retry helper.
- Changed worker summary/control endpoints to use lightweight context where safe.
- Kept one heavy bootstrap in `start_workspace_workers` before spawning worker loops.
- Changed `worker_loop` to use `run_worker_loop_once`, which loads lightweight context and then claims work.
- Added worker lifecycle logs: `worker_loop_started`, `worker_context_loaded`, `worker_claim_idle`, `worker_context_error`, `worker_claimed_task`, `worker_task_started`, `worker_task_finished`.
- Throttled idle/context/error worker-loop logs so a stuck idle or context-error loop does not print every second forever.
- Added focused runtime/worker tests for missing bootstrap, no stage sync from lightweight context, read-mostly summary/start, load under open write transaction, and claim-to-webhook worker smoke.

## Bootstrap Status

`worker_loop` does not call `bootstrap_database`.

Normal loop iterations now call:

```text
worker_loop
  -> run_worker_loop_once
      -> runtime::load_worker_runtime_context
          -> database::verify_worker_runtime_database
      -> executor::run_worker_task
          -> claim_worker_runtime_tasks
          -> n8n/mock webhook
```

`start_workspace_workers` still calls `runtime::load_workspace_context` once before loops are spawned, so server startup can perform the required bootstrap/sync without repeating it on every idle/claim pass.

## Worker Summary And Control

Worker summary/control endpoints are read-mostly for stage rows after B15.
They parse current pipeline config and verify the DB/schema, but they do not sync stage rows through `bootstrap_database`.

Covered endpoints/functions:

- `worker_summary`
- `recover_expired_leases`
- `start_workers`
- `stop_workers`
- `update_pool_desired_concurrency`
- `pause_all` / `resume_all`
- `pause_pool` / `resume_pool`
- `release_lease`

## Test Results

Passed:

```text
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
git diff --check
```

Full Rust result:

```text
217 passed; 0 failed; 3 ignored
```

## Smoke Results

Smoke was run as a small automated test workspace with a local mock webhook.
It did not use or mutate `itg_documents`.

Command:

```text
cargo test --manifest-path src-tauri/Cargo.toml services::workers::tests::worker_loop_once_reaches_claim_and_calls_webhook_without_bootstrap_loop -- --nocapture
```

Observed logs:

```text
worker_claimed_task
worker_task_started
worker_task_finished outcome=succeeded
```

The test also asserts:

- `summary.claimed == 1`
- `summary.succeeded == 1`
- mock webhook request count is `1`

No `Failed to upsert stage 'stage_0': database is locked` appeared in this smoke.

## Remaining SQLite Lock Risks

- Legitimate UI/admin bootstrap paths can still write stage rows, so they can still wait on SQLite if another writer is active.
- Those remaining stage sync writes now go through the busy retry helper, but the main mitigation is that background worker loops no longer perform that write path repeatedly.
- Worker lightweight context requires a bootstrapped current-schema DB. If the DB is missing or stale, workers now fail clearly with `workspace_not_bootstrapped_for_workers` instead of trying to repair schema/stages in the loop.

## B16 Notes

- Add UI/API surfacing for `workspace_not_bootstrapped_for_workers` with an explicit "open/bootstrap workspace first" action.
- Consider a dedicated server/API smoke that starts `beehive-server` against a temp workspace if deeper end-to-end worker startup evidence is needed.
- Keep `itg_documents` smoke bounded and observational only unless a separate instruction explicitly authorizes a production-sized run.

ТЗ перечитано на этапах: after_plan, after_root_cause_review, after_context_split_design, after_worker_loop_changes, after_tests, after_smoke, before_feedback
