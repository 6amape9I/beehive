# S3 MVP Pipeline Pilot Runbook

## Goal

Run a small manual S3 pipeline pilot:

```text
smoke_source -> n8n stage A -> smoke_processed -> n8n stage B -> smoke_final
```

Beehive remains the control plane. n8n remains the data plane. S3 remains the artifact store.

## Setup

1. Keep S3 and n8n secrets in `.env` or shell variables, not in Git.

```text
S3_HOST=...
S3_REGION=...
S3_KEY=...
S3_SEC_KEY=...
S3_BUCKET_NAME=...
BEEHIVE_SMOKE_PREFIX=beehive-smoke/test_workflow
BEEHIVE_N8N_SMOKE_WEBHOOK=...
BEEHIVE_N8N_STAGE_B_WEBHOOK=...
```

`BEEHIVE_N8N_STAGE_B_WEBHOOK` is optional for smoke-compatible pilots; if omitted, the opt-in real pilot reuses `BEEHIVE_N8N_SMOKE_WEBHOOK`.

2. Configure `pipeline.yaml` with S3 storage and chained stages:

```yaml
storage:
  provider: s3
  bucket: steos-s3-data
  workspace_prefix: beehive-smoke/test_workflow
  region: ru-1
  endpoint: https://s3.ru-1.storage.selcloud.ru

stages:
  - id: smoke_source
    input_uri: s3://steos-s3-data/beehive-smoke/test_workflow/raw
    workflow_url: https://n8n.example/webhook/stage-a
    next_stage: smoke_processed

  - id: smoke_processed
    input_uri: s3://steos-s3-data/beehive-smoke/test_workflow/processed
    workflow_url: https://n8n.example/webhook/stage-b
    next_stage: smoke_final

  - id: smoke_final
    input_uri: s3://steos-s3-data/beehive-smoke/test_workflow/final
    workflow_url: http://localhost/not-used-terminal
    allow_empty_outputs: true
```

3. Open Beehive and go to Workspace Explorer.

4. Run `Reconcile S3`.

5. Confirm the source stage has pending S3 pointer rows.

6. Run `Run pipeline waves` with:

```text
max_waves=2
max_tasks_per_wave=3
stop_on_first_failure=true
```

7. Inspect aggregate counts and per-wave summaries.

8. Confirm lineage:

```text
smoke_source source rows -> done
smoke_processed child rows -> done after wave 2
smoke_final child rows -> pending
```

9. Inspect stage run audit:

```bash
sqlite3 /path/to/app.db \
  "select run_id, entity_id, stage_id, success, http_status, error_type from stage_runs order by id desc limit 20;"
```

10. Confirm final S3 outputs exist:

```bash
aws --endpoint-url "https://${S3_HOST}" \
  s3 ls "s3://${S3_BUCKET_NAME}/${BEEHIVE_SMOKE_PREFIX}/final/"
```

## Opt-in Test Command

```bash
BEEHIVE_REAL_S3_MVP_PIPELINE_PILOT=1 \
BEEHIVE_SMOKE_BATCH_LIMIT=3 \
cargo test --manifest-path src-tauri/Cargo.toml real_s3_n8n_mvp_pipeline_pilot -- --ignored --nocapture
```

## Troubleshooting

`failure_or_blocked`: inspect the latest `stage_runs.error_type` and `error_message`.

`manifest_invalid`: n8n returned a malformed manifest, wrong schema, missing output identity, or a workspace/source mismatch.

`manifest_blocked`: output `save_path` or key does not match a configured S3 stage route.

`idle after wave 1`: no pending source artifacts were registered; run S3 reconciliation or manual registration.

`stage B not reached`: stage A did not return outputs to the stage-B `input_uri` route, or `max_waves` was too low.

`old header workflow`: replace the workflow with a body-JSON envelope workflow and rerun preflight/lint.
