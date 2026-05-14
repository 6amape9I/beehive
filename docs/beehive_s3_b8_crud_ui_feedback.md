# B8 CRUD UI Feedback

## Что сделано

B8 перевёл web MVP ближе к операторскому приложению:

- добавлен Workspace CRUD через registry API;
- добавлен Stage CRUD поверх `pipeline.yaml` и SQLite lifecycle sync;
- default UI упрощён для операторских действий;
- Diagnostics/Advanced сохранили технические детали, но скрыты по умолчанию;
- добавлен HTTP CRUD smoke на временном registry/workspace root.

## Какие файлы изменены

Ключевые B8 файлы:

- `instructions/codex_b8_operator_crud_ui_simplification_instruction.md`
- `docs/beehive_s3_b8_crud_ui_plan.md`
- `docs/beehive_s3_b8_crud_ui_feedback.md`
- `docs/operator_crud_runbook.md`
- `src-tauri/src/services/workspaces.rs`
- `src-tauri/src/services/pipeline.rs`
- `src-tauri/src/http_api/mod.rs`
- `src-tauri/src/http_server.rs`
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/domain/mod.rs`
- `src-tauri/src/database/mod.rs`
- `src/pages/WorkspaceSelectorPage.tsx`
- `src/pages/StageEditorPage.tsx`
- `src/pages/WorkspaceExplorerPage.tsx`
- `src/lib/apiClient/*`
- `src/lib/bootstrapApi.ts`
- `src/lib/runtimeApi.ts`
- `src/types/domain.ts`
- `src/app/styles.css`
- `scripts/web_operator_crud_smoke.mjs`

Existing unrelated worktree changes were present in `.gitignore` and `upload_updated_raw_to_s3.py`; B8 did not rely on them.

## Как работает Workspace CRUD

Routes:

```text
GET    /api/workspaces?include_archived=true|false
POST   /api/workspaces
PATCH  /api/workspaces/{workspace_id}
DELETE /api/workspaces/{workspace_id}
POST   /api/workspaces/{workspace_id}/restore
```

Create accepts only operator fields: `name`, `bucket`, `workspace_prefix`, `region`, `endpoint`, optional `id`. Server paths are generated under `BEEHIVE_WORKSPACES_ROOT`, default `/tmp/beehive-web-workspaces`.

Create initializes directory, S3 `pipeline.yaml`, SQLite `app.db`, and atomically writes `workspaces.yaml` with backup.

Update allows `name`, `endpoint`, `region`. Bucket/prefix changes are rejected when the workspace has stages, registered artifacts, or run history.

Delete never removes S3 objects. Empty workspaces are hard-deleted from registry; non-empty workspaces are archived with `is_archived=true`.

Old registry entries without `created_at`, `updated_at`, `archived_at`, and `is_archived` still load.

## Как работает Stage CRUD

Routes:

```text
POST   /api/workspaces/{workspace_id}/stages
PATCH  /api/workspaces/{workspace_id}/stages/{stage_id}
DELETE /api/workspaces/{workspace_id}/stages/{stage_id}
POST   /api/workspaces/{workspace_id}/stages/{stage_id}/restore
POST   /api/workspaces/{workspace_id}/stages/{stage_id}/next-stage
```

Create remains simple: `stage_id`, production webhook URL, optional `next_stage`, retry settings, `allow_empty_outputs`. Beehive generates S3 `input_uri` and `save_path_aliases`.

Patch changes only operator-safe fields: `workflow_url`, `max_attempts`, `retry_delay_sec`, `allow_empty_outputs`, `next_stage`.

Delete blocks inbound `next_stage` references. Empty unlinked stages are removed from active `pipeline.yaml`. Stages with runtime history are archived/deactivated through SQLite sync, preserving history and S3 objects.

Restore re-adds an inactive SQLite stage when no active duplicate exists.

## Что упрощено в UI

`/workspaces` now supports create, edit, archive/delete, restore, Show archived, and select. Cards show only name, id, bucket, workspace prefix, stage count, and active/archived status.

`/workspaces/{id}/workspace` now leads with workspace status, pending/failed/blocked/done counts, Reconcile S3, Run selected pipeline waves, and Add stage.

`/workspaces/{id}/stages` now has operator CRUD: add, edit, connect, archive/delete, restore, and copy save_path aliases.

Workspace Explorer default table now focuses on checkbox, entity/artifact, S3 key/path, status, validation, and actions.

## Что спрятано в Advanced/Diagnostics

Hidden by default:

- detailed reconciliation counters;
- manual S3 source registration;
- broad small-batch queue action;
- broad pipeline wave result table;
- stage system paths and detailed counters;
- YAML preview and raw validation;
- checksum/etag-style diagnostic table columns.

## Команды и результаты

```text
cargo fmt --manifest-path src-tauri/Cargo.toml
result: passed

cargo test --manifest-path src-tauri/Cargo.toml
result: ok, 159 passed, 0 failed, 3 ignored

npm run build
result: passed, dist/assets/index-BJAZQW9N.js 414.26 kB

VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
result: passed, dist/assets/index-BWyaFIwt.js 410.03 kB

python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
result: passed, no output

rg "@tauri-apps/api/core|invoke\(" src -n
result: only src/lib/apiClient/tauriClient.ts

git diff --check
result: passed, no output
```

## Результаты тестов

Added/covered:

- workspace create initializes registry/files;
- duplicate workspace id rejected;
- old registry entries without metadata load;
- dangerous bucket/prefix update rejected when non-empty/history exists;
- workspace archive/delete and restore;
- stage update changes workflow/retry settings;
- stage delete blocks inbound `next_stage`;
- stage hard delete for empty unlinked stage;
- stage archive with history and restore;
- HTTP CRUD routes parse request bodies;
- B7 selected-run route still parses.

## Результаты smoke

Temporary server:

```text
BEEHIVE_WORKSPACES_CONFIG=/tmp/beehive-b8-crud-smoke/workspaces.yaml
BEEHIVE_WORKSPACES_ROOT=/tmp/beehive-b8-crud-smoke/workspaces
BEEHIVE_SERVER_PORT=8789
```

Smoke command:

```text
BEEHIVE_API_BASE_URL=http://127.0.0.1:8789 node scripts/web_operator_crud_smoke.mjs
```

Result:

```json
{"ok":true,"api_base":"http://127.0.0.1:8789","workspace_id":"crud-smoke-1778749718410-bbebd7","delete_workspace":"archived","blocked_delete_code":"delete_s3_stage_failed"}
```

The non-escalated local bind/fetch attempts were blocked by sandbox permissions; escalated local-only server/smoke passed. The temporary server was stopped after smoke.

## Риски

- B8 CRUD is still synchronous MVP behavior, not background job orchestration.
- Stage restore depends on existing inactive SQLite stage history.
- Bucket/prefix update for non-empty workspaces is intentionally conservative.
- Manual browser click/screenshot QA was not run; builds and HTTP smoke passed.
- Production n8n workflow audit remains outside B8.

## Что делать в B9

- Add browser automation screenshots for workspace CRUD, stage CRUD, and selected-run flow.
- Add confirmation modals for destructive/archive actions.
- Add richer operator retry/reset UX for failed selected sources.
- Add async job/progress model if CRUD and selected runs become long-running.
- Decide deployment defaults for `BEEHIVE_WORKSPACES_ROOT`, token, and CORS.

## Checkpoints

ТЗ перечитано на этапах: after_plan, after_workspace_crud_design, after_stage_crud_design, after_backend_crud, after_ui_simplification, after_tests, after_smoke, before_feedback
