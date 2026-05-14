# B7. Real Web Operator Pilot, Approved Batch, and Server Hardening

## 0. Role

You are Codex agent working as Beehive Web Operator Engineer.

B6 created a runnable browser-first MVP:

```text
beehive-server
browser workspace selector
workspace_id API flow
S3 stage creation
stage linking
run-small-batch / run-pipeline-waves
stage-run output lineage endpoint
```

B7 must turn that MVP into a real operator pilot path. The goal is not to add more abstract infrastructure. The goal is to prove and harden the browser workflow that non-programmer operators will use.

Normal operators must not use CLI, Rust ignored tests, or curl. CLI/curl/tests are allowed only as agent verification tools.

## 1. Strategic goal

B7 goal:

```text
browser operator selects workspace
→ reviews/approves a small set of S3 source artifacts
→ runs selected artifacts through n8n/S3 pipeline waves
→ sees source statuses, child outputs, one-to-many lineage, errors/retries
```

This must work through `beehive-server` and the browser/HTTP flow created in B6.

B7 is accepted only if the web path is exercised. It is not enough to prove the same thing through ignored Rust tests.

## 2. Current baseline from B6

Use B6 as accepted baseline:

```text
cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
```

Working B6 endpoints already include:

```text
GET  /api/health
GET  /api/workspaces
GET  /api/workspaces/{workspace_id}
GET  /api/workspaces/{workspace_id}/workspace-explorer
POST /api/workspaces/{workspace_id}/reconcile-s3
POST /api/workspaces/{workspace_id}/register-s3-source
POST /api/workspaces/{workspace_id}/run-small-batch
POST /api/workspaces/{workspace_id}/run-pipeline-waves
POST /api/workspaces/{workspace_id}/stages
POST /api/workspaces/{workspace_id}/stages/{stage_id}/next-stage
GET  /api/workspaces/{workspace_id}/stage-runs/{run_id}/outputs
```

Do not rewrite these foundations. B7 must build on them.

## 3. Main B7 problems to solve

B6 intentionally did not verify:

```text
real S3+n8n execution through the web path
real child outputs through HTTP/UI
safe selected/approved batch execution
browser-visible one-to-many output expansion with real outputs
minimal server hardening for pilot use
```

B7 must address these gaps.

## 4. Non-goals

B7 must not implement:

```text
high-load scheduler
background daemon
worker pool
async manifest polling
n8n REST workflow editor
production workflow storage in repo
Postgres migration
full RBAC
large 22k-file production run
full README rewrite
```

Do not add heavy frontend or backend frameworks unless absolutely necessary. If a small dependency is needed, justify it in the plan before adding it.

## 5. Required plan before code

Before code, create:

```text
docs/beehive_s3_b7_web_pilot_hardening_plan.md
```

The plan must include:

```text
1. B6 baseline summary.
2. Exact B7 goals.
3. Which B6 code will be reused unchanged.
4. Approved/selected batch design.
5. How selected roots will be executed without grabbing unrelated pending artifacts.
6. UI changes.
7. HTTP API changes.
8. One-to-many lineage behavior.
9. Real S3+n8n pilot plan.
10. n8n live workflow preflight plan.
11. Server hardening plan.
12. Test plan.
13. What will not be implemented.
14. Risks and rollback.
15. Checkpoints.
```

Do not write code before creating this plan.

## 6. Instruction reread checkpoints

Reread this instruction at minimum:

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

The final feedback must include exactly this line, with any additional checkpoints if needed:

```text
ТЗ перечитано на этапах: after_plan, after_selected_batch_design, after_backend_selected_runner, after_frontend_selected_batch_ui, after_server_hardening, after_n8n_preflight, after_real_web_pilot_attempt, after_tests, before_feedback
```

## 7. Approved / selected batch execution

### 7.1 Why this is needed

B4/B6 `run_pipeline_waves` can run any pending tasks. This is unsafe for operator pilot work when a workspace contains many pending source artifacts.

B7 must add an operator-controlled selected batch path.

The operator must be able to choose a small set of source artifacts and run only that set and its descendants.

### 7.2 Required backend API

Add an HTTP-shaped endpoint and Tauri-compatible command/service:

```text
POST /api/workspaces/{workspace_id}/run-selected-pipeline-waves
```

Suggested request:

