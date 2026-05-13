# n8n S3 Pointer Workflow Adapter

## Goal

Convert an n8n workflow that chooses S3 files itself into a Beehive-driven pointer workflow.

Production input selection must be:

```text
Beehive claim -> Webhook headers -> download exactly one S3 object
```

Production input selection must not be:

```text
Manual Trigger -> Search Bucket/List Bucket -> choose an object
```

Search Bucket/List Bucket nodes are acceptable only for debugging or one-off demos. They are not safe as the production source selector because they bypass Beehive runtime state, retries, and lineage.

## Request Contract

Beehive calls the configured `workflow_url` with an empty body and S3 pointer headers.

Required headers:

```text
X-Beehive-Workspace-Id
X-Beehive-Run-Id
X-Beehive-Stage-Id
X-Beehive-Source-Bucket
X-Beehive-Source-Key
X-Beehive-Manifest-Prefix
```

Optional headers:

```text
X-Beehive-Source-Version-Id
X-Beehive-Source-Etag
```

n8n must read `X-Beehive-Source-Bucket` and `X-Beehive-Source-Key`, download exactly that object, transform the business JSON, upload output JSON to S3, then return the synchronous B2 manifest in the webhook response.

## Adapter Shape

Replace:

```text
Manual Trigger
-> Search Bucket / List Bucket
-> Download file from selected result
-> Extract from File
-> Transform
```

With:

```text
Webhook
-> Read Beehive headers
-> Download file from X-Beehive-Source-Bucket + X-Beehive-Source-Key
-> Parse business JSON
-> Transform
-> Upload outputs under resolved save_path prefix
-> Respond with beehive.s3_artifact_manifest.v1
```

## Success Manifest

B2 expects a synchronous JSON manifest in the webhook response:

```json
{
  "schema": "beehive.s3_artifact_manifest.v1",
  "workspace_id": "beehive-s3-dev",
  "run_id": "run_123",
  "source": {
    "bucket": "steos-s3-data",
    "key": "main_dir/raw/input_001.json",
    "version_id": null,
    "etag": null
  },
  "status": "success",
  "outputs": [
    {
      "artifact_id": "art_001",
      "entity_id": "entity_001",
      "relation_to_source": "child_entity",
      "bucket": "steos-s3-data",
      "key": "main_dir/processed/raw_entities/art_001.json",
      "save_path": "main_dir/processed/raw_entities",
      "content_type": "application/json",
      "checksum_sha256": null,
      "size": 12345
    }
  ],
  "created_at": "2026-05-13T00:00:00Z"
}
```

Every output must include `artifact_id`, `entity_id`, `relation_to_source`, `bucket`, `key`, and `save_path`. Beehive validates that `save_path` resolves to an active S3 stage route and that `key` is under that route prefix.

## Error Manifest

Return an error manifest when n8n can identify a controlled business/runtime failure:

```json
{
  "schema": "beehive.s3_artifact_manifest.v1",
  "workspace_id": "beehive-s3-dev",
  "run_id": "run_123",
  "source": {
    "bucket": "steos-s3-data",
    "key": "main_dir/raw/input_001.json"
  },
  "status": "error",
  "error_type": "llm_invalid_json",
  "error_message": "Model returned invalid JSON",
  "outputs": [],
  "created_at": "2026-05-13T00:00:00Z"
}
```

For transport failures, an HTTP 5xx/4xx response is also valid; Beehive records the failed attempt and retry state.

## Source Registration

Beehive can discover source S3 objects by metadata or manual registration.

Supported S3 user metadata keys:

```text
x-amz-meta-beehive-entity-id
x-amz-meta-beehive-artifact-id
x-amz-meta-beehive-stage-id
x-amz-meta-beehive-source-artifact-id
```

Objects without Beehive identity metadata are recorded as unmapped during reconciliation and are not made runnable automatically.

Manual source registration can register one known source object without reading its body:

```text
stage_id, entity_id, artifact_id, bucket, key, version_id?, etag?, checksum_sha256?, size?
```

The key must be under the stage `input_uri` prefix.

## Preflight Checklist

- `pipeline.yaml` uses `storage.provider: s3` with bucket, workspace prefix, and active S3 stage `input_uri` values.
- Beehive runtime can list/head S3 metadata using AWS env vars or local aliases `S3_HOST`, `S3_REGION`, `S3_KEY`, `S3_SEC_KEY`.
- Source object is either tagged with Beehive metadata or registered manually.
- n8n webhook reads headers, not a search/list result.
- n8n downloads exactly the bucket/key from Beehive headers.
- n8n uploads outputs under the intended `save_path` prefix.
- n8n response manifest uses the same `workspace_id`, `run_id`, and source bucket/key that Beehive sent.
- `outputs` include `artifact_id`, `entity_id`, `relation_to_source`, `bucket`, `key`, and `save_path`.
- The workflow has been checked with a single artifact before enabling repeated runs.
