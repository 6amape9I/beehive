# B5. Web Operator MVP Transition, Front/Back Boundary, and Multi-Output Lineage

## 0. Mission

You are Codex-agent continuing Beehive after the accepted S3 B4 stage.

B4 successfully added a bounded manual pipeline wave runner and proved a small real multi-stage S3+n8n pilot. That work is accepted as a backend/runtime capability. B5 must redirect the product toward the correct operator experience: a web application for non-programmer colleagues.

Main B5 goal:

```text
Turn Beehive from a Tauri/local-workdir operator shell into a web-ready control-plane application with explicit front/back boundaries, a server-side workspace registry, workspace selection UI, stage creation UI, and clear one-input-to-many-output lineage.
```

Normal operators must not use terminal commands, ignored Rust tests, or local file paths. Those remain developer/QA tools only.

## 1. Strategic context

Beehive is control plane.
n8n is data plane.
S3 is business artifact storage.

In S3 production mode Beehive sends n8n only a technical JSON control envelope:

```text
POST workflow_url
Content-Type: application/json; charset=utf-8
body.schema = beehive.s3_control_envelope.v1
```

Beehive must not send business JSON to n8n. n8n downloads exactly `source_bucket/source_key`, writes business outputs to S3, and returns `beehive.s3_artifact_manifest.v1`.

The product direction is now:

```text
browser UI -> Beehive backend/API -> server-side workspace registry + control DB -> S3 + n8n webhooks
```

The old Tauri desktop shell may remain as a development/admin shell during transition, but the primary UX target is a browser application.

## 2. Read first

Read these before coding:

```text
docs/beehive_s3_b3_feedback.md
docs/beehive_s3_b4_feedback.md
docs/beehive_s3_b4_plan.md
docs/s3_n8n_contract.md
docs/s3_operator_runbook.md
docs/n8n_workflow_authoring_standard.md
src-tauri/src/commands/mod.rs
src-tauri/src/executor/mod.rs
src-tauri/src/s3_control_envelope.rs
src-tauri/src/s3_reconciliation.rs
src/pages/WorkspaceExplorerPage.tsx
src/lib/runtimeApi.ts
src/types/domain.ts
```

Also inspect current package/app layout:

```text
package.json
src/
src-tauri/
```

## 3. Required plan before code

Create:

```text
docs/beehive_s3_b5_web_transition_plan.md
```

The plan must include:

```text
1. What B4 implemented and what B5 will keep.
2. Current front/back boundaries and missing boundaries.
3. Proposed service/API architecture.
4. Workspace registry design.
5. Web workspace selector design.
6. Stage creation design.
7. One-input-to-many-output lineage design.
8. API endpoints to add.
9. Frontend client abstraction design.
10. Tests to add.
11. What will not be implemented in B5.
12. Risks and rollback.
```

Do not start code before writing the plan.

## 4. Reread checkpoints

Reread this instruction at checkpoints:

```text
after_plan
after_front_back_boundary
after_workspace_registry
after_web_api_design
after_workspace_selector_ui
after_stage_creation_ui
after_multi_output_lineage
after_tests
before_feedback
```

Feedback must contain:

```text
ТЗ перечитано на этапах: after_plan, after_front_back_boundary, after_workspace_registry, after_web_api_design, after_workspace_selector_ui, after_stage_creation_ui, after_multi_output_lineage, after_tests, before_feedback
```

## 5. Key non-negotiable decisions

### 5.1 Keep B4 runtime, do not throw it away

Keep the B4 wave runner as a useful backend action. It should become an API/UI action, not a terminal-first feature.

Do not rewrite the runtime from scratch. Reuse existing:

```text
reconcile_s3_workspace
register_s3_source_artifact
run_due_tasks_limited
run_pipeline_waves
stage state machine
S3 manifest validation
S3 save_path routing
```

### 5.2 Web app is the primary operator UX

Normal operator path should be:

