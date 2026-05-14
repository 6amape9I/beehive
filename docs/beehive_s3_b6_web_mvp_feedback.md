# B6 Web Operator MVP Feedback

## 1. What Was Implemented

- Added runnable `beehive-server` binary with JSON API and static `dist/` frontend serving.
- Added browser-safe workspace-id API flow for workspace selection, explorer, S3 registration, run controls, S3 stage creation, stage linking, and stage-run outputs.
- Added frontend workspace routes:
  - `/workspaces`
  - `/workspaces/:workspaceId/workspace`
  - `/workspaces/:workspaceId/stages`
  - `/workspaces/:workspaceId/entities/:entityId`
- Added HTTP-mode workspace selector behavior that stores `selected_workspace_id` and avoids Tauri `openRegisteredWorkspace()`.
- Added simple connect-stages API/UI for `source_stage_id -> target_stage_id` or terminal clear.
- Added web-compatible multi-output expansion through `GET /api/workspaces/{workspace_id}/stage-runs/{run_id}/outputs`.
- Fixed S3 `save_path_aliases` config validation so generated aliases remain compatible with the existing S3 save_path router:
  - logical route
  - `/workspace_prefix/...`
  - matching `s3://bucket/...`

## 2. Preserved B5/B4 Pieces

B6 reuses the existing B5 service layer, workspace registry, API client boundary, S3 stage creation service, and multi-output lineage read model. It also preserves B4 `run_pipeline_waves`, B3/B4 `run_due_tasks_limited`, S3 reconciliation, manual S3 source registration, JSON body control envelope, manifest validation, save_path routing, S3 pointer registration, and state-machine retry/block behavior.

## 3. How To Start Server

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
```

The server logs the listening URL and workspace registry path.

## 4. Host And Port

Defaults:

```text
host = 127.0.0.1
port = 8787
```

Environment overrides:

```text
BEEHIVE_SERVER_HOST
BEEHIVE_SERVER_PORT
BEEHIVE_WORKSPACES_CONFIG
```

## 5. Working Endpoints

Verified or wired endpoints:

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

## 6. Token And Non-Local Protection

Default bind is local-only. Non-local bind requires:

```text
BEEHIVE_SERVER_ALLOW_NON_LOCAL=1
BEEHIVE_OPERATOR_TOKEN=<token>
```

If `BEEHIVE_OPERATOR_TOKEN` is set, API requests require:

```text
Authorization: Bearer <token>
```

The server does not log S3 keys, S3 secrets, or operator tokens.

## 7. Frontend HTTP Mode

HTTP-mode production build:

```bash
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
```

`beehive-server` serves the built frontend from `dist/`, so the browser entrypoint is:

```text
http://127.0.0.1:8787/
```

## 8. Browser And Operator Scenarios Checked

- `/api/workspaces` loads the registry.
- Static frontend is served from `/`.
- Workspace explorer loads through `workspace_id`.
- S3 stage creation works through the HTTP API.
- Stage linking works through the HTTP API.
- `run-small-batch` and `run-pipeline-waves` work on the idle local smoke workspace.
- Manual S3 source artifact registration works through the HTTP API.
- Workspace explorer shows S3 pointer metadata, pending runtime status, bucket/key/S3 URI, and next-stage link.
- Stage-run outputs endpoint returns an `outputs[]` payload.

## 9. Stage Creation Evidence

HTTP smoke created stage `web_mvp_created` in registered workspace `smoke`.

Generated route hints:

```text
input_uri = s3://steos-s3-data/beehive-smoke/test_workflow/stages/web_mvp_created
save_path_aliases =
  beehive-smoke/test_workflow/stages/web_mvp_created
  /beehive-smoke/test_workflow/stages/web_mvp_created
  s3://steos-s3-data/beehive-smoke/test_workflow/stages/web_mvp_created
