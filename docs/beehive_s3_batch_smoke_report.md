# Beehive S3 Batch Smoke Report

## Scope

Short opt-in B2/B3 readiness smoke for the current S3 production contract:

```text
Beehive JSON control envelope body -> n8n -> S3 output manifest -> Beehive pointer registration
```

The test is ignored by default and must be run explicitly:

```bash
cargo test --manifest-path src-tauri/Cargo.toml real_s3_n8n_smoke_batch_three_to_five_artifacts -- --ignored --nocapture
```

Default batch size is 3. Use `BEEHIVE_BATCH_SMOKE_LIMIT=5` for 5.

## Latest Evidence

Command result: passed.

```text
B2_RECONCILE listed=53 tagged=50 registered=50 updated=0 unchanged=0 unmapped=3 missing=0 restored=0
B2_RUN_SUMMARY claimed=3 succeeded=3 failed=0 blocked=0 retry_scheduled=0 skipped=0
```

Runs:

```text
run_id=2417c7e8-3860-41b7-97be-d794c6343f80
source_key=beehive-smoke/test_workflow/raw/smoke_entity_001__порфирия.json
output_key=beehive-smoke/test_workflow/processed/smoke-output-2417c7e8-3860-41b7-97be-d794c6343f80.json
source_state=done
child_state=pending
s3_output_exists=true
size=2186
```

```text
run_id=7ad31fa4-9647-47ce-83ce-35416ae5df31
source_key=beehive-smoke/test_workflow/raw/smoke_entity_002__миосаркома-желчного-пузыря.json
output_key=beehive-smoke/test_workflow/processed/smoke-output-7ad31fa4-9647-47ce-83ce-35416ae5df31.json
source_state=done
child_state=pending
s3_output_exists=true
size=1591
```

```text
run_id=46aded5c-dace-4299-a640-0f2dce25198f
source_key=beehive-smoke/test_workflow/raw/smoke_entity_003__цистицеркоз.json
output_key=beehive-smoke/test_workflow/processed/smoke-output-46aded5c-dace-4299-a640-0f2dce25198f.json
source_state=done
child_state=pending
s3_output_exists=true
size=3029
```

Machine-readable report:

```text
/tmp/beehive_s3_batch_smoke_workdir/batch_smoke_report.json
```