```text
open browser -> choose workspace -> inspect stages/pipeline/statuses -> create stage -> reconcile/run/retry
```

Tauri can stay temporarily, but B5 must create a web-ready frontend/backend boundary.

### 5.3 Workdir becomes Workspace

For S3 mode, the operator should not choose a local folder path. They choose a workspace from a server-side registry.

A workspace represents:

```text
workspace_id
workspace_name
bucket
workspace_prefix
region
endpoint
pipeline config
control database
stages
runtime state
```

The server owns paths and secrets. The browser never receives S3 secrets.

### 5.4 Stage creation is simple

A non-programmer creates a stage using:

```text
stage_id / stage name
production n8n webhook URL
optional next stage / pipeline edge
optional max_attempts / retry_delay_sec with defaults
optional allow_empty_outputs
```

The operator must not manually type input folders/save folders.

Backend generates S3 routes:

```text
input_uri = s3://{bucket}/{workspace_prefix}/stages/{stage_id}
save_path_aliases =
  {workspace_prefix}/stages/{stage_id}
  /{workspace_prefix}/stages/{stage_id}
  s3://{bucket}/{workspace_prefix}/stages/{stage_id}
```

For a linear pipeline, n8n output from stage A should use the save_path alias of stage B. The UI should show/copy the target stage save_path for the n8n operator, but the normal app user should not manually configure storage paths.

### 5.5 Multi-output is a first-class feature

One source artifact may produce:

```text
0 outputs only if allow_empty_outputs=true
1 output
N outputs into one target stage
N outputs into multiple target stages through save_path branching
```

Beehive must show this clearly:

```text
source artifact
producer run_id
output_count
child artifacts
child target stages
child runtime statuses
S3 URIs
relation_to_source
```

Do not hide one-to-many branching behind a single `created_child_path` field in the UI.

## 6. Front/back boundary work

Current project is React + Tauri/Rust. Do not perform a risky wholesale monorepo move unless it is trivial and safe. B5 should create a clear logical separation first.

### 6.1 Backend service layer

Create or improve a backend service layer so Tauri commands and future HTTP endpoints call the same Rust functions.

Suggested modules:

```text
src-tauri/src/services/mod.rs
src-tauri/src/services/workspaces.rs
src-tauri/src/services/pipeline.rs
src-tauri/src/services/runtime.rs
src-tauri/src/services/artifacts.rs
```

Tauri commands should become thin adapters. Do not duplicate business logic in Tauri command handlers and HTTP handlers.

### 6.2 HTTP API layer

Add a web API server mode. Recommended Rust approach: `axum` or another small Rust HTTP framework. Keep this incremental.

Possible implementation:

```text
src-tauri/src/http_api/mod.rs
src-tauri/src/bin/beehive-server.rs
```

If adding a standalone bin is too large, create the service/router module and tests first, and document exactly how it will be launched in B6. But B5 should preferably provide a runnable dev server:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
```

Minimum endpoints:

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
```

If implementation names differ, document them clearly.

### 6.3 Frontend API client abstraction

Do not let React components import Tauri invoke directly everywhere.

Create a client boundary, for example:

```text
src/lib/apiClient/types.ts
src/lib/apiClient/tauriClient.ts
src/lib/apiClient/httpClient.ts
src/lib/apiClient/index.ts
```

`runtimeApi.ts` may be adapted or split, but final result should support two modes:

```text
Tauri adapter: invoke(...)
HTTP adapter: fetch(`${apiBase}/api/...`)
```

Frontend mode can be selected by env/config:

```text
VITE_BEEHIVE_API_BASE_URL=http://localhost:8787
```

If no API base URL is set, Tauri mode may remain default for now.

## 7. Workspace registry

Add server-side workspace registry.

Suggested file:

```text
server_data/workspaces.yaml
```

or:

```text
config/workspaces.yaml
```

Do not store secrets in this file.

Minimum schema:

