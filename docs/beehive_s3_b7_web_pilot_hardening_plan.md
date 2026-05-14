# B7 Web Pilot Hardening Plan

## 1. B6 Baseline Summary

B6 delivered the browser-first MVP:

- `beehive-server` with local default bind and static `dist/` serving.
- Workspace registry and workspace-id HTTP API flow.
- Browser workspace selector that does not use Tauri `openRegisteredWorkspace()` in HTTP mode.
- S3 stage creation, stage linking, broad `run-small-batch`, broad `run-pipeline-waves`, and stage-run outputs endpoint.
- Workspace Explorer can show S3 pointer rows and runtime counters.

## 2. Exact B7 Goals

B7 must make the web operator path safe enough for a real small pilot:

- select 1 to 10 S3 source artifact rows in Workspace Explorer;
- run only those selected roots and descendants;
- show selected run summary and one-to-many output tree;
- keep broad queue actions available but clearly distinguish them from the recommended selected action;
- add request body size limit, tighter CORS, and useful structured logs;
- attempt a real S3+n8n web pilot, or record a precise external blocker.

## 3. B6 Code Reused Unchanged

Reuse these foundations instead of rewriting them:

- workspace registry and `services::runtime::workspace_context`;
- existing executor state transitions and `run_entity_stage`;
- S3 control envelope and manifest validation;
- S3 pointer registration and `producer_run_id` lineage;
- stage-run outputs read model;
- B6 HTTP router/server shape;
- B6 React API client boundary.

## 4. Approved / Selected Batch Design

Use the acceptable MVP design from the instruction:

1. Validate selected `entity_file_id` roots in the workspace database.
2. Clamp root count to `1..10`, `max_waves` to `1..10`, and `max_tasks_per_wave` to `1..5`.
3. Execute exact `(entity_id, stage_id)` pairs for selected root files using the existing executor helper.
4. After each wave, collect child files whose `producer_run_id` matches run IDs produced by the previous wave.
5. Execute only those child files in the next wave.
6. Stop on idle, max waves, failure/block if requested, or runtime error.

This avoids broad `run_due_tasks` and never claims unrelated pending roots.

## 5. How Selected Roots Avoid Unrelated Pending Artifacts

The selected runner will never call broad due-queue execution. It will maintain a frontier of concrete `entity_file_id` values. Each wave resolves those files back to concrete `(entity_id, stage_id)` pairs and runs only those pairs. The next frontier is built only from `entity_files.producer_run_id` values that match selected stage run IDs.

## 6. UI Changes

Workspace Explorer will add:

- checkbox per eligible S3 row;
- selected count;
- Clear selection;
- selected-run controls for `max_waves`, `max_tasks_per_wave`, `stop_on_first_failure`;
- `Run selected pipeline waves` as the recommended pilot action;
- a result panel with roots, run IDs, totals, and output tree;
- clearer wording that `Run small batch` and `Run pipeline waves` are broad queue actions.

Eligibility will be conservative for B7: S3 rows with `file_exists = true` and `runtime_status` of `pending` or `retry_wait`.

## 7. HTTP API Changes

Add:

```text
POST /api/workspaces/{workspace_id}/run-selected-pipeline-waves
```

Add matching Tauri-compatible command/service and frontend method:

```text
runSelectedPipelineWavesById(workspaceId, rootEntityFileIds, maxWaves, maxTasksPerWave, stopOnFirstFailure)
```

## 8. One-To-Many Lineage Behavior

The selected response will include an `output_tree` that preserves every child artifact per producing run. The UI will not collapse one input into one child row. Workspace Explorer will also make B6 stage-run output expansion easier to access for rows with `producer_run_id`.

## 9. Real S3+n8n Pilot Plan

Attempt through `beehive-server` and HTTP/browser path:

1. Build frontend in HTTP mode.
2. Start `beehive-server`.
3. Verify health/workspaces/explorer.
4. Reconcile S3 or register 1 to 3 known S3 source artifacts through HTTP.
5. Run selected pipeline waves against those `entity_file_id` values.
6. Check stage runs, child outputs, S3 output keys, and final states.

If network/S3/n8n is blocked, record the exact blocker in the B7 report.

## 10. n8n Live Workflow Preflight Plan

Create `docs/n8n_live_web_pilot_preflight_b7.md`. Check available repo workflow examples and any pilot workflow references without committing production workflows. The checklist will cover POST webhook, response node, JSON body envelope, no header source-key path, no production Search/List bucket source selection, no `/main_dir/pocessed` typo, manifest schema, output fields, and save_path compatibility.

## 11. Server Hardening Plan

Add:

- `BEEHIVE_SERVER_MAX_BODY_BYTES`, default `1048576`, returning `413` before body read when exceeded;
- `BEEHIVE_ALLOWED_ORIGIN` with local defaults and no wildcard for non-local bind;
- request CORS response based on the incoming `Origin`;
- structured stdout/stderr logs for server start, request completed/failed, workspace actions, selected batch started/finished;
- no logging of secrets, authorization header, request body, business JSON, or source document body.

Keep B6 token model; do not add RBAC.

## 12. Test Plan

Rust:

- selected runner rejects missing/unrelated ids;
- selected runner runs only selected roots;
- selected runner follows two child outputs from one source;
- selected runner does not claim unrelated pending source;
- selected runner preserves block/failure summary behavior;
- HTTP route parses selected-run request;
- request body size limit rejects oversized bodies;
- non-local bind/token tests still pass;
- stage linking and stage-run outputs tests still pass.

Frontend/build:

- `npm run build`;
- `VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build`;
- direct Tauri import check.

Other:

- existing n8n workflow linter;
- `git diff --check`;
- server HTTP smoke and new `scripts/web_operator_smoke.mjs`.

## 13. Not Implemented In B7

- scheduler/worker pool/background daemon;
- async manifest polling;
- n8n REST workflow editor;
- production workflow storage in repo;
- Postgres/full RBAC/multi-user locking;
- large production run;
- full README rewrite.

## 14. Risks And Rollback

Risks:

- selected runner must preserve existing runtime state-machine safety;
- real n8n pilot may be blocked by external workflow/S3 credentials;
- CORS tightening must not break local dev.

Rollback:

- selected-run API is additive;
- broad B6 run endpoints remain unchanged;
- server hardening env defaults are local-dev compatible.

## 15. Checkpoints

Reread B7 instruction at:

```text
after_plan
after_selected_batch_design
after_backend_selected_runner
after_frontend_selected_batch_ui
after_server_hardening
after_n8n_preflight
after_real_web_pilot_attempt
after_tests
before_feedback
```
