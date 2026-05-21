# n8n S3 Manifest Contract

## Request Envelope

Beehive calls each S3 stage webhook with a JSON control envelope. The body points to one claimed source artifact; it is not the business document.

Required n8n behavior:

1. Read `source_bucket` and literal `source_key` from the request body.
2. Download exactly that S3 object.
3. Write output JSON artifacts to S3.
4. Return one manifest object with schema `beehive.s3_artifact_manifest.v1`.

Do not URL-encode `source.key` in the response manifest. It must be the same literal key Beehive sent in `source_key`, including Cyrillic characters.

## Success Manifest

```json
{
  "schema": "beehive.s3_artifact_manifest.v1",
  "workspace_id": "workspace-id",
  "run_id": "run-id",
  "source": {
    "bucket": "steos-s3-data",
    "key": "workspace/stages/source/entity.json",
    "version_id": null,
    "etag": null
  },
  "status": "success",
  "outputs": [
    {
      "artifact_id": "artifact-001",
      "entity_id": "entity-001",
      "relation_to_source": "child_entity",
      "bucket": "steos-s3-data",
      "key": "workspace/stages/target/artifact-001.json",
      "save_path": "workspace/stages/target",
      "content_type": "application/json",
      "checksum_sha256": null,
      "size": 123
    }
  ],
  "created_at": "2026-05-19T00:00:00Z"
}
```

Top-level fields:

- `schema`: must be `beehive.s3_artifact_manifest.v1`.
- `workspace_id`: workspace id from the Beehive request.
- `run_id`: run id from the Beehive request.
- `source`: source artifact pointer that must match the claimed source.
- `status`: `success` or `error`.
- `outputs`: array of produced S3 artifacts. It may be empty only when the source stage allows zero outputs.
- `created_at`: ISO timestamp from n8n.

Output fields:

- `artifact_id`: non-empty output artifact id, unique inside the manifest.
- `entity_id`: non-empty logical Beehive entity id.
- `relation_to_source`: `same_entity`, `child_entity`, `representation_of`, `candidate_parent`, `relation_artifact`, or `other`.
- `bucket`: S3 bucket containing the output object.
- `key`: literal S3 object key for the output JSON.
- `save_path`: Beehive route alias for the target stage.
- `content_type`: expected to be `application/json`.
- `checksum_sha256`: optional checksum.
- `size`: optional object size in bytes.

## Error Manifest

```json
{
  "schema": "beehive.s3_artifact_manifest.v1",
  "workspace_id": "workspace-id",
  "run_id": "run-id",
  "source": {
    "bucket": "steos-s3-data",
    "key": "workspace/stages/source/entity.json"
  },
  "status": "error",
  "error_type": "llm_invalid_json",
  "error_message": "Model returned invalid JSON",
  "outputs": [],
  "created_at": "2026-05-19T00:00:00Z"
}
```

`error_type` and `error_message` are required for `status = error`. Error manifests do not register outputs and are handled by retry/failure policy.

## Cardinality

Stage output cardinality is explicit:

- default: exactly one output;
- `allow_zero_outputs = true`: zero outputs are allowed;
- `allow_multiple_outputs = true`: more than one output is allowed;
- both flags together allow zero, one, or many outputs.

Legacy `allow_empty_outputs = true` is accepted as a deprecated alias for `allow_zero_outputs = true`.

Cardinality violations are `manifest_blocked`, not retryable failures. The webhook returned a structurally valid manifest, but it violated the stage contract.

## Strict Shape

Beehive rejects these response shapes:

- root array;
- root string, number, boolean, or null;
- manifest wrapped in `body`;
- top-level or per-output business fields: `body`, `payload`, `business_payload`, `business_json`, `data`;
- duplicate `artifact_id` values inside one manifest;
- source bucket/key mismatch;
- URL-encoded source key instead of the literal key.

The original n8n response manifest is preserved in the stage run response body. Output registration details are recorded separately as app event `output_registration_report`.

## Partial Registration

Beehive registers valid output siblings even when another output from the same manifest has a registration conflict.

Registration statuses in `output_registration_report`:

- `registered_count`, `skipped_count`, `invalid_count`, `conflict_count`, `failed_count`: summary counters;
- `registered`: output pointer was inserted;
- `idempotent_skipped`: the same output pointer already existed for the same producer run;
- `invalid`: output could not be registered because required identity or target stage data was invalid;
- `conflict`: output collided with an existing artifact/entity/stage/key relationship;
- `failed`: unexpected registration failure.

If at least one output is registered or idempotently skipped, the source run succeeds and the report is stored for diagnostics. If every output is invalid or conflicting, the run is blocked with `manifest_blocked` and is not retried.
