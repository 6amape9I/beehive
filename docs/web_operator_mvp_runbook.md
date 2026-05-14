# Web Operator MVP Runbook

## Start The Server

Default local-only server:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
```

Defaults:

```text
host = 127.0.0.1
port = 8787
registry = config/workspaces.yaml
```

Override host/port:

```bash
BEEHIVE_SERVER_HOST=127.0.0.1 \
BEEHIVE_SERVER_PORT=8787 \
cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
```

Use another registry:

```bash
BEEHIVE_WORKSPACES_CONFIG=/absolute/path/workspaces.yaml \
cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
```

Optional B7 pilot hardening:

```bash
BEEHIVE_SERVER_MAX_BODY_BYTES=1048576
BEEHIVE_ALLOWED_ORIGIN=http://127.0.0.1:8787,http://localhost:8787,http://127.0.0.1:5173,http://localhost:5173
```

`BEEHIVE_SERVER_MAX_BODY_BYTES` defaults to `1048576` and oversized HTTP requests return `413 Payload Too Large`. `BEEHIVE_ALLOWED_ORIGIN` is an allow-list. Local defaults are used when it is unset.

## Non-Local Bind Protection

The server refuses non-local bind unless both are set:

```bash
BEEHIVE_SERVER_ALLOW_NON_LOCAL=1
BEEHIVE_OPERATOR_TOKEN=...
```

When `BEEHIVE_OPERATOR_TOKEN` is set, requests must include:

```text
Authorization: Bearer <token>
```

Do not expose this MVP server on a public network without a token.

The browser HTTP client can send the same token with either:

```bash
VITE_BEEHIVE_OPERATOR_TOKEN=...
```

or `localStorage.BEEHIVE_OPERATOR_TOKEN` in the browser.

## API Smoke

```bash
curl -sS http://127.0.0.1:8787/api/health
curl -sS http://127.0.0.1:8787/api/workspaces
```

Expected health response:

```json
{"status":"ok"}
```

Scripted B7 web-operator smoke:

```bash
BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 \
BEEHIVE_SMOKE_WORKSPACE_ID=<workspace_id> \
node scripts/web_operator_smoke.mjs
```

This checks health, workspace registry access, Workspace Explorer, and the selected-run validation envelope. It does not replace a real selected pilot run against S3+n8n.

## Browser UI

If `dist/` exists, `beehive-server` serves the built frontend:

```text
http://127.0.0.1:8787/
```

For Vite development mode:

```bash
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run dev
```

Then open the Vite URL and use:

```text
/workspaces
/workspaces/{workspace_id}/workspace
/workspaces/{workspace_id}/stages
```

## Operator Flow

1. Open `/workspaces`.
2. Select a registered workspace.
3. Inspect Workspace Explorer.
4. Run `Reconcile S3`.
5. Register a source manually if metadata is absent.
6. Select 1-10 eligible S3 source rows.
7. Run `Run selected pipeline waves`.
8. Inspect the selected-run summary, root statuses, child outputs, and output tree.
9. Use broad `Run small batch` or `Run pipeline waves` only for admin-style due-queue operation.
10. Open Stage Editor for the workspace.
11. Create an S3 stage from `stage_id` and n8n webhook URL.
12. Copy generated save_path aliases to the n8n operator.
13. Connect stages with source/target dropdowns.
14. Select an output artifact with `producer_run_id` and load all run outputs.

For B7 pilot work, the recommended action is `Run selected pipeline waves`. It runs only the approved roots and descendants created by those selected runs. Broad queue actions can claim unrelated pending artifacts.

## Selected Pipeline Waves

HTTP endpoint:

```text
POST /api/workspaces/{workspace_id}/run-selected-pipeline-waves
```

Request shape:

```json
{
  "root_entity_file_ids": [101, 102, 103],
  "max_waves": 5,
  "max_tasks_per_wave": 3,
  "stop_on_first_failure": true
}
```

The B7 runner validates selected roots, executes exact `entity_file_id` rows first, follows only children with matching `producer_run_id`, records normal `stage_runs`, and never falls back to the global pending queue.

## Workspace Registry

The browser sends only `workspace_id`; server-side registry resolves workdir, pipeline, and database paths. Do not put S3 secrets in the registry.
