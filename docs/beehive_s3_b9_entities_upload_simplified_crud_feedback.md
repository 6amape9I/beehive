# B9 Entities Upload and Simplified CRUD Feedback

## What Changed

B9 simplifies the operator path around the current S3 production standard:

- workspace creation now accepts only `name` in the normal UI/API flow;
- backend generates `workspace_id`, server paths, `pipeline.yaml`, and SQLite DB;
- new workspaces default to bucket `steos-s3-data`, region `ru-1`, endpoint `https://s3.ru-1.storage.selcloud.ru`;
- `workspace_prefix` is the trimmed workspace name;
- S3 stage create/update ignores `next_stage`; new S3 stages always have `next_stage = null`;
- the old `next-stage` route now returns `next_stage_deprecated`;
- UI uses `Terminal stage` instead of `allow_empty_outputs`;
- entity list/detail/update/archive/restore APIs were added;
- folder upload for JSON object files was added to the workspace entities UI;
- uploaded JSON files are written to S3 and registered immediately as pending source artifacts.

## Files Changed

Backend/runtime:

- `src-tauri/src/domain/mod.rs`
- `src-tauri/src/database/mod.rs`
- `src-tauri/src/services/workspaces.rs`
- `src-tauri/src/services/pipeline.rs`
- `src-tauri/src/services/entities.rs`
- `src-tauri/src/services/mod.rs`
- `src-tauri/src/s3_client.rs`
- `src-tauri/src/http_api/mod.rs`
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/lib.rs`

Frontend/API:

- `src/types/domain.ts`
- `src/lib/apiClient/types.ts`
- `src/lib/apiClient/httpClient.ts`
- `src/lib/apiClient/tauriClient.ts`
- `src/lib/runtimeApi.ts`
- `src/app/App.tsx`
- `src/app/AppShell.tsx`
- `src/pages/WorkspaceSelectorPage.tsx`
- `src/pages/WorkspaceExplorerPage.tsx`
- `src/pages/StageEditorPage.tsx`
- `src/pages/EntitiesPage.tsx`
- `src/pages/EntityDetailPage.tsx`
- `src/components/entities/EntitiesTable.tsx`
- `src/components/entities/EntityFilters.tsx`
- `src/components/stage-editor/StageDraftForm.tsx`
- `src/components/stage-editor/StageDraftList.tsx`
- `src/components/dashboard/StageGraph.tsx`

Scripts/docs:

- `scripts/web_operator_crud_smoke.mjs`
- `scripts/web_operator_entities_smoke.mjs`
- `docs/beehive_s3_b9_entities_upload_simplified_crud_plan.md`
- `docs/beehive_s3_b9_entities_upload_simplified_crud_feedback.md`
- `docs/operator_entities_upload_runbook.md`
- `instructions/codex_b9_entities_upload_simplified_crud_instruction.md` only had trailing whitespace removed so `git diff --check` passes.

## Workspace Create Simplification

`POST /api/workspaces` accepts a name-only request:

```json
{ "name": "Медицинские сущности тест" }
```

Backend behavior:

- rejects empty, whitespace-only, path-like, `..`, slash/backslash, and control-character names;
- generates a safe ASCII id when possible;
- falls back to `workspace-YYYYMMDD-abcdef` when the name has no ASCII slug;
- writes registry and workspace files under `BEEHIVE_WORKSPACES_ROOT`;
- creates S3 `pipeline.yaml` and initializes `app.db`;
- does not store S3 secrets in workspace descriptors.

The workspace selector UI no longer asks normal users for id, bucket, region, endpoint, prefix, workdir, pipeline path, or database path.

## Stage save_path-only Changes

Normal stage create/edit now exposes:

- Stage ID;
- Production n8n webhook URL;
- Max attempts;
- Retry delay;
- Terminal stage checkbox;
- generated save_path aliases for copy.

Removed from normal stage UI:

- Connect Stages;
- Next stage;
- source/target stage dropdowns;
- manual S3 route editing.

Backend keeps compatibility structs, but normal create/update ignores `next_stage` and stores new S3 stages with `next_stage = null`. The old route returns:

```json
{
  "errors": [
    {
      "code": "next_stage_deprecated",
      "message": "next_stage is deprecated. Route outputs through n8n save_path."
    }
  ]
}
```

## Entity CRUD Behavior

Added workspace-scoped endpoints:

- `GET /api/workspaces/{workspace_id}/entities`
- `GET /api/workspaces/{workspace_id}/entities/{entity_id}`
- `PATCH /api/workspaces/{workspace_id}/entities/{entity_id}`
- `DELETE /api/workspaces/{workspace_id}/entities/{entity_id}`
- `POST /api/workspaces/{workspace_id}/entities/{entity_id}/restore`
- `POST /api/workspaces/{workspace_id}/entities/import-json-batch`

SQLite migration adds archive/operator fields to entities. Archive never deletes S3 objects. Default list hides archived entities; `include_archived=true` shows them.

Entity update is intentionally narrow for B9:

- `display_name`;
- `operator_note`.

Business JSON editing remains out of scope.

## Folder Upload Behavior

The Entities page now has `Upload entities` in normal workspace UI. Browser mode uses:

```html
<input type="file" webkitdirectory multiple accept="application/json,.json" />
```

Frontend parses JSON locally, rejects non-object JSON before upload, and sends valid files to backend in batches of 25.

Backend validates each file independently. One bad file produces an `invalid` result and does not fail the whole batch.

## S3 Key and Metadata Behavior

Valid JSON objects upload to:

```text
s3://steos-s3-data/{workspace_prefix}/stages/{stage_id}/{file_name}
```

Filename handling:

- preserves safe Cyrillic filenames;
- strips local folder path to a leaf filename;
- rejects empty names, `..`, non-JSON names, and path separators;
- adds `__{short_hash}.json` when `overwrite_existing=false` and a collision exists.

Entity/artifact identity:

- `entity_id`: `content.entity_id`, then `content.id`, then safe file stem plus short hash;
- `artifact_id`: `content.artifact_id`, then `{entity_id}__source`, then `source__{short_hash}`.

S3 metadata is written on upload:

- `beehive-entity-id`;
- `beehive-artifact-id`;
- `beehive-stage-id`.

After upload the artifact pointer is registered as `pending` on the selected stage, so it appears in the UI without a manual S3 reconcile/register step.

## UI Simplification

Normal workspace creation is name-only.

Normal stage management no longer presents the old graph/link model. Route internals remain visible only as generated aliases or in Advanced/Diagnostics-style areas.

Entities default view shows only operator-useful columns:

- checkbox;
- entity/display name;
- stage;
- status;
- short S3 key;
- updated;
- actions.

Archive/restore and selected pipeline waves are available directly from the Entities page.

## Commands Run

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
rg "@tauri-apps/api/core|invoke\\(" src -n
git diff --check
BEEHIVE_WORKSPACES_CONFIG=/tmp/beehive-b9-entities-smoke/workspaces.yaml BEEHIVE_WORKSPACES_ROOT=/tmp/beehive-b9-entities-smoke/workspaces BEEHIVE_SERVER_PORT=8787 cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 node scripts/web_operator_entities_smoke.mjs
```