```json
{
  "root_entity_file_ids": [101, 102, 103],
  "max_waves": 5,
  "max_tasks_per_wave": 3,
  "stop_on_first_failure": true
}
```

Suggested response:

```json
{
  "summary": {
    "root_entity_file_ids": [101, 102, 103],
    "waves_executed": 2,
    "stopped_reason": "idle|max_waves_reached|failure_or_blocked|runtime_error",
    "total_claimed": 0,
    "total_succeeded": 0,
    "total_failed": 0,
    "total_blocked": 0,
    "total_retry_scheduled": 0,
    "root_results": [],
    "wave_summaries": [],
    "output_tree": []
  },
  "errors": []
}
```

Add the corresponding frontend API method:

```text
runSelectedPipelineWavesById(workspaceId, rootEntityFileIds, maxWaves, maxTasksPerWave, stopOnFirstFailure)
```

### 7.3 Execution invariant

Selected batch must not claim unrelated pending source artifacts.

The implementation may use one of these safe designs:

#### Preferred design: exact-root then descendant-scoped waves

Wave 1 runs only the exact selected root files/states.

Subsequent waves run only descendants produced by previous selected runs.

Track descendants using:

```text
entity_files.producer_run_id
source_file_id
stage_run.run_id
entity_file_id
stage_id
entity_id
```

Do not call the broad `run_due_tasks` for later waves unless it can be safely scoped to the selected descendants.

#### Acceptable MVP design

If exact descendant-scoped execution would be too risky to implement in one stage, create a service-level scoped runner that:

```text
1. validates selected entity_file_ids;
2. executes exact selected entity/stage pairs;
3. collects created child artifacts;
4. executes exact child entity/stage pairs in the next wave;
5. never falls back to global pending queue.
```

If existing executor APIs are insufficient, add narrowly-scoped executor helpers. Do not weaken runtime safety or bypass normal state transitions.

### 7.4 Safety rules

The selected runner must:

```text
clamp root count to 1..10 for B7;
clamp max_waves to 1..10;
clamp max_tasks_per_wave to 1..5;
reject missing entity_file_ids;
reject entity_file_ids outside the workspace DB;
reject non-S3 roots for S3 web pilot;
reject roots without runtime state;
reject already done roots unless explicitly reset by existing manual action;
preserve retry/block/failure behavior;
record stage_runs normally;
preserve stage_run.request_json/response_json audit;
never send business JSON to n8n;
never use source-key headers.
```

## 8. UI requirements

### 8.1 Workspace Explorer selection

In browser workspace route:

```text
/workspaces/{workspace_id}/workspace
```

Add checkbox selection for S3 source artifact rows.

Minimum UI:

```text
checkbox per eligible S3 row
selected count
Clear selection
Run selected pipeline waves
max_waves
max_tasks_per_wave
stop_on_first_failure
result summary
```

Eligible rows:

```text
storage_provider = s3
runtime_status in pending/retry_wait/failed/blocked if existing manual reset/approval flow allows it
file_exists = true
```

For B7, keep it conservative: default selectable should be `pending` and `retry_wait`.

### 8.2 Operator messaging

The UI must make it clear that:

```text
Run small batch = can run due queue broadly
Run pipeline waves = broad due queue waves
Run selected pipeline waves = only selected roots and descendants
```

For pilot work, the recommended action must be `Run selected pipeline waves`.

### 8.3 Output tree / lineage display

After a selected run, show:

```text
root artifact
source stage
run_id(s)
output_count
child artifacts
target stage
child runtime status
relation_to_source
s3_uri
error/retry status where applicable
```

If one input produces multiple outputs, do not collapse them into one row. Show all children.

### 8.4 Stage-run output expansion

Keep the B5/B6 endpoint:

```text
GET /api/workspaces/{workspace_id}/stage-runs/{run_id}/outputs
```

In B7, make it easy to use from the Workspace Explorer, not only Entity Detail.

For any S3 row with `producer_run_id`, the operator should be able to load sibling outputs for that run.

## 9. Real S3+n8n web pilot

### 9.1 Minimum pilot

Attempt a real web/API pilot:

```text
1. Start beehive-server.
2. Build frontend with VITE_BEEHIVE_API_BASE_URL.
3. Open/list workspace via HTTP.
4. Reconcile S3 or manually register known source artifacts through HTTP/UI.
5. Select 1 to 3 source artifacts.
6. Run selected pipeline waves through HTTP/UI.
7. Confirm n8n was called.
8. Confirm output objects exist in S3.
9. Confirm source states changed.
10. Confirm child outputs appear in Workspace Explorer and stage-run outputs endpoint.
```

