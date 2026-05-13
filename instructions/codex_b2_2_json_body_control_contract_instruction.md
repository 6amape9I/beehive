# B2.2. JSON Control Envelope for S3/n8n Smoke

## 0. Why this stage exists

The current S3 n8n trigger contract sends many values through HTTP headers. This breaks or complicates real smoke tests when `source_key` contains Cyrillic filenames such as:

```text
beehive-smoke/test_workflow/raw/smoke_entity_001__порфирия.json
```

The production dataset will contain about 22,000 S3 objects with Russian names. Do not fight HTTP header encoding for S3 object keys.

The strategic rule remains:

```text
Beehive must not send business JSON to n8n.
```

But Beehive may and should send a small technical JSON control envelope.

## 1. Required contract decision

Change S3-mode n8n trigger from:

```text
POST with empty body + X-Beehive-* headers
```

to:

```text
POST with application/json body containing a technical control envelope
```

The body is not business JSON. It is only a pointer/control message.

## 2. Required control envelope

Beehive must send this shape in S3 mode:

```json
{
  "schema": "beehive.s3_control_envelope.v1",
  "workspace_id": "beehive-s3-smoke",
  "run_id": "run_uuid",
  "stage_id": "smoke_source",
  "source_bucket": "steos-s3-data",
  "source_key": "beehive-smoke/test_workflow/raw/smoke_entity_001__порфирия.json",
  "source_version_id": null,
  "source_etag": null,
  "source_entity_id": "smoke_entity_001",
  "source_artifact_id": "smoke_artifact_001",
  "manifest_prefix": "beehive-smoke/test_workflow/runs/run_uuid/",
  "workspace_prefix": "beehive-smoke/test_workflow",
  "target_prefix": "beehive-smoke/test_workflow/processed",
  "save_path": "beehive-smoke/test_workflow/processed"
}
```

Required fields:

```text
schema
workspace_id
run_id
stage_id
source_bucket
source_key
source_entity_id
source_artifact_id
manifest_prefix
workspace_prefix
target_prefix
save_path
```

Optional nullable fields:

```text
source_version_id
source_etag
```

Do not include business payload/body text/content blocks/source JSON.

## 3. Backend changes

In `src-tauri/src/executor/mod.rs` S3 branch:

1. Replace `call_s3_webhook` empty-body/header request with JSON body request.
2. Use `Content-Type: application/json` and `Accept: application/json`.
3. Keep `stage_runs.request_json` as the same control envelope that was sent.
4. Include `source_entity_id` and `source_artifact_id` from the source `EntityFileRecord`.
5. Do not include `payload_json` or business JSON.
6. Add backward compatibility only if it is small: n8n docs may mention old header mode as deprecated, but new tests must assert JSON body mode.

Recommended function name:

```rust
call_s3_control_webhook(workflow_url, &control_envelope, timeout_sec)
```

## 4. Tests required

Add/update Rust tests:

1. S3 mode sends `application/json` request body.
2. Captured request body contains `source_key` with Cyrillic characters unchanged.
3. Captured request body contains `source_entity_id` and `source_artifact_id`.
4. Captured request body does not contain business text from `payload_json`.
5. n8n mock still returns valid manifest and Beehive registers output pointer.
6. Old local mode still sends payload-only business JSON and is unchanged.

Use a mock HTTP server. Do not call real n8n in unit tests.

## 5. n8n workflow requirement

Use the uploaded/importable workflow:

```text
Beehive_S3_Pointer_Smoke_Adapter_BODY_JSON.json
```

It should use n8n nodes instead of large Code nodes:

```text
Webhook
-> Edit Fields / Read control JSON body
-> S3 Download source object
-> Extract JSON
-> Edit Fields / Build output document
-> Convert to File
-> S3 Upload smoke output
-> Edit Fields / Build success manifest
-> Respond to Webhook
```

No `Search Bucket` or `List Bucket` node is allowed in production path.

## 6. Docs to update

Update:

```text
docs/s3_n8n_contract.md
docs/n8n_s3_pointer_workflow_adapter.md
docs/s3_control_plane_architecture.md
```

They must say:

```text
B2.2 uses JSON control envelope body.
Headers are deprecated for S3 object keys and should not be used for source_key.
The control envelope is technical metadata, not business JSON.
```

## 7. Real smoke checklist

After backend change and n8n import:

1. Upload source objects with Russian filenames to S3.
2. Register/reconcile source artifacts in Beehive.
3. Run due tasks for one S3 artifact.
4. Confirm n8n receives JSON body with Cyrillic `source_key`.
5. Confirm n8n downloads exactly that object.
6. Confirm n8n uploads output JSON under `target_prefix`.
7. Confirm Beehive receives manifest and source becomes `done`.
8. Confirm child output pointer appears as `pending` in target stage.

## 8. Feedback required

Create:

```text
docs/beehive_s3_b2_2_feedback.md
```

Include:

```text
1. What changed in the contract.
2. Why JSON body replaced headers.
3. Files changed.
4. Tests added/updated.
5. Commands run and exact results.
6. Whether a Cyrillic source_key mock test passed.
7. Whether real n8n/S3 smoke was run.
8. If real smoke was not run, exact blocker.
9. Remaining risks.
10. Next step.
```

Required checkpoint line:

```text
ТЗ перечитано на этапах: after_plan, after_backend_contract_change, after_n8n_workflow_update, after_tests, before_feedback
```