## Test Results

- `cargo fmt --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo test --manifest-path src-tauri/Cargo.toml`: passed, `167 passed; 0 failed; 3 ignored`.
- `npm run build`: passed.
- `VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build`: passed.
- `python3 scripts/lint_n8n_workflows.py docs/n8n_workflows`: passed.
- `rg "@tauri-apps/api/core|invoke\\(" src -n`: passed; only `src/lib/apiClient/tauriClient.ts` imports `invoke`.
- `git diff --check`: passed.

## Smoke Results

Smoke script: `scripts/web_operator_entities_smoke.mjs`.

Temporary runtime:

- registry: `/tmp/beehive-b9-entities-smoke/workspaces.yaml`;
- workspace root: `/tmp/beehive-b9-entities-smoke/workspaces`;
- API: `http://127.0.0.1:8787`.

Successful smoke summary:

```json
{
  "ok": true,
  "api_base": "http://127.0.0.1:8787",
  "workspace_id": "b9-smoke-1778754433537-cc1559",
  "workspace_name": "B9 Smoke 1778754433537-cc1559",
  "imported": 3,
  "first_entity_id": "c_733d9ac84176",
  "first_file_id": 3,
  "archived_hidden_by_default": true,
  "selected_run_claimed": 1,
  "stage_delete": "archived",
  "workspace_delete": "archived"
}
```

The smoke used real S3 upload. It created three objects under:

```text
s3://steos-s3-data/B9 Smoke 1778754433537-cc1559/stages/raw_entities/
```

The selected-run route returned HTTP 200 and claimed one root artifact. The configured webhook is a placeholder `https://n8n.example.test/...`, so the runtime task itself failed after claim; B9 smoke only verifies that selected-run still routes and responds after upload.

During first smoke attempt, `head_object` failed because AWS SDK display text was only `service error`; the actual 404 was present in debug metadata. `src-tauri/src/s3_client.rs` now checks both display and debug text for S3 not-found status.

## Known Risks

- B9 upload sends parsed JSON objects through the API, so very large folders still need batching and body-size handling; frontend currently batches by 25.
- S3 metadata values inherit sanitized entity/artifact ids; unusual Unicode edge cases should be watched in production.
- The old local draft YAML editor still exists for advanced/local workdir flows, but the normal workspace HTTP operator flow is simplified.
- Selected-run success still depends on a real production n8n workflow and valid save_path manifest behavior.

## B10 Recommendations

- Add a real n8n-backed selected-run smoke for uploaded B9 entities.
- Add UI progress/cancel for large folder uploads.
- Add better per-file upload report export.
- Add retention/cleanup tooling for smoke prefixes in S3.
- Decide whether the old local draft YAML editor should move fully under an Advanced route.

## Checkpoint Line

ТЗ перечитано на этапах: after_plan, after_workspace_simplification, after_stage_save_path_only, after_entity_crud_design, after_upload_implementation, after_ui_update, after_tests, after_smoke, before_feedback
