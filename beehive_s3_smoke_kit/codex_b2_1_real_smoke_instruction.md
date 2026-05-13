# B2.1 Real Smoke Kit and Smoke Execution Instruction for Codex Agent

## 0. Mission

You are continuing Beehive S3+n8n integration after B2 foundation.

B2 added S3 metadata reconciliation and manual S3 source registration, but real S3+n8n smoke was not run. Your task is to turn the existing infrastructure into a real smoke run using the provided `selected_50_for_n8n.zip`, real S3 credentials, and a Beehive-header n8n workflow.

Main output:

```text
One concrete S3 source artifact is processed by n8n and tracked by Beehive as source done plus child S3 pointer.
```

Stretch output:

```text
50 selected source artifacts are uploaded/registered and a small controlled batch is run.
```

## 1. Read first

Read:

```text
docs/beehive_s3_b2_feedback.md
docs/n8n_s3_pointer_workflow_adapter.md
docs/s3_n8n_contract.md
src-tauri/src/s3_reconciliation.rs
src-tauri/src/s3_client.rs
src-tauri/src/executor/mod.rs
```

Also read this smoke kit:

```text
s3_real_smoke_miniguide.md
pipeline.s3_smoke.example.yaml
prepare_selected50_s3_smoke.py
n8n_beehive_s3_pointer_smoke_workflow.json
```

## 2. Required plan before code/run

Create:

```text
docs/beehive_s3_b2_1_real_smoke_plan.md
```

The plan must include:

```text
1. Which S3 bucket/prefix will be used.
2. How selected_50_for_n8n.zip will be transformed/uploaded.
3. Which n8n production webhook URL will be used.
4. Which workdir/pipeline.yaml will be used.
5. Whether smoke will use reconciliation or manual registration first.
6. Exact commands to run.
7. What DB/S3/n8n evidence will prove success.
8. What will be skipped if credentials/endpoint/webhook are missing.
```

Do not run destructive cleanup unless explicitly instructed.

## 3. Inputs expected from the human/operator

The human should provide or confirm:

```text
S3_HOST=s3.ru-1.storage.selcloud.ru
S3_REGION=ru-1
S3_KEY=***
S3_SEC_KEY=***
S3_BUCKET_NAME=steos-s3-data
BEEHIVE_SMOKE_PREFIX=beehive-smoke/test_workflow
BEEHIVE_N8N_SMOKE_WEBHOOK=https://n8n-dev.steos.io/webhook/<imported-smoke-path>
```

The human should import `n8n_beehive_s3_pointer_smoke_workflow.json` into n8n, attach S3 credentials to both S3 nodes, activate it, and provide the production webhook URL.

## 4. Prepare 50 S3 source objects

Run:

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

Verify list:

```bash
ENDPOINT="${S3_ENDPOINT:-https://${S3_HOST}}"
aws s3 ls --endpoint-url "$ENDPOINT" \
  "s3://${S3_BUCKET_NAME}/${BEEHIVE_SMOKE_PREFIX}/raw/"
```

## 5. Prepare Beehive S3 smoke workdir

Create a clean smoke workdir, for example:

```text
/tmp/beehive_s3_smoke_workdir
```

Copy `pipeline.s3_smoke.example.yaml` into it as `pipeline.yaml`.

Replace source stage `workflow_url` with `BEEHIVE_N8N_SMOKE_WEBHOOK`.

Do not commit this workdir or secrets.

## 6. Run smoke

Preferred path:

1. Bootstrap/open workdir.
2. Run `reconcile_s3_workspace`.
3. Confirm at least one S3 source artifact appears in DB as pending on stage `smoke_source`.
4. Run `run_due_tasks` with `max_parallel_tasks=1`.
5. Confirm one source is `done` and one child pointer is `pending` on `smoke_processed`.
6. Confirm output exists in S3 under `processed/`.

If no UI route exists, add a small ignored Rust integration test or helper command that runs this exact flow using env vars. It must not run by default in normal `cargo test`.

Suggested ignored test name:

```text
real_s3_n8n_smoke_one_artifact
```

It should:

```text
- create temp workdir/pipeline.yaml from env;
- bootstrap database;
- optionally call reconcile_s3_workspace;
- if reconciliation finds none, manually register the first artifact from smoke_source_manifest.json;
- run_due_tasks(max_tasks=1);
- query DB for source done and child pending;
- optionally head/list S3 processed prefix;
- print run_id, source key, output key, final states.
```

## 7. Required feedback

Create:

```text
docs/beehive_s3_b2_1_real_smoke_feedback.md
```

It must include:

```text
1. Whether 50 selected source objects were prepared.
2. Whether source objects were uploaded to S3.
3. Bucket/prefix used, without secrets.
4. Whether n8n workflow was imported and which webhook URL/path was used, without secrets.
5. Whether Beehive reconciliation found the objects.
6. Whether one-artifact real smoke ran.
7. Source entity_id/artifact_id/key.
8. n8n run result or Beehive stage_run run_id.
9. Output artifact_id/key.
10. Final source state and child state.
11. S3 output existence check.
12. Commands run and exact results.
13. What failed or could not be verified.
14. What should be done next.
```

If real smoke cannot run, do not claim success. State the exact blocker:

```text
missing n8n webhook URL
missing credentials
S3 upload failed
n8n download failed
manifest route blocked
Beehive command inaccessible
other
```

## 8. Acceptance criteria

B2.1 is accepted only if at least one of these is true:

### Full acceptance

```text
Real S3 was contacted.
Real n8n was contacted.
One selected source artifact reached source done.
One child S3 pointer was registered in smoke_processed.
Output object exists in S3 processed prefix.
Feedback contains concrete run evidence.
```

### Partial acceptance

```text
50 objects uploaded to S3 and reconciled in Beehive,
but n8n real smoke is blocked by missing/invalid webhook.
```

If neither is true, B2.1 is not complete.
