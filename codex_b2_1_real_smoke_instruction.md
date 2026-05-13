# B2.1 Real Smoke Kit and Smoke Execution Instruction for Codex Agent

Deprecated historical instruction.

B2.1 used an empty-body plus `X-Beehive-*` header smoke workflow. That workflow has been removed from the current smoke kit. B2.2 and later use the JSON body control envelope contract documented in `README.md`, `docs/s3_n8n_contract.md`, and `beehive_s3_smoke_kit/s3_real_smoke_miniguide.md`.

## 0. Mission

Continue Beehive S3+n8n integration after B2 foundation. Do not rebuild B2 foundation.

Main output:

```text
One concrete S3 source artifact is processed by n8n and tracked by Beehive as source done plus child S3 pointer.
```

Stretch output:

```text
50 selected source artifacts are uploaded/registered and a small controlled batch is run.
```

## 1. Inputs

Read `.env` without printing secrets. Expected keys:

```text
S3_HOST=s3.ru-1.storage.selcloud.ru
S3_REGION=ru-1
S3_KEY=***
S3_SEC_KEY=***
S3_BUCKET_NAME=steos-s3-data
BEEHIVE_SMOKE_PREFIX=beehive-smoke/test_workflow
BEEHIVE_N8N_SMOKE_WEBHOOK=https://n8n-dev.steos.io/webhook-test/beehive-s3-pointer-smoke
```

Root files expected:

```text
selected_50_for_n8n.zip
beehive_s3_smoke_kit.zip
```

If `selected_50_for_n8n.zip` is absent but `beehive_s3_smoke_kit.zip` already contains `s3_smoke_dataset`, use the prepared dataset and record the deviation.

## 2. Read First

Read:

```text
docs/beehive_s3_b2_feedback.md
docs/n8n_s3_pointer_workflow_adapter.md
docs/s3_n8n_contract.md
src-tauri/src/s3_reconciliation.rs
src-tauri/src/s3_client.rs
src-tauri/src/executor/mod.rs
```

Also read the smoke kit:

```text
s3_real_smoke_miniguide.md
pipeline.s3_smoke.example.yaml
prepare_selected50_s3_smoke.py
n8n_beehive_s3_pointer_smoke_workflow.json
```

## 3. Required Plan Before Run

Create:

```text
docs/beehive_s3_b2_1_real_smoke_plan.md
```

The plan must include bucket/prefix, dataset preparation/upload, webhook URL, workdir path, reconcile/manual registration strategy, exact commands, success evidence, and expected blockers.

## 4. Prepare 50 Source Objects

Preferred command:

```bash
python3 prepare_selected50_s3_smoke.py \
  --zip selected_50_for_n8n.zip \
  --out s3_smoke_dataset \
  --prefix "${BEEHIVE_SMOKE_PREFIX:-beehive-smoke/test_workflow}" \
  --limit 50
```

Then upload:

```bash
cd s3_smoke_dataset
./upload_selected50_to_s3.sh
```

Verify:

```bash
aws s3 ls --endpoint-url "https://${S3_HOST}" \
  "s3://${S3_BUCKET_NAME}/${BEEHIVE_SMOKE_PREFIX}/raw/"
```

Expected: 50 JSON files.

## 5. Prepare Beehive Workdir

Create:

```text
/tmp/beehive_s3_smoke_workdir
```

Copy `pipeline.s3_smoke.example.yaml` to `/tmp/beehive_s3_smoke_workdir/pipeline.yaml` and replace `workflow_url` with `BEEHIVE_N8N_SMOKE_WEBHOOK`.

Do not commit this workdir or secrets.

## 6. Smoke Success Definition

Minimum successful smoke:

1. S3 contains 50 source objects in `beehive-smoke/test_workflow/raw`.
2. Beehive reconcile registers them as pending on stage `smoke_source`.
3. `run_due_tasks` takes one artifact.
4. n8n receives empty body and `X-Beehive-*` headers.
5. n8n downloads exactly the source bucket/key.
6. n8n uploads output JSON to `beehive-smoke/test_workflow/processed`.
7. n8n returns a manifest.
8. Beehive validates the manifest.
9. Source state becomes `done`.
10. Child artifact pointer appears on stage `smoke_processed`.

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

## 7. SQLite Checks

```bash
sqlite3 /tmp/beehive_s3_smoke_workdir/app.db \
  "select entity_id, stage_id, status, file_exists from entity_stage_states order by updated_at desc limit 20;"
```

```bash
sqlite3 /tmp/beehive_s3_smoke_workdir/app.db \
  "select run_id, entity_id, stage_id, success, http_status, error_type from stage_runs order by id desc limit 10;"
```

```bash
sqlite3 /tmp/beehive_s3_smoke_workdir/app.db \
  "select entity_id, artifact_id, stage_id, storage_provider, bucket, object_key, producer_run_id from entity_files order by id desc limit 20;"
```

## 8. S3 Output Check

```bash
aws s3 ls --endpoint-url "https://${S3_HOST}" \
  "s3://${S3_BUCKET_NAME}/${BEEHIVE_SMOKE_PREFIX}/processed/"
```

Expected: at least one output JSON.

## 9. Honest Blockers

If smoke cannot run, name the exact blocker. Now valid blockers are limited to:

```text
n8n workflow not imported / webhook URL not provided
n8n S3 credentials not configured
S3 upload failed
Beehive command unavailable in current environment
manifest route blocked
n8n upload node requires manual setup
other concrete blocker
```

The blocker should not be "no source object" or "no pipeline" if the smoke kit dataset/pipeline is available.

## 10. Feedback

Create:

```text
docs/beehive_s3_b2_1_real_smoke_feedback.md
```

Include prepared/uploaded counts, bucket/prefix, webhook path, reconciliation result, source entity/artifact/key, run_id, output artifact/key, final states, S3 output check, commands run, exact failures, and next steps.