```yaml
workspaces:
  - id: smoke
    name: Smoke Test Workspace
    provider: s3
    bucket: steos-s3-data
    workspace_prefix: beehive-smoke/test_workflow
    region: ru-1
    endpoint: https://s3.ru-1.storage.selcloud.ru
    workdir_path: /var/lib/beehive/workspaces/smoke
    pipeline_path: /var/lib/beehive/workspaces/smoke/pipeline.yaml
    database_path: /var/lib/beehive/workspaces/smoke/app.db
```

Backend requirements:

```text
- list workspaces;
- get workspace by id;
- reject unknown workspace id;
- never accept arbitrary database_path/workdir_path from browser requests;
- bootstrap database/config for selected workspace server-side;
- preserve existing local workdir functions for Tauri/dev mode where needed.
```

Frontend requirements:

```text
- Workspace Selector page;
- workspace cards/list;
- selected workspace stored in app state/router;
- all runtime pages use workspace_id instead of raw local path when HTTP mode is active.
```

## 8. Stage creation UI/API

Add minimal stage creation for S3 workspaces.

### 8.1 API contract

Suggested request:

```json
{
  "stage_id": "semantic_rich",
  "workflow_url": "https://n8n-dev.steos.io/webhook/...",
  "next_stage": "weight_entity",
  "max_attempts": 3,
  "retry_delay_sec": 30,
  "allow_empty_outputs": false
}
```

Backend must:

```text
- validate stage_id slug/safe id;
- reject duplicate active stage_id;
- validate workflow_url is http/https;
- generate input_uri from workspace bucket/prefix/stage_id;
- generate save_path_aliases;
- append/update pipeline.yaml atomically;
- sync SQLite stages;
- not create local S3 stage directories;
- return created stage and route hints.
```

### 8.2 UI behavior

Stage creation screen/card should ask only for:

```text
Stage ID / name
Production webhook URL
Optional next stage
Retries settings, with defaults
Allow empty outputs checkbox
```

After creation, show:

```text
input_uri
save_path aliases
copy save_path button for n8n operator
warning: n8n must return manifest outputs using one of these save_path aliases
```

Do not ask normal operator to type source/save folders.

## 9. Multi-output lineage implementation

B5 should improve UI/read-model visibility for one-to-many outputs.

If existing DB already provides enough via `entity_files.producer_run_id`, use a read model rather than creating a new table.

Add or expose:

```text
stage_run output_count
stage_run output_artifacts[]
output artifact entity_id
output artifact artifact_id
output artifact target_stage_id
output artifact relation_to_source
output artifact s3_uri
output artifact runtime_status
```

Suggested API:

```text
GET /api/workspaces/{workspace_id}/stage-runs/{run_id}/outputs
```

or include outputs inside existing entity/stage run detail payloads.

UI requirements:

```text
- Entity Detail stage run row can expand to show output artifacts;
- Workspace Explorer can show producer_run_id and branch outputs;
- when one source creates several children, user sees all children, not just first created_child_path;
- branching by save_path shows target stage labels.
```

Important runtime invariant:

```text
source state becomes done only after all manifest outputs are validated and registered transactionally;
children become pending;
unknown/unsafe/ambiguous save_path blocks the run;
if registration conflicts, source must not silently become done.
```

## 10. n8n workflow handling

Do not store real production n8n pipelines in the code repository.

Keep only tiny contract examples and docs. Real n8n workflows are external artifacts supplied by the human when a review is needed.

Do not overbuild the linter. Keep lightweight preflight only:

```text
- POST webhook;
- responseNode mode;
- no source_key headers;
- no Search/List Bucket as production input selector;
- no /main_dir/pocessed typo;
- no old node references like Read Beehive headers;
- body.source_bucket/body.source_key used in S3 download.
```

If the human supplies workflow JSONs, analyze them and write findings, but do not commit full production workflows unless explicitly asked.

Known live-smoke issue to document/check: a workflow may read body fields in `Edit Fields` but downstream Code node may still reference old `$('Read Beehive headers')`. That should be caught by preflight or runbook notes.

