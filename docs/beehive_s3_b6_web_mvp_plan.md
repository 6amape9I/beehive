# Beehive S3 B6 Web MVP Plan

## 1. What I Understand

B6 turns the B5 web-ready abstraction into a minimal browser-first operator MVP. The admin/developer may still use CLI to start the server and run checks, but normal operator actions must happen through a browser using `workspace_id`, not arbitrary local paths.

## 2. B5 Pieces Reused

B6 will keep and reuse:

- B4 `run_pipeline_waves`
- B3/B4 `run_due_tasks_limited`
- S3 reconciliation and manual S3 source registration
- S3 JSON control envelope
- manifest validation and transactional output registration
- save_path routing
- B5 service layer
- B5 workspace registry
- B5 API client boundary
- B5 S3 stage creation service
- B5 multi-output lineage read model

## 3. Server Binary

Add:

```text
src-tauri/src/bin/beehive-server.rs
```

The server will use the existing `http_api` router and a small standard-library HTTP loop to avoid adding new dependencies. It will serve JSON API and, when `dist/` exists, static frontend files for a simple browser entrypoint.

## 4. Bind And Protection

Defaults:

```text
BEEHIVE_SERVER_HOST=127.0.0.1
BEEHIVE_SERVER_PORT=8787
```

Non-local bind requires:

```text
BEEHIVE_SERVER_ALLOW_NON_LOCAL=1
BEEHIVE_OPERATOR_TOKEN=<token>
```

If `BEEHIVE_OPERATOR_TOKEN` is set, all non-OPTIONS API requests require `Authorization: Bearer <token>`, including localhost. Tokens and S3 secrets must not be logged.

## 5. HTTP Endpoints

B6 will make these endpoints available through the server:

- `GET /api/health`
- `GET /api/workspaces`
- `GET /api/workspaces/{workspace_id}`
- `GET /api/workspaces/{workspace_id}/workspace-explorer`
- `POST /api/workspaces/{workspace_id}/reconcile-s3`
- `POST /api/workspaces/{workspace_id}/register-s3-source`
- `POST /api/workspaces/{workspace_id}/run-small-batch`
- `POST /api/workspaces/{workspace_id}/run-pipeline-waves`
- `POST /api/workspaces/{workspace_id}/stages`
- `POST /api/workspaces/{workspace_id}/stages/{stage_id}/next-stage`
- `GET /api/workspaces/{workspace_id}/stage-runs/{run_id}/outputs`

## 6. Workspace ID Routes

Add workspace-scoped routes:

- `/workspaces`
- `/workspaces/:workspaceId/workspace`
- `/workspaces/:workspaceId/stages`
- `/workspaces/:workspaceId/entities/:entityId?`

Old routes remain for Tauri/admin compatibility.

## 7. Workspace Selector In HTTP Mode

In HTTP mode, selecting a workspace stores `selected_workspace_id` in app state and navigates to `/workspaces/{workspace_id}/workspace`. It must not call `openRegisteredWorkspace()`.

In Tauri mode, the existing registered-workspace open flow may remain.

## 8. Create Stage

Stage creation stays simple:

- `stage_id`
- `workflow_url`
- optional existing `next_stage`
- `max_attempts`
- `retry_delay_sec`
- `allow_empty_outputs`

Backend generates `input_uri` and `save_path_aliases`. The UI shows route hints and copyable save_path aliases.

## 9. Connect Stages / Pipeline Links

Add a small service/API/UI action:

```text
source_stage_id -> target_stage_id | terminal
```

Backend validates source exists, target exists when set, rejects self-link, updates `pipeline.yaml` atomically, syncs SQLite, and returns the updated stage.

## 10. Multi-Output Lineage

Keep the B5 `producer_run_id` read model. Add web-compatible UI access from Workspace Explorer so an operator can select an S3 output row and load all sibling outputs for the same `producer_run_id` through HTTP.

Entity Detail expansion remains useful in Tauri/workdir mode.

## 11. Manual QA / Smoke

Run:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
curl -sS http://127.0.0.1:8787/api/health
curl -sS http://127.0.0.1:8787/api/workspaces
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
```

If possible, start Vite dev server in HTTP mode and check `/workspaces` loads against the API.

## 12. Not Implemented In B6

B6 will not implement scheduler/worker pools, async manifests, n8n REST editing, production workflow storage, RBAC, Postgres, multi-user locking, large production runs, or full README rewrite.

## 13. Risks And Rollback

Risks:

- static frontend serving may be basic compared to Vite;
- HTTP-mode entity pages may remain less complete than Tauri admin pages;
- auth is only token-level, not RBAC;
- registry workspace paths must exist or stage creation must provision them.

Rollback:

- server binary and HTTP route additions are additive;
- existing Tauri path-based commands remain available;
- B4 runtime code remains untouched.

## 14. Checkpoints

- after_plan
- after_server_binary
- after_workspace_routes
- after_frontend_http_flow
- after_stage_creation_and_links
- after_multi_output_ui
- after_tests
- after_http_smoke
- before_feedback
