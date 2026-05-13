# Beehive S3 B2.1 Real Smoke Feedback

## 1. Source Object Preparation

`selected_50_for_n8n.zip` is present in the repo root and was processed with the smoke kit script.

Command:

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
```

Result:

```text
Prepared 50 smoke objects
Local JSON count: 50
```

## 2. S3 Upload

Bucket/prefix:

```text
s3://steos-s3-data/beehive-smoke/test_workflow/raw/
```

Initial non-escalated upload failed because sandbox networking used an unavailable proxy.

Escalated upload first failed with AWS CLI TLS validation:

```text
SSL validation failed: self signed certificate in certificate chain
```

Retrying with the system CA bundle succeeded:

```bash
AWS_CA_BUNDLE=/etc/ssl/certs/ca-certificates.crt ./upload_selected50_to_s3.sh
```

User-provided `selectel` profile checks also succeeded:

```text
aws --profile selectel --endpoint-url "$SEL_ENDPOINT" s3 ls
```

Result included:

```text
s3data
steos-s3-data
```

Bucket check:

```text
aws --profile selectel --endpoint-url "$SEL_ENDPOINT" s3 ls s3://steos-s3-data/
```

Result included:

```text
PRE beehive-smoke/
PRE selected_50_for_n8n/
```

Raw prefix count:

```text
50
```

## 3. Workdir and Pipeline

Smoke workdir:

```text
/tmp/beehive_s3_smoke_workdir
```

The ignored Rust helper wrote:

```text
/tmp/beehive_s3_smoke_workdir/pipeline.yaml
/tmp/beehive_s3_smoke_workdir/app.db
```

The pipeline used:

```text
project.name=beehive-s3-smoke
storage.provider=s3
bucket=steos-s3-data
workspace_prefix=beehive-smoke/test_workflow
stage smoke_source input_uri=s3://steos-s3-data/beehive-smoke/test_workflow/raw
stage smoke_processed input_uri=s3://steos-s3-data/beehive-smoke/test_workflow/processed
```

The webhook URL came from `.env` and is not repeated here beyond the non-secret path class: `webhook-test`.

## 4. Beehive Reconciliation

Command:

```bash
cargo test --manifest-path src-tauri/Cargo.toml real_s3_n8n_smoke_one_artifact -- --ignored --nocapture
```

Reconciliation evidence:

```text
B2_1_RECONCILE listed=50 tagged=50 registered=50 updated=0 unchanged=0 unmapped=0 missing=0 restored=0
```

SQLite evidence:

```text
50 S3 source entity_files registered on stage smoke_source
smoke_entity_001 queued and then attempted
smoke_entity_002..smoke_entity_050 remained pending
```

Partial acceptance is satisfied up to S3 upload and Beehive reconciliation.

## 5. One-Artifact Real n8n Smoke

Run summary:

```text
B2_1_RUN_SUMMARY claimed=1 succeeded=0 failed=0 blocked=0 retry_scheduled=1 skipped=0
```

Latest stage run:

```text
run_id=d1ac14e5-781b-42ea-b799-d185ae770358
entity_id=smoke_entity_001
stage_id=smoke_source
success=0
http_status=404
error_type=http_status
error_message=n8n webhook returned HTTP status 404.
```

Source artifact key:

```text
beehive-smoke/test_workflow/raw/smoke_entity_001__порфирия.json
```

Final source state:

```text
smoke_entity_001 | smoke_source | retry_wait | file_exists=1 | last_http_status=404
```

Child artifact:

```text
none
```

S3 output check:

```text
s3://steos-s3-data/beehive-smoke/test_workflow/processed/
count=0
```

## 6. Exact Blocker

Real n8n smoke status: failed before manifest.

Blocker:

```text
n8n workflow not imported/listening or webhook-test URL is not active
```

Evidence:

```text
Beehive contacted the configured n8n webhook and received HTTP 404.
```

This is not a source-object blocker, not a pipeline blocker, and not an S3 upload blocker.

Likely resolution:

- import `beehive_s3_smoke_kit/n8n_beehive_s3_pointer_smoke_workflow.json`;
- attach Selectel S3 credentials to both n8n S3 nodes;
- activate the workflow and provide the production `/webhook/...` URL, or open the test workflow listener before using `/webhook-test/...`;
- rerun the ignored helper.

## 7. Commands and Results

- `python3 prepare_selected50_s3_smoke.py ...`: passed, prepared 50.
- `./upload_selected50_to_s3.sh`: first sandbox/proxy failed; escalated with default CA failed TLS; escalated with `AWS_CA_BUNDLE=/etc/ssl/certs/ca-certificates.crt` passed.
- `aws --profile selectel --endpoint-url "$SEL_ENDPOINT" s3 ls`: passed.
- `aws --profile selectel --endpoint-url "$SEL_ENDPOINT" s3 ls s3://steos-s3-data/`: passed.
- raw prefix count: passed, `50`.
- `cargo test --manifest-path src-tauri/Cargo.toml real_s3_n8n_smoke_one_artifact -- --ignored --nocapture`: failed at real n8n step with HTTP 404 after successful S3 reconciliation.
- `cargo test --manifest-path src-tauri/Cargo.toml`: passed, `122 passed; 0 failed; 1 ignored`.
- `git diff --check`: passed.

`sqlite3` CLI is not installed in the environment, so DB evidence was collected with Python's standard `sqlite3` module.

## 8. Files Added or Updated

- `codex_b2_1_real_smoke_instruction.md`
- `docs/beehive_s3_b2_1_real_smoke_plan.md`
- `docs/beehive_s3_b2_1_real_smoke_feedback.md`
- `src-tauri/src/s3_reconciliation.rs`
- extracted smoke kit under `beehive_s3_smoke_kit/`

## 9. Next Step

After n8n workflow import/activation and S3 node credential setup, rerun:

```bash
cargo test --manifest-path src-tauri/Cargo.toml real_s3_n8n_smoke_one_artifact -- --ignored --nocapture
```

Expected full-success evidence:

```text
stage_run success=true
source state done
child pointer pending on smoke_processed
processed S3 output count >= 1
```
