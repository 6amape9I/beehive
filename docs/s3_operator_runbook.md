# S3 Operator Runbook

## Sequence

1. Configure S3 environment variables in `.env` or the shell. Do not commit secrets.

```text
S3_HOST=...
S3_REGION=...
S3_KEY=...
S3_SEC_KEY=...
S3_BUCKET_NAME=...
BEEHIVE_N8N_SMOKE_WEBHOOK=...
```

2. Import or use a body-JSON n8n workflow. The active example is `docs/n8n_workflows/beehive_s3_pointer_smoke_body_json.json`.

3. Configure `pipeline.yaml` with S3 storage and S3 stage input URIs:

```yaml
storage:
  provider: s3
  bucket: steos-s3-data
  workspace_prefix: beehive-smoke/test_workflow
  region: ru-1
  endpoint: https://s3.ru-1.storage.selcloud.ru

stages:
  - id: smoke_source
    input_uri: s3://steos-s3-data/beehive-smoke/test_workflow/raw/
    workflow_url: https://n8n.example/webhook/beehive-s3-pointer-smoke
```

4. Open Beehive and go to Workspace Explorer.

5. Run `Reconcile S3`. Check `listed_object_count`, `metadata_tagged_count`, `registered_file_count`, `unmapped_object_count`, and `latest_reconciliation_at`.

6. If reconciliation cannot map an object because metadata is absent, use `Register S3 source` with `stage_id`, `entity_id`, `artifact_id`, `bucket`, and `key`. Optional fields are `version_id`, `etag`, `checksum_sha256`, and `size`.

7. Run a small batch from Workspace Explorer with a limit of 1-5 tasks.

8. Verify source state and child state in Workspace Explorer or Entity Detail:

```text
source stage -> done
processed/child stage -> pending
storage_provider -> s3
bucket/key -> output pointer
producer_run_id -> run that created the pointer
```

9. Inspect `stage_runs.request_json` and `stage_runs.response_json` when debugging:

```bash
sqlite3 /path/to/app.db \
  "select run_id, entity_id, stage_id, success, http_status, error_type, request_json, response_json from stage_runs order by id desc limit 5;"
```

10. Verify the output object exists in S3:

```bash
aws --endpoint-url "https://${S3_HOST}" \
  s3 ls "s3://${S3_BUCKET_NAME}/${BEEHIVE_SMOKE_PREFIX}/processed/"
```

11. Use Entity Detail actions to retry, reset to pending, or skip failed/blocked tasks.

## Troubleshooting

`HTTP 404 n8n webhook`: the workflow is not active/imported at that URL, or the production/test webhook URL does not match the imported workflow.

`manifest_invalid`: n8n returned non-JSON, the wrong schema, missing `run_id`, missing `outputs`, or an output without required fields.

`manifest_blocked` / save_path route mismatch: the manifest `save_path` or output key does not match an active Beehive S3 target route/prefix.

`missing artifact_id`: the S3 source was not reconciled with Beehive metadata and was not manually registered with an artifact ID.

`S3 credentials missing`: `.env` or shell env is missing `S3_HOST`, `S3_REGION`, `S3_KEY`, `S3_SEC_KEY`, or `S3_BUCKET_NAME`.

`S3 object unmapped`: reconciliation listed an object, but it lacks Beehive metadata and no manual registration exists.

`Cyrillic source_key problems`: verify the workflow reads `source_key` from the JSON body, not headers. Headers are deprecated and unsafe for real S3 keys.

`old header workflow accidentally used`: lint active workflow fixtures and import the body-JSON workflow instead:

```bash
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
```
