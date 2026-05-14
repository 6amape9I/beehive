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

## API Smoke

```bash
curl -sS http://127.0.0.1:8787/api/health
curl -sS http://127.0.0.1:8787/api/workspaces
```

Expected health response:

```json
{"status":"ok"}
```

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
6. Run a small batch or pipeline waves.
7. Open Stage Editor for the workspace.
8. Create an S3 stage from `stage_id` and n8n webhook URL.
9. Copy generated save_path aliases to the n8n operator.
10. Connect stages with source/target dropdowns.
11. Select an output artifact with `producer_run_id` and load all run outputs.

## Workspace Registry

The browser sends only `workspace_id`; server-side registry resolves workdir, pipeline, and database paths. Do not put S3 secrets in the registry.
