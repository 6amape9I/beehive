# Beehive S3 B5 Web Transition Plan

## 1. B4 Baseline Kept

B5 keeps the accepted B4 runtime as the backend execution foundation:

- `reconcile_s3_workspace`
- `register_s3_source_artifact`
- `run_due_tasks_limited`
- `run_pipeline_waves`
- stage state machine and retry/block behavior
- S3 JSON body control envelope
- S3 manifest validation and transactional output registration
- S3 `save_path` routing

B5 does not replace the executor, S3 reconciliation, manifest validation, or wave runner. It wraps them behind clearer service/API boundaries for a web operator application.

## 2. Current Front/Back Boundary And Gaps

Current frontend code calls small TypeScript API helpers, but those helpers call Tauri `invoke` directly. Operators still open workdirs by local path, and runtime calls accept raw paths from the UI. That is acceptable for the desktop/admin shell but not for browser-based operators.

Missing boundaries:

- no server-side workspace registry;
- no stable `workspace_id` contract for browser requests;
- no HTTP-shaped API/router surface;
- no frontend adapter switch between Tauri and HTTP;
- stage creation still exposes low-level folders/routes through the Stage Editor;
- stage-run lineage does not expose a first-class one-run-to-many-outputs read model.

## 3. Service/API Architecture

Add a Rust service layer under `src-tauri/src/services/`:

- `workspaces.rs`: load/list/get registry entries and resolve workspace IDs to server-owned paths.
- `runtime.rs`: wrap B4 runtime actions for a resolved workspace.
- `pipeline.rs`: create S3 stages and sync config/SQLite.
- `artifacts.rs`: expose stage-run output lineage by `producer_run_id`.

Tauri commands and HTTP/router code should call services instead of duplicating business logic. Existing path-based Tauri commands remain during transition for dev/admin use.

## 4. Workspace Registry Design

Add non-secret registry config at `config/workspaces.yaml`.

Schema:

```yaml
workspaces:
  - id: smoke
    name: Smoke Test Workspace
    provider: s3
    bucket: steos-s3-data
    workspace_prefix: beehive-smoke/test_workflow
    region: ru-1
    endpoint: https://s3.ru-1.storage.selcloud.ru
    workdir_path: /tmp/beehive-web-workspaces/smoke
    pipeline_path: /tmp/beehive-web-workspaces/smoke/pipeline.yaml
    database_path: /tmp/beehive-web-workspaces/smoke/app.db
```

The browser receives only public workspace descriptors. Backend requests resolve `workspace_id` server-side, reject unknown IDs, and never accept arbitrary workdir/database paths from browser-originated requests. S3 credentials stay in env or the server credential chain.

## 5. Web Workspace Selector

Add a Workspace Selector page backed by the registry. In Tauri mode it can open a registered workspace through a `workspace_id` command. In HTTP mode it will call `GET /api/workspaces` and store the selected `workspace_id` in app state/router.

The selector shows workspace name, provider, bucket, prefix, region, endpoint, and current selection status. It should not expose S3 keys or server filesystem paths.

## 6. Stage Creation

Add a minimal S3 stage creation request:

```json
{
  "stage_id": "semantic_rich",
  "workflow_url": "https://n8n.example/webhook/semantic_rich",
  "next_stage": "weight_entity",
  "max_attempts": 3,
  "retry_delay_sec": 30,
  "allow_empty_outputs": false
}
```

Backend behavior:

- validate safe slug stage ID;
- reject duplicate active stage IDs;
- validate `http://` or `https://` workflow URL;
- generate `input_uri = s3://{bucket}/{workspace_prefix}/stages/{stage_id}`;
- generate save path aliases:
  - `{workspace_prefix}/stages/{stage_id}`
  - `/{workspace_prefix}/stages/{stage_id}`
  - `s3://{bucket}/{workspace_prefix}/stages/{stage_id}`
- update `pipeline.yaml` atomically using existing editor save semantics where practical;
- bootstrap/sync SQLite stages;
- do not create local directories for S3 stage routes;
- return route hints for n8n.

## 7. One-Input-To-Many-Output Lineage

Use existing `entity_files.producer_run_id` as the primary read model. Add a query/service result that returns all child artifacts for one run:

- source `run_id`;
- `output_count`;
- output `entity_id`;
- output `artifact_id`;
- target `stage_id`;
- `relation_to_source`;
- `s3_uri`;
- child runtime status;
- bucket/key/version/etag/size metadata.

This makes branching visible without relying on the legacy single `created_child_path` field.

## 8. API Endpoints

Target HTTP-shaped routes:

- `GET /api/health`
- `GET /api/workspaces`
- `GET /api/workspaces/{workspace_id}`
- `GET /api/workspaces/{workspace_id}/workspace-explorer`
- `POST /api/workspaces/{workspace_id}/reconcile-s3`
- `POST /api/workspaces/{workspace_id}/register-s3-source`
- `POST /api/workspaces/{workspace_id}/run-small-batch`
- `POST /api/workspaces/{workspace_id}/run-pipeline-waves`
- `POST /api/workspaces/{workspace_id}/stages`
- `GET /api/workspaces/{workspace_id}/stage-runs/{run_id}/outputs`

B5 will prefer a small service/router module without new network dependencies if adding a full server binary would risk the locked build. Any non-runnable HTTP work will be documented as the B6 launch step.

## 9. Frontend Client Abstraction

Add:

- `src/lib/apiClient/types.ts`
- `src/lib/apiClient/tauriClient.ts`
- `src/lib/apiClient/httpClient.ts`
- `src/lib/apiClient/index.ts`

`VITE_BEEHIVE_API_BASE_URL` selects HTTP mode. Without it, Tauri mode remains default. Existing `runtimeApi.ts` and `bootstrapApi.ts` become compatibility wrappers over the selected adapter. React components should not import `@tauri-apps/api/core` directly.

## 10. Tests

Rust/backend:

- registry loads valid config;
- unknown workspace ID is rejected;
- browser-style workspace requests resolve paths server-side only;
- S3 stage creation generates canonical routes;
- stage creation rejects duplicate IDs and bad workflow URLs;
- stage-run output read model returns all children for a producer run;
- B4 `run_pipeline_waves` tests still pass;
- S3 control envelope tests still pass.

Frontend/build:

- `npm run build`;
- no React component imports Tauri `invoke` outside API adapters;
- workspace selector and stage creation UI type-check.

Lint:

- `python3 scripts/lint_n8n_workflows.py docs/n8n_workflows`

## 11. Not Implemented In B5

B5 will not implement high-load scheduling, async manifest polling, n8n REST workflow editing, real production workflow storage, full auth/RBAC, a large monorepo split, or removal of the Tauri desktop/admin shell.

## 12. Risks And Rollback

Risks:

- full HTTP server mode may be too much without adding new dependencies;
- existing path-based Tauri flow must keep working while registry mode is introduced;
- stage creation must not desynchronize `pipeline.yaml` and SQLite;
- exposing workspace path fields to the browser would violate the web boundary.

Rollback:

- new registry/API/UI files can be removed without touching B4 executor semantics;
- path-based Tauri commands remain as fallback;
- stage creation uses existing config validation so invalid YAML should be rejected before save.

## Checkpoints

- after_plan
- after_front_back_boundary
- after_workspace_registry
- after_web_api_design
- after_workspace_selector_ui
- after_stage_creation_ui
- after_multi_output_lineage
- after_tests
- before_feedback
