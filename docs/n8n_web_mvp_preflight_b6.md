# n8n Web MVP Preflight B6

Use this checklist before a web-operator pilot. This is a practical preflight, not a full n8n workflow validator.

## Webhook

- Webhook method is `POST`.
- Webhook response mode returns through a response node.
- Manifest is returned synchronously.

## Beehive Request Contract

- Workflow reads the JSON body control envelope.
- `source_bucket` comes from body.
- `source_key` comes from body.
- Workflow does not use `X-Beehive-Source-Key` or other source-key headers.
- Workflow does not reference old nodes such as `Read Beehive headers`.

## S3 Input Selection

- Workflow downloads exactly `body.source_bucket` / `body.source_key`.
- Workflow does not use Search Bucket/List Bucket as production source selection.

## Output Manifest

- Manifest schema is `beehive.s3_artifact_manifest.v1`.
- Success outputs include:
  - `artifact_id`
  - `entity_id`
  - `relation_to_source`
  - `bucket`
  - `key`
  - `save_path`
- Output `save_path` matches one of the Beehive stage save_path aliases.
- No output path uses `/main_dir/pocessed`.

## Repository Policy

Do not commit production workflow JSONs. Keep only tiny contract examples and runbook notes in the repository.
