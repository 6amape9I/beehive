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

Each success output must include:

- `artifact_id`: non-empty physical artifact/run output id, unique within the manifest;
- `entity_id`: non-empty logical Beehive entity id;
- `relation_to_source`: one of `same_entity`, `child_entity`, `representation_of`, `candidate_parent`, `relation_artifact`, `other`;
- `bucket`, `key`, `save_path`, `content_type`, and optional checksum/size metadata.

For `same_entity`, `entity_id` must equal the source logical entity id claimed by Beehive. For child or representation outputs, `entity_id` may differ but must still be explicit. Beehive does not infer `entity_id` from `artifact_id`.

## save_path Routing

`save_path` maps an output to a configured Beehive stage route. It is a logical S3 route, not a local path.

Accepted forms:

```text
main_dir/processed/raw_entities
/main_dir/processed/raw_entities
s3://steos-s3-data/main_dir/processed/raw_entities
```

Beehive blocks the run if `save_path` is unsafe, unknown, ambiguous, points to the wrong bucket, or if the output key is outside the resolved stage prefix.

## Empty Outputs

`allow_empty_outputs` is a Beehive stage config flag. It defaults to `false`.

A success manifest may have zero outputs only when the source stage explicitly sets `allow_empty_outputs: true`. This is independent from `next_stage`; a terminal-looking S3 stage still rejects zero-output success unless this flag is set.

## Registration Conflicts

Beehive registers all outputs from one success manifest in one SQLite transaction.

- Same `producer_run_id + artifact_id + bucket/key`: idempotent replay.
- Same `producer_run_id + artifact_id` with different bucket/key: registration conflict.
- Same bucket/key with a different `entity_id`, `artifact_id`, or producer run: registration conflict.
- Duplicate `artifact_id` values inside one manifest: invalid manifest.
