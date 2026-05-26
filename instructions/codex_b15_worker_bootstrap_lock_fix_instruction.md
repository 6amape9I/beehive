# B15. Worker Bootstrap Lock Fix and Runtime Context Split

## 0. Context

After B14, workers can be started from UI and the server can run worker loops. In real testing on `itg_documents`, workers started, UI showed running, but n8n received zero requests.

Server logs show:

```text
worker_error
Failed to upsert stage 'stage_0': database is locked

This means workers fail before claiming tasks and before calling n8n.

Current cause:

worker_loop
  -> runtime::load_workspace_context
      -> database::bootstrap_database
          -> sync_stages / upsert stages
              -> database is locked

Workers must not run full workspace bootstrap/sync on every loop iteration.

1. Goal

Fix worker runtime so background workers can claim tasks without repeatedly bootstrapping/syncing the workspace database.

After B15:

worker loop loads lightweight runtime context
worker claim reaches worker_leases
n8n webhook is called for claimed tasks
database is locked from stage upsert no longer blocks workers
2. Do not change

Do not implement:

RabbitMQ
Kafka
Postgres
new scheduler architecture
large itg_documents run
force-kill n8n executions
production workflow changes

Do not delete or reset itg_documents.

3. Required plan

Before coding create:

docs/beehive_s3_b15_worker_bootstrap_lock_plan.md

Plan must explain:

1. Where worker loop currently calls load_workspace_context.
2. Where load_workspace_context calls bootstrap_database.
3. Why bootstrap_database writes stages and can lock SQLite.
4. Which new lightweight context function will be added.
5. How workers will ensure DB schema exists without syncing stages every loop.
6. What tests will prove workers reach claim without stage upsert.
4. Split runtime context loading

Current runtime::load_workspace_context is too heavy for worker loops.

Add two functions:

load_workspace_context(...)
load_workspace_runtime_context_light(...)

or equivalent names.

4.1 Existing heavy function

Keep current behavior for UI/admin paths that need bootstrap:

open workspace
create/update/delete stage
workspace explorer if necessary
manual bootstrap/reconcile paths

It may still call:

database::bootstrap_database(...)
4.2 New lightweight worker context

New worker context must:

- read workspace registry;
- resolve workdir_path;
- verify pipeline.yaml exists;
- parse pipeline.yaml;
- return workdir_path, database_path, config;
- ensure DB file exists/schema exists if absolutely required;
- DO NOT call sync_stages on every worker loop;
- DO NOT upsert stages on every worker loop.

If DB/schema does not exist, return a clear error:

workspace_not_bootstrapped_for_workers

or call a schema-only initializer that does not sync stages.

Preferred approach:

runtime::load_worker_runtime_context(workspace_id)

This should be used by:

services::workers::worker_loop
services::workers::worker_summary, if safe
services::workers::start_workers, if safe

But be careful: summary may still need bootstrap in some UI flows. Do not break existing UI. The critical path is worker loop.

5. Bootstrap once before starting worker loops

When beehive-server starts workers for a workspace, it may do one safe bootstrap before spawning loops:

start_workspace_workers
  -> load heavy context once
  -> bootstrap/sync once
  -> spawn loops

After loops are spawned, each loop must use lightweight context.

UI Start workers should not repeatedly bootstrap on every summary refresh.

6. Avoid repeated stage sync in summary

Check whether these endpoints call heavy load_workspace_context repeatedly:

GET /api/workspaces/{id}/workers/summary
GET /api/workspaces/{id}/entities
GET /api/workspaces/{id}/workspace-explorer

Do not make B15 a full UI optimization, but avoid unnecessary write bootstrap in worker summary and worker control endpoints if possible.

Worker summary should be read-mostly.

7. SQLite lock handling

B14 already added WAL and busy timeout. B15 should add more targeted handling:

7.1 Increase default busy timeout if needed

Keep:

BEEHIVE_SQLITE_BUSY_TIMEOUT_MS

Default may remain 30000 or increase to 60000 if justified.

7.2 Retry bootstrap writes

For the remaining legitimate bootstrap/sync paths, wrap stage upsert/sync transactions with existing busy retry helper or add one.

But do not rely only on retry. The main fix is: workers must not bootstrap every loop.

8. Worker loop logging

Add better logs around worker lifecycle.

At minimum:

worker_loop_started
worker_context_loaded
worker_claim_idle
worker_claimed_task
worker_context_error
worker_task_started
worker_task_finished

Do not spam logs every second forever. Log idle at low frequency or only when useful.

Current logs only show worker_error, which made diagnosis harder.

9. Tests

Add tests for:

1. worker lightweight context does not call bootstrap/sync stages.
2. worker loop claim path can operate when DB is already bootstrapped.
3. worker summary/control endpoints do not write stage rows unnecessarily.
4. schema/bootstrap still happens for create/update stage and explicit workspace open.
5. simulated locked DB during stage sync does not prevent worker lightweight context from loading.
6. worker_loop with lightweight context can reach claim_worker_runtime_tasks.

If direct “does not call bootstrap” is hard to test, test observable behavior:

- record stage updated_at before worker context load;
- load worker context several times;
- assert stage rows unchanged.
10. Manual verification commands

Run:

cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
git diff --check
11. Smoke test

Add a small smoke or manual report:

1. Start server with workers enabled for a small test workspace.
2. Start workers from UI/API.
3. Confirm logs show worker_claimed_task or task_started.
4. Confirm n8n/mock webhook receives at least one request.
5. Confirm no worker_error "Failed to upsert stage ... database is locked".

Do not run full itg_documents.

If using itg_documents, use only a very small bounded manual observation and do not reset/delete/import.

12. Feedback

Create:

docs/beehive_s3_b15_worker_bootstrap_lock_feedback.md

Feedback must include:

- exact root cause found;
- changed functions;
- whether worker loop still calls bootstrap_database;
- whether worker summary/control endpoints are read-mostly;
- test results;
- smoke results;
- remaining SQLite lock risks;
- what to do in B16.

Required checkpoint line:

ТЗ перечитано на этапах: after_plan, after_root_cause_review, after_context_split_design, after_worker_loop_changes, after_tests, after_smoke, before_feedback
13. Acceptance criteria

B15 is accepted only if:

1. Worker loop no longer calls full bootstrap/sync stages every iteration.
2. Worker loop can claim tasks without triggering stage upsert.
3. "Failed to upsert stage 'stage_0': database is locked" no longer appears during normal worker idle/claim loop.
4. At least one test or smoke proves worker reaches claim/task start.
5. Existing create/update/delete stage still syncs DB correctly.
6. Existing UI workspace operations still work.
7. No destructive action is run on itg_documents.