```

The API returned `errors: []` and wrote an atomic `pipeline.yaml` backup under `/tmp/beehive-web-workspaces/smoke/`.

## 10. Stage Linking Evidence

HTTP smoke linked:

```text
smoke_source -> web_mvp_created
```

The API returned `errors: []`, and subsequent workspace explorer output showed:

```text
stage_id = smoke_source
next_stage = web_mvp_created
```

## 11. Multi-Output Lineage Evidence

The HTTP endpoint:

```text
GET /api/workspaces/smoke/stage-runs/1/outputs
```

returned:

```json
{"errors":[],"payload":{"output_count":0,"outputs":[],"run_id":"1"}}
```

No real stage run with children existed in the B6 smoke workspace, so one-to-many output rows were verified by Rust tests and the web-compatible endpoint shape was verified by HTTP smoke.

## 12. Commands And Exact Results

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
```

Passed.

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Passed: `141 passed; 0 failed; 3 ignored`.

```bash
npm run build
```

Passed: Vite built `dist/assets/index-B7n2U3jY.js`.

```bash
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
```

Passed: Vite built `dist/assets/index-Dn3Gak4X.js`.

```bash
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
```

Passed.

```bash
rg "@tauri-apps/api/core|invoke\(" src -n
```

Only result: `src/lib/apiClient/tauriClient.ts`.

```bash
git diff --check
```

Passed.

Server/API smoke:

```text
GET /api/health -> {"status":"ok"}
GET /api/workspaces -> workspace smoke, errors []
GET / -> built HTML served from dist/
GET /api/workspaces/smoke/workspace-explorer -> errors []
POST /api/workspaces/smoke/stages -> errors []
POST /api/workspaces/smoke/stages/smoke_source/next-stage -> errors []
POST /api/workspaces/smoke/run-small-batch -> errors [], claimed 0
POST /api/workspaces/smoke/run-pipeline-waves -> errors [], stopped_reason idle
POST /api/workspaces/smoke/register-s3-source -> errors [], pending S3 pointer registered
GET /api/workspaces/smoke/stage-runs/1/outputs -> errors [], output_count 0
```

Note: first server start inside the command sandbox failed with `Operation not permitted (os error 1)` on bind. The same server command passed when run with approved escalation, which is consistent with the sandbox denying local listening sockets.

## 13. Not Verified

- `npm run dev` was not started because the B6 server successfully serves the built frontend from `dist/`.
- Real S3 reconciliation was not triggered in this B6 smoke to avoid external network/runtime side effects.
- Real n8n webhook execution was not triggered; B6 only verified the browser/API operator path on a local registered smoke workspace.
- No real stage run with output children was available during HTTP smoke; child-output behavior remains covered by existing Rust tests.

## 14. Remaining Unsupported HTTP Adapter Methods

Normal B6 operator path is supported. Admin/desktop-oriented methods that still remain unsupported or Tauri-first in HTTP mode include path-based workdir initialization/open/reload, local filesystem scan/open actions, dashboard/editor actions that depend on arbitrary local paths, and direct file editing commands.

## 15. Remaining Tauri Dependencies

The desktop app and Tauri command layer remain in place. React imports Tauri only through `src/lib/apiClient/tauriClient.ts`; the browser HTTP flow uses `src/lib/apiClient/httpClient.ts`.

## 16. Risks Before B7

- Auth is token-only and not full RBAC.
- The server is single-process MVP, not a background worker daemon.
- No multi-user locking model exists for concurrent pipeline edits.
- Real S3/n8n pilot should be rerun through the web path before broader operator use.
- Static serving is intentionally minimal.

## 17. Next Stage Handoff

B7 should focus on real operator hardening: run a real S3/n8n web-triggered pilot, add clearer frontend smoke automation, decide which remaining Tauri/admin actions need HTTP equivalents, and introduce a production deployment story for server lifecycle, logs, and token management.

## 18. Checkpoints

ТЗ перечитано на этапах: after_plan, after_server_binary, after_workspace_routes, after_frontend_http_flow, after_stage_creation_and_links, after_multi_output_ui, after_tests, after_http_smoke, before_feedback
