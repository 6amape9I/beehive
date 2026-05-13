# Beehive S3 B2.1 Real Smoke Plan

## 1. Bucket and Prefix

Use the non-secret `.env` configuration already present in the repo:

```text
S3_HOST=present
S3_REGION=present
S3_KEY=present
S3_SEC_KEY=present
S3_BUCKET_NAME=steos-s3-data
BEEHIVE_SMOKE_PREFIX=beehive-smoke/test_workflow
BEEHIVE_N8N_SMOKE_WEBHOOK=present
```

The real S3 source prefix is:

```text
s3://steos-s3-data/beehive-smoke/test_workflow/raw/
```

The expected output prefix is:

```text
s3://steos-s3-data/beehive-smoke/test_workflow/processed/
```

## 2. Dataset Preparation and Upload

Preferred preparation now applies because `selected_50_for_n8n.zip` is present in the repo root.

Commands:

```bash
cd beehive_s3_smoke_kit
set -a
. ../.env
set +a
python3 prepare_selected50_s3_smoke.py \
  --zip ../selected_50_for_n8n.zip \
  --out s3_smoke_dataset \
  --prefix "${BEEHIVE_SMOKE_PREFIX:-beehive-smoke/test_workflow}" \
  --limit 50
cd s3_smoke_dataset
./upload_selected50_to_s3.sh
```

Upload verification:

```bash
aws s3 ls --endpoint-url "https://${S3_HOST}" \
  "s3://${S3_BUCKET_NAME}/${BEEHIVE_SMOKE_PREFIX}/raw/"
```

Expected result: 50 JSON objects.

## 3. n8n Webhook

Use the URL from `.env`:

```text
BEEHIVE_N8N_SMOKE_WEBHOOK=present
```

The workflow must already be imported in n8n and have S3 credentials attached to both the source download and output upload nodes. The URL is a `webhook-test` endpoint, so a blocker is possible if n8n requires a manual test-listening state.

## 4. Workdir and Pipeline

Use:

```text
/tmp/beehive_s3_smoke_workdir
```

Create `pipeline.yaml` from `beehive_s3_smoke_kit/pipeline.s3_smoke.example.yaml`, replacing the placeholder workflow URL with the `.env` webhook URL.

The workdir stays outside the repo and must not be committed.

## 5. Beehive Execution Path

Use reconciliation first:

1. Bootstrap the workdir database with the smoke pipeline.
2. Run `reconcile_s3_workspace`.
3. Confirm 50 pending `smoke_source` artifacts if S3 metadata upload succeeded.
4. Run `run_due_tasks` with `max_parallel_tasks=1`.
5. Query SQLite for source done, successful stage_run, and child pending pointer.
6. List S3 `processed/` for output existence.

Because the Tauri command surface is not directly callable from shell, add an ignored Rust real-smoke test/helper if no existing CLI command can execute this flow.

Suggested command:

```bash
cargo test --manifest-path src-tauri/Cargo.toml real_s3_n8n_smoke_one_artifact -- --ignored --nocapture
```

## 6. Evidence for Success

Required evidence:

```text
source artifact key
run_id
stage_run success
output artifact key
source done
child pending
S3 output exists
```

SQLite checks:

```bash
sqlite3 /tmp/beehive_s3_smoke_workdir/app.db \
  "select entity_id, stage_id, status, file_exists from entity_stage_states order by updated_at desc limit 20;"
sqlite3 /tmp/beehive_s3_smoke_workdir/app.db \
  "select run_id, entity_id, stage_id, success, http_status, error_type from stage_runs order by id desc limit 10;"
sqlite3 /tmp/beehive_s3_smoke_workdir/app.db \
  "select entity_id, artifact_id, stage_id, storage_provider, bucket, object_key, producer_run_id from entity_files order by id desc limit 20;"
```

## 7. Expected Blockers

Allowed blockers at this point:

- n8n workflow is not imported or webhook-test is not listening;
- n8n S3 credentials are not configured;
- S3 upload fails;
- Beehive command/helper cannot run in the current environment;
- manifest route is blocked;
- n8n upload node requires manual setup.

`selected_50_for_n8n.zip`, source object availability, and smoke pipeline availability are no longer accepted blockers unless the files are removed during the run.