A browser-click manual QA is preferred. If browser automation is unavailable, a scripted HTTP smoke is acceptable, but feedback must clearly state whether the actual browser UI was manually opened.

### 9.2 Required report

Create:

```text
docs/beehive_s3_b7_real_web_pilot_report.md
```

Include:

```text
workspace_id
server URL
n8n webhook URLs used
source artifact keys
selected entity_file_ids
run_ids
output artifact keys
source final states
child final states
stage-run outputs endpoint result
whether S3 outputs exist
whether real n8n was contacted
whether browser UI was manually inspected
screenshots omitted/available note
blockers if any
```

Do not include secrets.

### 9.3 If real pilot is blocked

A real pilot blocker is acceptable only if precise and external.

Valid blockers:

```text
n8n workflow not imported/active
n8n S3 credentials missing
S3 credentials/network unavailable
source objects absent
webhook returns invalid manifest
live workflow uses old headers/Search bucket/typo route
```

Invalid blockers:

```text
no source object
no pipeline
no web endpoint
no selected batch UI
```

The B7 code must still implement selected batch and web UI even if the real n8n pilot is externally blocked.

## 10. n8n live workflow preflight

B7 must create:

```text
docs/n8n_live_web_pilot_preflight_b7.md
```

This is a practical checklist/report, not a heavy n8n validator.

Check any workflow JSON available in the repo or supplied by the operator for the pilot.

Check at least:

```text
Webhook method POST
responseMode responseNode
workflow reads JSON body control envelope
source_bucket/source_key from body, not headers
no X-Beehive-Source-Key
no references to old node names such as "Read Beehive headers"
no Search/List Bucket as production source selection
no /main_dir/pocessed typo
manifest outputs have artifact_id/entity_id/relation_to_source/bucket/key/save_path
save_path matches stage aliases
```

Important known risks to mention in the report:

```text
- Some smoke workflow variants still have a Code node reading $('Read Beehive headers') while the graph already uses body/Edit Fields.
- Some semantic workflow variants still contain Manual Trigger → Search bucket → Download file demo branch.
- Some older workflow variants still use /main_dir/pocessed typo.
```

Do not commit production n8n workflows. If a corrected example is useful, add only a tiny contract example or runbook note.

## 11. Server hardening for B7

B6 server is MVP. B7 must add minimal hardening, without making it enterprise-grade.

### 11.1 Request body size limit

Add env:

```text
BEEHIVE_SERVER_MAX_BODY_BYTES
```

Default:

```text
1048576
```

Reject bigger request bodies with `413 Payload Too Large`.

### 11.2 CORS tightening

Current MVP behavior may be permissive. B7 should add:

```text
BEEHIVE_ALLOWED_ORIGIN
```

Default local dev allowed origins can include:

```text
http://127.0.0.1:8787
http://localhost:8787
http://127.0.0.1:5173
http://localhost:5173
```

If non-local bind is enabled, do not use `Access-Control-Allow-Origin: *`.

### 11.3 Structured logs

Add minimal structured logs for:

```text
server_start
request_completed
request_failed
workspace_action
selected_batch_started
selected_batch_finished
```

Do not log:

```text
S3_KEY
S3_SEC_KEY
BEEHIVE_OPERATOR_TOKEN
Authorization header
business JSON payload
full source document body
```

Logging may be stdout JSON lines or compact text, but it must be useful for operator/admin troubleshooting.

### 11.4 Token behavior

Keep B6 token model. Do not implement full RBAC in B7.

If token is configured, HTTP client must support sending token via env/build/runtime setting.

At minimum, document how to call API with token and how to configure frontend dev mode.

## 12. HTTP API error semantics

Do not do a risky full API semantics rewrite.

But B7 should make selected-batch and hardening endpoints return clear HTTP statuses for route/request-level errors:

```text
400 invalid request
401 unauthorized
404 route not found
413 body too large
500 internal server error
```

Operation-level Beehive errors may remain in `{ errors: [...] }` result envelopes for compatibility.

## 13. Frontend smoke automation

Add a lightweight smoke helper, for example:

