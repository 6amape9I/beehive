# Beehive S3 B4 Feedback

## 1. B3 readiness result

B3 was ready for B4. The S3 operator console, JSON-body S3 control envelope, manual S3 registration, limited one-pass batch execution, body-JSON n8n fixture, lightweight workflow linter, and real 3-artifact S3+n8n batch smoke were already in place.

## 2. What was implemented

Implemented a bounded manual pipeline wave runner, exposed it in the Workspace Explorer S3 Operator Console, added B4 n8n live workflow preflight notes, added the MVP pipeline pilot runbook, added mock multi-stage/branching S3 tests, and added an ignored real MVP pilot test.

## 3. Files changed

- `docs/beehive_s3_b4_plan.md`
- `docs/beehive_s3_b4_feedback.md`
- `docs/n8n_live_workflow_preflight_b4.md`
- `docs/s3_mvp_pipeline_pilot_runbook.md`
- `docs/s3_operator_runbook.md`
- `src-tauri/src/domain/mod.rs`
- `src-tauri/src/executor/mod.rs`
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/s3_reconciliation.rs`
- `src/types/domain.ts`
- `src/lib/runtimeApi.ts`
- `src/pages/WorkspaceExplorerPage.tsx`
- `src/app/styles.css`

## 4. Wave runner behavior and safety caps

Added `run_pipeline_waves(path, max_waves, max_tasks_per_wave, stop_on_first_failure)`.

Safety caps:

```text
max_waves: 1..10
max_tasks_per_wave: 1..5
max_total_tasks: max_waves * max_tasks_per_wave
```

The runner repeatedly calls existing `executor::run_due_tasks`, records per-wave summaries, aggregates totals, and stops with `idle`, `max_waves_reached`, `failure_or_blocked`, or `runtime_error`. It does not bypass the existing claim/state-machine/retry behavior.

## 5. UI/operator flow changes

Workspace Explorer S3 Operator Console now includes `Run pipeline waves` with `max_waves`, `max_tasks_per_wave`, and `stop_on_first_failure`. The UI displays aggregate totals, stopped reason, and per-wave summaries, then refreshes Workspace Explorer.

## 6. n8n live workflow preflight findings

Created `docs/n8n_live_workflow_preflight_b4.md`. The local active body-JSON fixture passes the practical preflight: POST/responseNode, body `source_bucket/source_key`, no source-key headers, no old header node, no Search/List Bucket source selection, no `/main_dir/pocessed`, synchronous manifest path.

No separate full live stage-B workflow JSON was supplied. The real B4 pilot reused `BEEHIVE_N8N_SMOKE_WEBHOOK` as stage B, and the pilot passed.

## 7. Mock multi-stage test result

Passed:

- `mock_s3_multistage_pipeline_moves_one_artifact_through_two_stages`
- `mock_s3_branching_response_registers_outputs_by_save_path`
- `run_pipeline_waves_stops_when_no_tasks_are_claimed`
- `run_pipeline_waves_clamps_limits_and_aggregates_per_wave_summaries`
- `run_pipeline_waves_stops_on_failed_or_blocked_when_requested`

## 8. Real multi-stage pilot status

Passed.

The pilot used an isolated workdir:

```text
/tmp/beehive_s3_mvp_pipeline_pilot_workdir
```

It approved 3 source artifacts in the isolated DB, then ran:

```text
max_waves=2
max_tasks_per_wave=3
stop_on_first_failure=true
```

Result:

```text
waves_executed=2
stopped_reason=max_waves_reached
total_claimed=6
total_succeeded=6
total_failed=0
total_blocked=0
```

Report:

```text
/tmp/beehive_s3_mvp_pipeline_pilot_workdir/mvp_pipeline_pilot_report.json
```

## 9. Whether real S3 was contacted

Yes. Real S3 reconciliation listed `65` objects, found `50` metadata-tagged source objects, registered `50`, and reported `15` unmapped existing objects from prior smoke output prefixes. The pilot also verified final output objects with S3 head calls.

## 10. Whether real n8n was contacted

Yes. Six real n8n executions succeeded: three stage-A executions and three stage-B executions.

## 11. Source artifact keys attempted

```text
beehive-smoke/test_workflow/raw/smoke_entity_003__цистицеркоз.json
beehive-smoke/test_workflow/raw/smoke_entity_002__миосаркома-желчного-пузыря.json
beehive-smoke/test_workflow/raw/smoke_entity_001__порфирия.json
```

## 12. Stage run IDs

```text
b2286288-7794-48ce-a060-b2a104b145fa
36755734-9d7c-4df5-a5c1-4dd756ec455e
be5af9b7-a936-4c45-9aac-c44f9f611d84
b9f25cad-cb2c-4e75-aa92-3fe3fd774f48
cb1d8353-8418-4c8e-b3b8-09648401ebd8
b0cfe267-84b1-462b-995b-7f4b48fac769
```

## 13. Output artifact keys

Processed stage:

```text
beehive-smoke/test_workflow/processed/smoke-output-b2286288-7794-48ce-a060-b2a104b145fa.json
beehive-smoke/test_workflow/processed/smoke-output-36755734-9d7c-4df5-a5c1-4dd756ec455e.json
beehive-smoke/test_workflow/processed/smoke-output-be5af9b7-a936-4c45-9aac-c44f9f611d84.json
```

Final stage:

```text
beehive-smoke/test_workflow/final/smoke-output-b9f25cad-cb2c-4e75-aa92-3fe3fd774f48.json
beehive-smoke/test_workflow/final/smoke-output-cb1d8353-8418-4c8e-b3b8-09648401ebd8.json
beehive-smoke/test_workflow/final/smoke-output-b0cfe267-84b1-462b-995b-7f4b48fac769.json
```

## 14. Final source and child states

Final child states:

```text
smoke_entity_003 / smoke_final / pending
smoke_entity_002 / smoke_final / pending
smoke_entity_001 / smoke_final / pending
```

The real pilot confirms artifacts moved beyond the first n8n stage into the final S3 stage.

## 15. Commands run and exact results

```text
cargo fmt --manifest-path src-tauri/Cargo.toml
result: passed

