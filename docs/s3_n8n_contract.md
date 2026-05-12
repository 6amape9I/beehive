# S3 n8n Contract

## Core Rule

In S3 mode Beehive does not send business JSON to n8n. Beehive sends only a technical pointer to one claimed S3 artifact. n8n downloads that artifact from S3, writes outputs back to S3, and returns a technical manifest.

## Webhook Request

Beehive sends:

```text
POST workflow_url
Content-Type: application/octet-stream
Accept: application/json
```

The HTTP body is empty.

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

## n8n Responsibilities

n8n should:

1. Read the source bucket/key headers.
2. Download exactly that S3 object.
3. Parse and transform the business JSON.
4. Upload output business JSON artifacts to configured S3 prefixes.
5. Return a manifest with schema `beehive.s3_artifact_manifest.v1`.

n8n must not use `Search bucket` / S3 list nodes to select production inputs. Those nodes are acceptable for demos and debugging only.

## Manifest

Success:

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
      "bucket": "steos-s3-data",
      "key": "main_dir/processed/raw_entities/art_001.json",
      "save_path": "main_dir/processed/raw_entities",
      "content_type": "application/json",
      "checksum_sha256": null,
      "size": 12345
    }
  ],
  "created_at": "2026-05-12T00:00:00Z"
}
```

Error:

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
  "created_at": "2026-05-12T00:00:00Z"
}
```

The manifest must not contain business payload fields such as `payload`, `business_payload`, `business_json`, or `data`.

## save_path Routing

`save_path` maps an output to a configured Beehive stage route. It is a logical S3 route, not a local path.

Accepted forms:

```text
main_dir/processed/raw_entities
/main_dir/processed/raw_entities
s3://steos-s3-data/main_dir/processed/raw_entities
```

Beehive blocks the run if `save_path` is unsafe, unknown, ambiguous, points to the wrong bucket, or if the output key is outside the resolved stage prefix.

## Terminal Stages

A success manifest may have zero outputs only when the source stage is terminal/no-output. Non-terminal stages must return at least one output artifact.
