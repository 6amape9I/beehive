# n8n Workflow Authoring Standard for Beehive S3

## Production Contract

Beehive S3 mode calls n8n with a JSON technical control envelope:

```text
POST workflow_url
Content-Type: application/json; charset=utf-8
Accept: application/json
body.schema = beehive.s3_control_envelope.v1
```

The request body is metadata only. It must not contain source business JSON, `payload_json`, article blocks, or raw document content. `X-Beehive-Source-Key` and other source-key headers are deprecated and must not be used by active production workflows.

## Required Workflow Behavior

1. S3 mode receives a JSON control envelope body.
2. n8n must download exactly `source_bucket` / `source_key` from the body.
3. n8n must not use Search Bucket/List Bucket as production source selection.
4. n8n must upload outputs before returning a manifest.
5. n8n must return `beehive.s3_artifact_manifest.v1` synchronously unless async mode is explicitly implemented later.
6. Output manifest entries require `artifact_id`, `entity_id`, `relation_to_source`, `bucket`, `key`, and `save_path`.
7. `save_path` must match an active Beehive S3 route/prefix.
8. Code nodes are discouraged and allowed only for justified operations that n8n-native nodes cannot reasonably perform.

## Fixture Policy

Active workflow examples live in `docs/n8n_workflows/` and must use the body-JSON envelope contract. Header-based examples are not production-safe; keep them out of active fixtures or rename them with `deprecated_header_mode` and document the limitation.

Do not commit full smoke datasets, generated output batches, credentials, or zip archives with workflow fixtures.

## Linting

Run:

```bash
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
```

The linter fails active fixtures that use deprecated source-key headers, Search/List Bucket source selection, legacy typo paths such as `/main_dir/pocessed`, local-looking absolute save paths, unjustified Code-node density, or Webhook nodes that are not configured for POST plus response-node execution.
