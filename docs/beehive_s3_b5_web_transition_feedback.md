# Beehive S3 B5 Web Transition Feedback

## 1. B4 Work Kept

B4 runtime stayed intact. `run_pipeline_waves`, `run_due_tasks_limited`, S3 reconciliation, manual S3 source registration, S3 JSON control envelope, manifest validation, save_path routing, and the state machine were reused rather than rewritten.

## 2. Web/Front-Back Transition Implemented

Added a backend service layer and frontend API client boundary. React compatibility wrappers now delegate through `src/lib/apiClient/`, and direct Tauri `invoke` is isolated to `tauriClient.ts`.

## 3. Files Changed

Added:

- `config/workspaces.yaml`
- `docs/beehive_s3_b5_web_transition_plan.md`
- `docs/beehive_s3_b5_web_transition_feedback.md`
- `docs/front_back_split.md`
- `docs/workspace_registry.md`
- `docs/stage_creation_s3_ui_contract.md`
- `docs/multi_output_lineage.md`
- `src-tauri/src/services/*`
- `src-tauri/src/http_api/mod.rs`
- `src/lib/apiClient/*`
- `src/pages/WorkspaceSelectorPage.tsx`

Updated Rust domain/commands/bootstrap/lib, frontend app shell/routes/context/styles, runtime/bootstrap APIs, Workspace Explorer, Stage Editor, Entity Detail, and Stage Runs panel.

## 4. Workspace Registry Behavior

`config/workspaces.yaml` is the default server-side registry, with `BEEHIVE_WORKSPACES_CONFIG` override. Browser-facing descriptors expose no server paths or secrets. Backend validates safe IDs, absolute paths, bucket/prefix requirements, and rejects unknown workspace IDs.

## 5. Workspace Selector Behavior

Added `/workspaces` page. It lists registered workspaces and opens a workspace through `open_registered_workspace(workspace_id)` in Tauri mode. App state now carries `selected_workspace_id`.

## 6. Stage Creation Behavior And Routes

Added backend/API and Stage Editor UI for S3 stage creation from stage ID plus webhook URL. Backend generates:

```text
input_uri = s3://{bucket}/{workspace_prefix}/stages/{stage_id}
save_path_aliases =
  {workspace_prefix}/stages/{stage_id}
  /{workspace_prefix}/stages/{stage_id}
  s3://{bucket}/{workspace_prefix}/stages/{stage_id}
```

It validates slug, duplicate stage, workflow URL, next stage, writes `pipeline.yaml` atomically, and syncs SQLite.

## 7. HTTP API Endpoints

Implemented HTTP-shaped router/service layer for:

```text
GET /api/health
GET /api/workspaces
GET /api/workspaces/{workspace_id}
GET /api/workspaces/{workspace_id}/workspace-explorer
POST /api/workspaces/{workspace_id}/reconcile-s3
POST /api/workspaces/{workspace_id}/register-s3-source
POST /api/workspaces/{workspace_id}/run-small-batch
POST /api/workspaces/{workspace_id}/run-pipeline-waves
POST /api/workspaces/{workspace_id}/stages
GET /api/workspaces/{workspace_id}/stage-runs/{run_id}/outputs
```

No standalone `beehive-server` binary was added in B5; B6 should bind this router to a localhost dev server.

## 8. Frontend API Client Abstraction

Added Tauri and HTTP adapters. `VITE_BEEHIVE_API_BASE_URL` selects HTTP mode; otherwise Tauri remains default. `rg "@tauri-apps/api/core|invoke\\(" src -n` now returns only `src/lib/apiClient/tauriClient.ts`.

## 9. Multi-Output Lineage Behavior

Added `StageRunOutputsPayload` read model from `entity_files.producer_run_id`. Entity Detail stage-run rows can expand to show all output artifacts, target stages, child statuses, relations, and S3 URIs.

## 10. Commands Run And Results

```text
cargo fmt --manifest-path src-tauri/Cargo.toml
result: passed

cargo test --manifest-path src-tauri/Cargo.toml
result: passed; 136 passed, 0 failed, 3 ignored

npm run build
result: passed; tsc and vite build completed

python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
result: passed

rg "@tauri-apps/api/core|invoke\\(" src -n
result: only src/lib/apiClient/tauriClient.ts imports invoke

git diff --check
result: passed
```

## 11. What Could Not Be Verified

No real S3/n8n smoke was run in B5 by design. No standalone HTTP server smoke was run because B5 added the router/service layer but not the server binary.

## 12. Remaining Desktop/Tauri Dependencies

Tauri remains the default adapter. Local workdir open/initialize and the directory picker remain desktop/admin flows. Some HTTP adapter methods intentionally return unsupported until B6 exposes equivalent endpoints.

## 13. Remaining Risks

The registry sample points to `/tmp/beehive-web-workspaces/smoke`; operators must provision or create a real registered workspace. Auth is still deferred; any future server must bind localhost by default or add an operator token before non-local exposure.

## 14. B6 Next Step

B6 should add a runnable localhost web server binary, finish HTTP equivalents for remaining runtime pages, add basic dev token handling if binding beyond localhost, and move normal operator flows fully to `workspace_id`.

## 15. Reread Checkpoints

ТЗ перечитано на этапах: after_plan, after_front_back_boundary, after_workspace_registry, after_web_api_design, after_workspace_selector_ui, after_stage_creation_ui, after_multi_output_lineage, after_tests, before_feedback
