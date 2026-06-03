# V1 Patch: Entity S3 View and Reset Actions Plan

## Scope

This patch stays inside Beehive V1. It adds two operator actions for workspace-mode Entity Detail:

- view the stored S3 JSON for an entity file/artifact through the backend;
- reset failed, blocked, or retry-wait entity stage state to pending with attempts reset to 0.

No V2 architecture, RabbitMQ, Postgres, bulk reset, S3 deletion, or browser-side S3 credential handling is included.

## Backend Design

### View S3 JSON

Route:

```text
GET /api/workspaces/{workspace_id}/entity-files/{entity_file_id}/s3-json
```

Flow:

1. Decode path params using the existing HTTP API URL decoder.
2. Resolve the workspace via the registry.
3. Load the entity file by `entity_file_id` from that workspace database.
4. Reject non-S3 records with `not_s3_artifact`.
5. Read `bucket` and `key` from the stored entity file record only.
6. Fetch object bytes via `AwsS3MetadataClient`.
7. Parse bytes as UTF-8 JSON in the service layer.
8. Return `s3_uri`, `bucket`, `key`, and structured JSON.

Expected clear errors:

- `entity_file_not_found`
- `not_s3_artifact`
- `s3_object_not_found`
- `s3_read_failed`
- `s3_json_invalid`

### Reset to Pending

Route:

```text
POST /api/workspaces/{workspace_id}/entities/{entity_id}/stages/{stage_id}/reset-to-pending
```

Request body:

```json
{
  "confirm": true,
  "reason": "manual retest after fixing n8n workflow"
}
```

Flow:

1. Decode workspace, entity, and stage path params.
2. Require `confirm: true`.
3. Resolve workspace database.
4. Load entity-stage state.
5. Reject non-resettable states. Allowed: `failed`, `blocked`, `retry_wait`.
6. Reject active worker lease with `active_worker_lease_exists`.
7. Update the current state to `pending`, `attempts = 0`, `next_retry_at = null`, and clear error/run fields already cleared by the existing database reset helper.
8. Insert app event `entity_stage_state_manual_reset`.
9. Return updated Entity Detail payload.

History is preserved: stage_runs, app_events, existing S3 objects, and output artifacts are not deleted.

## Frontend Design

### File Instances

`EntityDetailPage` passes a workspace-mode S3 JSON action into `EntityFileInstances`.

Each S3 file row gets a `View S3 JSON` button. The button calls the backend route and opens a modal with:

- title `S3 JSON`;
- subtitle `s3://bucket/key`;
- `Copy JSON`;
- `Copy S3 URI`;
- `Close`;
- scrollable JSON preview.

Credentials are never shown. Rendering may cap large JSON previews.

### Manual Reset

Workspace-mode Entity Detail should show manual actions too, not only legacy `workdirPath` mode.

`Reset` opens a confirmation modal with optional reason. On confirm it calls the workspace HTTP reset endpoint, refreshes detail, and shows `State reset to pending.`

The button remains controlled by backend `allowed_actions`.

## Tests

Backend tests:

- non-S3 artifact is rejected for S3 JSON view;
- missing entity file returns `entity_file_not_found`;
- S3 reader is called with stored bucket/key;
- valid JSON returns structured payload;
- invalid JSON returns `s3_json_invalid`;
- Cyrillic JSON is returned correctly;
- `failed`, `blocked`, and `retry_wait` reset to `pending` with `attempts = 0`;
- `queued` and `in_progress` reset are rejected;
- active worker lease blocks reset;
- reset inserts app event and preserves stage_runs;
- Cyrillic entity_id route decodes and resets.

Frontend/build checks:

- S3 JSON action appears for S3 file instances;
- reset confirmation calls the workspace endpoint;
- API errors are surfaced through existing error panels.

## Commands

Run and report:

```text
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
git diff --check
```