cargo test --manifest-path src-tauri/Cargo.toml run_pipeline_waves -- --nocapture
result: passed; 3 tests

cargo test --manifest-path src-tauri/Cargo.toml mock_s3_ -- --nocapture
result: passed; 2 tests

BEEHIVE_REAL_S3_MVP_PIPELINE_PILOT=1 BEEHIVE_SMOKE_BATCH_LIMIT=3 cargo test --manifest-path src-tauri/Cargo.toml real_s3_n8n_mvp_pipeline_pilot -- --ignored --nocapture
result: passed; 1 test

cargo test --manifest-path src-tauri/Cargo.toml
result: passed; 128 passed, 0 failed, 3 ignored

npm run build
result: passed

python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
result: passed

git diff --check
result: passed
```

## 16. Tests passed/failed/ignored

Passed: normal Rust suite, frontend build, n8n workflow lint, targeted mock wave/multi-stage tests, and real B4 pilot.

Ignored in normal Rust suite:

```text
real_s3_n8n_smoke_one_artifact
real_s3_n8n_smoke_batch_small
real_s3_n8n_mvp_pipeline_pilot
```

Diagnostic failures during implementation:

- Running the ignored real pilot without `BEEHIVE_REAL_S3_MVP_PIPELINE_PILOT=1` failed on the intended opt-in guard.
- An initial real pilot attempt without approving only the requested 3 sources used wave 2 for more raw sources; this was fixed by constraining the isolated pilot DB to the approved source set before running waves.

## 17. What could not be verified

The UI was verified by TypeScript/Vite build, not manually inspected in a browser. No separate production stage-B workflow JSON was supplied; the real pilot reused the body-JSON smoke webhook for stage B.

## 18. Ubuntu notes

Use `.env` or shell environment variables for S3/n8n configuration. Real S3/n8n pilot requires network access and Selectel S3 certificate trust to be configured as in the previous smoke setup.

## 19. Windows notes

Keep secrets out of Git. Use PowerShell environment variables for opt-in pilot runs and quote Cyrillic paths/keys carefully if manually inspecting AWS CLI output.

## 20. Remaining risks

The wave runner is manual and bounded, not a scheduler. With many pending source artifacts, operators must approve/register or otherwise constrain the intended source set for a true staged pilot. The current UI does not yet provide per-row source selection; the real pilot test constrains the isolated DB internally.

## 21. What should be done in B5

B5 should add better operator QA around approved batches, retry/block recovery UX, backpressure controls for repeated manual waves, and a clearer source approval workflow before any larger production-sized dry run.

## 22. Reread checkpoints

ТЗ перечитано на этапах: after_plan, after_wave_runner_design, after_wave_runner_backend, after_operator_ui, after_n8n_preflight, after_mock_multistage_tests, after_real_pilot_attempt, after_tests, before_feedback
