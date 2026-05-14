# B9 Entities Upload and Simplified CRUD Plan

## Scope

B9 turns the web operator MVP into a simpler production-facing flow:

- create a workspace by name only;
- create S3 stages with a production n8n webhook and save_path routing;
- upload a local folder of JSON objects into S3 and register them immediately;
- list, update, archive, restore, select, and run entities from the web UI;
- keep advanced diagnostics available without exposing route internals in normal forms.

## Checkpoints

Required rereads:

- after_plan
- after_workspace_simplification
- after_stage_save_path_only
- after_entity_crud_design
- after_upload_implementation
- after_ui_update
- after_tests
- after_smoke
- before_feedback

## Backend Plan

1. Workspace simplification
   - Make `POST /api/workspaces` accept name-only input.
   - Default S3 settings to `steos-s3-data`, `ru-1`, and `https://s3.ru-1.storage.selcloud.ru`.
   - Generate workspace id server-side.
   - Set `workspace_prefix` to the trimmed visible workspace name.
   - Reject empty, whitespace-only, path-like, parent traversal, and control-character names.

2. Stage save_path-only contract
   - Make newly created S3 stages terminal by default with `next_stage = null`.
   - Ignore or reject normal `next_stage` edits in create/update paths.
   - Keep the old `next-stage` HTTP route as a compatibility endpoint that returns `next_stage_deprecated`.
   - Map UI "Terminal stage" to `allow_empty_outputs`.

3. Entity CRUD
   - Add additive SQLite migration for entity archive/operator fields.
   - Add APIs for list/detail/update/archive/restore.
   - Hide archived entities by default.
   - Keep S3 artifacts immutable; archive only changes Beehive DB state.

4. JSON folder import
   - Add batch import endpoint for `{ stage_id, files, options }`.
   - Validate each file independently and return partial success/failure.
   - Accept JSON objects only.
   - Upload to `s3://steos-s3-data/{workspace_prefix}/stages/{stage_id}/{file_name}`.
   - Add S3 metadata for entity, artifact, and stage ids.
   - Register each uploaded file as a pending source artifact immediately.

## Frontend Plan

1. Workspace selector
   - Show only workspace name in create/edit forms.
   - Hide bucket, endpoint, region, prefix, paths, pipeline, and DB fields from the normal UI.

2. Stage editor
   - Remove Connect Stages and next-stage controls from normal UI.
   - Show Stage ID, production webhook URL, retry settings, and Terminal stage.
   - Keep generated save_path aliases copyable but not editable.

3. Entities page
   - Add upload JSON folder flow using browser File API.
   - Show import progress, per-file failures, and registered entity rows.
   - Support archive/restore, note update, selection, and selected pipeline waves.

4. Diagnostics
   - Keep YAML/raw validation and route internals under Advanced/Diagnostics.

## Verification Plan

- Rust tests for workspace name-only creation, defaults, unsafe name rejection, stage save_path-only behavior, deprecated next-stage route, entity archive/restore, and batch import validation.
- Frontend builds with and without `VITE_BEEHIVE_API_BASE_URL`.
- n8n workflow lint and `git diff --check`.
- Add `scripts/web_operator_entities_smoke.mjs` using a temporary registry/workspace root.
