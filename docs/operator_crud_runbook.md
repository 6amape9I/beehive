# Operator CRUD Runbook

## Server Setup

Start the web server with a registry file and a server-side workspace root:

```bash
BEEHIVE_WORKSPACES_CONFIG=/tmp/beehive-crud/workspaces.yaml \
BEEHIVE_WORKSPACES_ROOT=/tmp/beehive-crud/workspaces \
BEEHIVE_SERVER_PORT=8787 \
cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
```

`BEEHIVE_WORKSPACES_ROOT` defaults to:

```text
/tmp/beehive-web-workspaces
```

Workspace create uses that root to generate:

```text
workdir_path  = {BEEHIVE_WORKSPACES_ROOT}/{workspace_id}
pipeline_path = {workdir_path}/pipeline.yaml
database_path = {workdir_path}/app.db
```

Operators never type those paths in the browser.

## Workspace CRUD

Open:

```text
/workspaces
```

Available actions:

- Create workspace from name, bucket, workspace prefix, region, endpoint, and optional id.
- Select workspace.
- Edit name, region, endpoint.
- Edit bucket/prefix only while the workspace has no stages, artifacts, or run history.
- Archive/Delete workspace.
- Restore archived workspace.
- Toggle Show archived.

S3 objects are never deleted by workspace archive/delete.

HTTP routes:

```text
GET    /api/workspaces?include_archived=true|false
POST   /api/workspaces
PATCH  /api/workspaces/{workspace_id}
DELETE /api/workspaces/{workspace_id}
POST   /api/workspaces/{workspace_id}/restore
```

## Stage CRUD

Open:

```text
/workspaces/{workspace_id}/stages
```

Available actions:

- Add stage from stage id, production n8n webhook URL, optional next stage, retry settings, and empty-output flag.
- Edit workflow URL, retry settings, empty-output flag, and next stage.
- Connect stages.
- Archive/Delete stage.
- Restore archived stage.
- Copy generated save_path aliases.

Operators do not edit:

```text
input_uri
input_folder
output_folder
save_path_aliases
server paths
```

Beehive generates stage routes from bucket, workspace prefix, and stage id.

HTTP routes:

```text
POST   /api/workspaces/{workspace_id}/stages
PATCH  /api/workspaces/{workspace_id}/stages/{stage_id}
DELETE /api/workspaces/{workspace_id}/stages/{stage_id}
POST   /api/workspaces/{workspace_id}/stages/{stage_id}/restore
POST   /api/workspaces/{workspace_id}/stages/{stage_id}/next-stage
```

If another stage points to a target through `next_stage`, delete/archive is blocked until the link is cleared.

## Operator Run

Open:

```text
/workspaces/{workspace_id}/workspace
```

Default view shows:

- workspace/stage status;
- pending, failed, blocked, done counts;
- Reconcile S3;
- Run selected pipeline waves;
- Add stage;
- source artifact selection.

Diagnostics and Advanced sections contain raw counters, manual S3 source registration, broad queue actions, YAML preview, and validation details.

## CRUD Smoke

Use a temporary registry and workspace root. Do not point this smoke at a production registry.

```bash
BEEHIVE_WORKSPACES_CONFIG=/tmp/beehive-crud-smoke/workspaces.yaml \
BEEHIVE_WORKSPACES_ROOT=/tmp/beehive-crud-smoke/workspaces \
BEEHIVE_SERVER_PORT=8787 \
cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
```

Then:

```bash
BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 \
node scripts/web_operator_crud_smoke.mjs
```

The smoke checks workspace create/update/delete and stage create/update/link/delete through HTTP only.
