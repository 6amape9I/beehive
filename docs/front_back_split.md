# Beehive Front/Back Split

## Direction

Beehive is moving from a Tauri/local-workdir shell toward:

```text
browser UI -> Beehive backend/API -> server-side workspace registry + SQLite control DB -> S3 + n8n
```

The Tauri shell remains as a development/admin adapter during the transition.

## Backend Boundary

Backend business logic is now grouped under:

- `src-tauri/src/services/workspaces.rs`
- `src-tauri/src/services/runtime.rs`
- `src-tauri/src/services/pipeline.rs`
- `src-tauri/src/services/artifacts.rs`
- `src-tauri/src/services/selected_runner.rs`

Tauri commands call these services for registry workspace selection, workspace-ID runtime actions, S3 stage creation, selected batch execution, and stage-run output lineage. Existing path-based Tauri commands remain for local/admin workdir flows.

## API Boundary

`src-tauri/src/http_api/mod.rs` defines JSON routing for:

- `GET /api/health`
- `GET /api/workspaces`
- `GET /api/workspaces/{workspace_id}`
- `GET /api/workspaces/{workspace_id}/workspace-explorer`
- `POST /api/workspaces/{workspace_id}/reconcile-s3`
- `POST /api/workspaces/{workspace_id}/register-s3-source`
- `POST /api/workspaces/{workspace_id}/run-small-batch`
- `POST /api/workspaces/{workspace_id}/run-pipeline-waves`
- `POST /api/workspaces/{workspace_id}/run-selected-pipeline-waves`
- `POST /api/workspaces/{workspace_id}/stages`
- `POST /api/workspaces/{workspace_id}/stages/{stage_id}/next-stage`
- `GET /api/workspaces/{workspace_id}/stage-runs/{run_id}/outputs`

B6 attaches this router to `src-tauri/src/bin/beehive-server.rs`. The MVP server binds to `127.0.0.1:8787` by default, supports token checks for non-local exposure, and can serve built frontend assets from `dist/`. B7 adds a default request body limit, CORS origin allow-listing, structured request/workspace logs, and the selected-run endpoint for operator-approved S3 batches.

## Frontend Boundary

React code uses `src/lib/apiClient/`:

- `types.ts`: shared frontend client interface.
- `tauriClient.ts`: only frontend module that imports Tauri `invoke`.
- `httpClient.ts`: fetch-based adapter selected by `VITE_BEEHIVE_API_BASE_URL`.
- `index.ts`: selects HTTP mode when an API base URL is set, otherwise Tauri mode.

Compatibility wrappers remain in `runtimeApi.ts` and `bootstrapApi.ts`, but they delegate to the selected client.

The HTTP client sends `Authorization: Bearer <token>` when `VITE_BEEHIVE_OPERATOR_TOKEN` or browser `localStorage.BEEHIVE_OPERATOR_TOKEN` is set.