```text
scripts/web_operator_smoke.mjs
```

It should call HTTP API only, not require browser automation.

Minimum checks:

```text
GET /api/health
GET /api/workspaces
GET /api/workspaces/{workspace_id}/workspace-explorer
POST /api/workspaces/{workspace_id}/run-selected-pipeline-waves with dry/safe invalid input to verify validation
```

If a real pilot env flag is set, it may run the actual selected pilot.

Do not add Playwright/Cypress unless explicitly justified.

## 14. Tests

### 14.1 Rust tests

Add/update tests for:

```text
selected runner rejects unrelated/missing entity_file_ids
selected runner executes only selected roots
selected runner follows two child outputs from one source
selected runner does not claim unrelated pending source
selected runner preserves retry/block behavior
HTTP route parses run-selected-pipeline-waves
request body size limit rejects oversized body
non-local bind/token rules still pass
stage linking still works
multi-output stage-run outputs still work
```

### 14.2 Frontend/build

Run:

```bash
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
```

Ensure direct Tauri imports remain isolated:

```bash
rg "@tauri-apps/api/core|invoke\(" src -n
```

Expected result:

```text
only src/lib/apiClient/tauriClient.ts
```

### 14.3 n8n lint

Run existing linter:

```bash
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
```

### 14.4 Server smoke

Run:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
```

Then:

```bash
curl -sS http://127.0.0.1:8787/api/health
curl -sS http://127.0.0.1:8787/api/workspaces
```

And run the new web smoke helper if added.

## 15. Commands to run

At minimum:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
rg "@tauri-apps/api/core|invoke\(" src -n
git diff --check
```

Server smoke:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
```

If network/S3/n8n are available, run a real selected web pilot and record it in the B7 report.

## 16. Required documentation

Create:

```text
docs/beehive_s3_b7_web_pilot_hardening_plan.md
docs/beehive_s3_b7_web_pilot_hardening_feedback.md
docs/beehive_s3_b7_real_web_pilot_report.md
docs/n8n_live_web_pilot_preflight_b7.md
```

Update if needed:

```text
docs/web_operator_mvp_runbook.md
docs/front_back_split.md
docs/multi_output_lineage.md
docs/stage_creation_s3_ui_contract.md
```

Do not rewrite the entire README.

## 17. Feedback requirements

Create:

```text
docs/beehive_s3_b7_web_pilot_hardening_feedback.md
```

It must include:

```text
1. What was implemented.
2. What B6/B5/B4 pieces were preserved.
3. Backend/API changes.
4. Frontend/UI changes.
5. Selected batch behavior and safety rules.
6. One-to-many lineage behavior.
7. Real web pilot status.
8. Whether browser UI was manually inspected.
9. Whether real S3 was contacted.
10. Whether real n8n was contacted.
11. Source artifact keys used.
12. Selected entity_file_ids.
13. Run IDs.
14. Output artifact keys.
15. Final source/child states.
16. Server hardening changes.
17. n8n preflight findings.
18. Commands run and exact results.
19. Tests passed/failed/ignored.
20. What could not be verified.
21. Remaining risks.
22. What should be done in B8.
23. Reread checkpoints.
```

## 18. Acceptance criteria

B7 is accepted if:

```text
beehive-server still starts;
browser workspace selector still works;
Workspace Explorer supports selecting S3 source artifacts;
run-selected-pipeline-waves endpoint exists;
selected runner does not run unrelated pending roots;
one input producing multiple outputs is visible as multiple child artifacts;
server has request body limit;
CORS is safer than unconditional wildcard for non-local usage;
token behavior remains intact;
real S3+n8n web pilot is attempted and either passes or has a precise external blocker;
docs/feedback/report are created;
tests/build/lint pass.
```

Preferred full success:

```text
browser/HTTP selected pilot runs 1-3 real source artifacts
→ real n8n called
→ S3 outputs created
→ Beehive registers child pointers
→ source rows done/retry/failed according to actual result
→ output lineage visible through UI/API.
```

## 19. Product reminder

B7 is not for programmers.

Codex may use CLI to verify, but the resulting product behavior must be usable by a non-programmer operator through the browser:

```text
select workspace
select source artifacts
run selected pipeline
see what happened
retry or inspect failures
copy S3/output details when needed
```

Do not optimize for terminal workflows at the expense of the browser operator flow.