## 11. Security and secrets

B5 is not a full auth product, but web mode must not expose secrets.

Requirements:

```text
- no S3_KEY/S3_SEC_KEY in browser responses;
- no credentials in workspace registry;
- backend reads env/credential chain server-side;
- browser sends workspace_id, not arbitrary paths;
- API validates workspace_id against registry;
- bind dev server to localhost by default;
- if server is configured to bind beyond localhost, require a simple operator token or document that auth is deferred and unsafe for public networks.
```

## 12. Tests required

Rust/backend:

```text
workspace registry loads valid config;
unknown workspace id rejected;
workspace path cannot be supplied by browser request;
stage creation generates input_uri and save_path_aliases;
stage creation rejects duplicate stage_id;
stage creation rejects bad workflow_url;
service layer works from both command adapter and HTTP/API adapter where implemented;
run_pipeline_waves still passes;
S3 control envelope tests still pass;
one-to-many output read model returns all children for producer_run_id.
```

Frontend:

```text
npm run build passes;
workspace selector renders from mocked API/client data;
stage creation form validates required fields;
S3 operator console still builds;
no React component imports Tauri invoke directly outside the API client adapter.
```

Python/lint:

```text
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
```

Do not run real S3/n8n tests by default. Real tests remain ignored/opt-in.

## 13. Verification commands

Run:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
git diff --check
```

If web server mode is implemented, also run a local smoke:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
curl http://localhost:8787/api/health
curl http://localhost:8787/api/workspaces
```

If this cannot be run in the environment, feedback must say why.

## 14. Required docs and feedback

Create:

```text
docs/beehive_s3_b5_web_transition_plan.md
docs/beehive_s3_b5_web_transition_feedback.md
docs/front_back_split.md
docs/workspace_registry.md
docs/stage_creation_s3_ui_contract.md
docs/multi_output_lineage.md
```

Update existing docs only where necessary. Do not rewrite the entire README in B5.

Feedback must include:

```text
1. What B4 work was kept.
2. What web/front-back transition was implemented.
3. Files changed.
4. Workspace registry behavior.
5. Workspace selector behavior.
6. Stage creation behavior and generated S3 routes.
7. HTTP API endpoints added or planned.
8. Frontend API client abstraction status.
9. Multi-output lineage behavior.
10. Commands run and exact results.
11. What could not be verified.
12. Remaining desktop/Tauri dependencies.
13. Remaining risks.
14. What should be done in B6.
15. Reread checkpoints.
```

## 15. Acceptance criteria

B5 is acceptable if:

```text
- B4 wave-runner remains functional;
- front/back boundary is explicit in code and docs;
- React no longer depends directly on Tauri invoke outside a dedicated adapter;
- server-side workspace registry exists;
- UI can list/select workspaces or has a working mocked/dev implementation backed by the registry/API;
- stage creation from stage_id + webhook URL exists at backend/API level and preferably UI level;
- stage creation auto-generates S3 input_uri and save_path_aliases;
- multi-output lineage is visible through API/read model and at least partially in UI;
- S3 JSON body control envelope remains production contract;
- tests/build pass;
- no secrets are exposed to frontend;
- normal operator workflow does not require command line.
```

## 16. What B5 must not do

Do not:

```text
- rewrite the whole app;
- delete accepted B4 runtime features;
- implement a high-load scheduler;
- implement async manifest polling;
- implement n8n REST workflow editing;
- store real production n8n workflows in repo;
- expose S3 secrets to browser;
- accept arbitrary local paths from browser requests;
- force operators to type S3 folders/save folders;
- build a full enterprise auth system unless a simple dev-safe token is trivial.
```

## 17. B6 preview

Expected B6 focus after B5:

```text
manual QA of web UI;
workspace/pipeline polish;
operator-friendly approval/selection of source batches;
retry/block recovery UX;
larger controlled dry runs;
optional auth hardening;
final product documentation.
```
