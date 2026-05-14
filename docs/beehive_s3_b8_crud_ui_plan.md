# B8 CRUD UI Plan

## 1. B7 Baseline

B7 is the accepted baseline:

- `beehive-server` serves the browser UI and HTTP API.
- Workspace registry is server-side and exposes only descriptors to the browser.
- Workspace Explorer supports selected S3 source rows and `run-selected-pipeline-waves`.
- Stage creation and linking exist, but stage edit/delete/restore are incomplete.
- Server hardening, token client support, selected-run lineage, and B7 smoke helper must remain intact.

## 2. B8 Goals

B8 turns the web MVP into an operator CRUD app:

- create, edit, archive/delete, restore, and select workspaces without YAML or server paths;
- create, edit, link, archive/delete, and restore stages without manual S3 routes;
- keep selected pipeline waves working;
- simplify default UI and move technical details into Advanced/Diagnostics;
- add a CRUD smoke that uses temporary registry/root paths, not production workspaces.

## 3. Non-Goals

B8 will not add a scheduler, RBAC, Postgres, n8n REST workflow editor, workflow import/export, or large production runs.

## 4. Workspace CRUD Design

Extend `services/workspaces.rs` instead of adding a separate registry layer unless the file becomes unwieldy.

New service functions:

- `list_workspace_descriptors(include_archived)`
- `create_workspace`
- `update_workspace`
- `archive_or_delete_workspace`
- `restore_workspace`

Registry records become backward-compatible with optional metadata:

- `is_archived`
- `created_at`
- `updated_at`
- `archived_at`

Create workspace accepts only operator fields: `name`, `bucket`, `workspace_prefix`, `region`, `endpoint`, optional `id`. The service generates paths from `BEEHIVE_WORKSPACES_ROOT`, defaulting to `/tmp/beehive-web-workspaces`.

Create initializes:

- workspace directory;
- `pipeline.yaml` with S3 storage and no stages;
- `app.db` via `database::bootstrap_database`;
- `workspaces.yaml` through temp-write, backup-old, rename-temp.

Update allows `name`, `endpoint`, `region`. Bucket/prefix changes are allowed only if the workspace has no stages, entity files, or stage runs.

Delete never removes S3 objects. Empty workspaces can be removed from the registry. Workspaces with local history are archived.

## 5. Stage CRUD Design

Extend `services/pipeline.rs`.

New service functions:

- `update_s3_stage_for_workspace`
- `archive_or_delete_stage_for_workspace`
- `restore_stage_for_workspace`

Create keeps the existing B6/B7 endpoint but remains operator-simple: `stage_id`, production webhook URL, optional `next_stage`, retry settings, and `allow_empty_outputs`. Generated S3 `input_uri` and `save_path_aliases` remain system-owned.

Update allows only:

- `workflow_url`
- `max_attempts`
- `retry_delay_sec`
- `allow_empty_outputs`
- `next_stage`

Delete blocks if another active stage points to the target. If the stage has no runtime history, remove it from `pipeline.yaml`; otherwise archive/deactivate it through config removal plus database sync, preserving SQLite history. Restore re-adds an archived stage when no active duplicate exists.

## 6. HTTP API Design

Add/extend routes:

- `GET /api/workspaces?include_archived=true|false`
- `POST /api/workspaces`
- `PATCH /api/workspaces/{workspace_id}`
- `DELETE /api/workspaces/{workspace_id}`
- `POST /api/workspaces/{workspace_id}/restore`
- `PATCH /api/workspaces/{workspace_id}/stages/{stage_id}`
- `DELETE /api/workspaces/{workspace_id}/stages/{stage_id}`
- `POST /api/workspaces/{workspace_id}/stages/{stage_id}/restore`

Keep B7 selected route unchanged:

- `POST /api/workspaces/{workspace_id}/run-selected-pipeline-waves`

## 7. Frontend Design

`/workspaces` becomes a CRUD management page with:

- Show archived toggle;
- Create workspace form;
- Edit workspace form;
- Archive/Delete;
- Restore;
- Select workspace.

Cards show only name, id, bucket, workspace prefix, stage count, and active/archived status.

`/workspaces/{id}/workspace` becomes operator-first:

- workspace name and S3 location;
- stage count;
- pending/failed/done;
- primary actions: Reconcile S3, Run selected pipeline waves, Add stage;
- detailed counters and raw reconciliation output inside Diagnostics.

Workspace Explorer table keeps action-relevant columns visible and hides checksums, etags, raw metadata, and producer output details until expansion.

`/workspaces/{id}/stages` becomes a CRUD page:

- Add stage;
- Edit stage;
- Connect stages;
- Archive/Delete;
- Restore;
- Copy save_path aliases;
- YAML preview and validation in Advanced.

## 8. Smoke Design

Add a new HTTP-only CRUD smoke helper, likely `scripts/web_operator_crud_smoke.mjs`.

It will require a running local `beehive-server` configured with temporary:

- `BEEHIVE_WORKSPACES_CONFIG`
- `BEEHIVE_WORKSPACES_ROOT`

It will verify:

- `POST /api/workspaces`
- `PATCH /api/workspaces/{id}`
- `POST /api/workspaces/{id}/stages`
- `PATCH /api/workspaces/{id}/stages/{stage_id}`
- `POST /api/workspaces/{id}/stages/{stage_id}/next-stage`
- `DELETE /api/workspaces/{id}/stages/{stage_id}`
- `DELETE /api/workspaces/{id}`

It must not touch production workspaces or S3.

## 9. Tests

Rust tests:

- workspace create initializes registry/files;
- duplicate workspace id rejected;
- old registry entries without metadata load;
- dangerous bucket/prefix update rejected when history exists;
- delete archives workspace with history;
- restore workspace works;
- stage update changes workflow/retry settings;
- stage delete blocks inbound `next_stage`;
- stage delete archives stage with history;
- stage hard delete works for empty unlinked stage;
- stage restore works without active duplicate;
- HTTP routes parse CRUD requests;
- B7 selected route still parses.

Frontend/build checks:

- `npm run build`
- `VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build`
- direct Tauri import boundary check.

Existing checks:

- `cargo fmt --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `python3 scripts/lint_n8n_workflows.py docs/n8n_workflows`
- `git diff --check`

## 10. Docs

Create:

- `docs/beehive_s3_b8_crud_ui_feedback.md`
- `docs/operator_crud_runbook.md`

Update if needed:

- `docs/front_back_split.md`
- `docs/web_operator_mvp_runbook.md`

## 11. Risks And Rollback

Risks:

- Registry write bugs can affect workspace discovery; keep atomic writes and backups.
- Stage deletion must preserve runtime history; do not delete SQLite history or S3 objects.
- UI simplification can hide useful diagnostics; keep diagnostics reachable.
- Existing user worktree changes exist; avoid touching unrelated files.

Rollback:

- Registry and pipeline writes create backups.
- B8 keeps broad B6/B7 routes and selected-run route intact.

## 12. Checkpoints

Reread `instructions/codex_b8_operator_crud_ui_simplification_instruction.md` at:

- after_plan
- after_workspace_crud_design
- after_stage_crud_design
- after_backend_crud
- after_ui_simplification
- after_tests
- after_smoke
- before_feedback
