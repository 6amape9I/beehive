# Beehive S3 B4 Plan

## 1. B3 Readiness Summary

B3 is the accepted baseline for B4. It added the S3 operator console, manual S3 registration, limited one-pass due-task execution, centralized S3 control envelope, lightweight n8n workflow governance, and a real 3-artifact S3+n8n batch smoke. The active production contract remains the JSON body S3 control envelope with `Content-Type: application/json; charset=utf-8`; source-key headers and business JSON webhook bodies remain deprecated/forbidden for S3 mode.

## 2. Exact B4 MVP Goal

B4 will add an operator-triggered MVP path for bounded multi-stage pipeline waves:

```text
operator chooses small limits -> Beehive runs repeated due-task waves -> n8n stages create S3 output pointers -> operator inspects stage-to-stage lineage and failures
```

The goal is not a scheduler daemon or high-load executor. It is a controlled manual pilot surface for multi-stage S3 pipelines.

## 3. Bounded Pipeline Wave Runner Design

Add backend command `run_pipeline_waves(path, max_waves, max_tasks_per_wave, stop_on_first_failure)` using existing `executor::run_due_tasks` semantics.

Safety caps:

- `max_waves`: clamp to `1..10`
- `max_tasks_per_wave`: clamp to `1..5`
- `max_total_tasks`: `max_waves * max_tasks_per_wave`

The runner will record one summary per wave, aggregate totals, and stop on:

- `idle` when a wave claims zero tasks
- `failure_or_blocked` when requested and a wave has failed/blocked/errors
- `runtime_error` when a due-task pass returns an execution error
- `max_waves_reached` when the configured wave cap is exhausted

It will not bypass the existing claim/state-machine/retry behavior.

## 4. UI/Operator Flow Changes

Extend the existing Workspace Explorer S3 Operator Console with:

- `max_waves` input, default `5`, range `1..10`
- `max_tasks_per_wave` input, default `3`, range `1..5`
- `stop_on_first_failure` checkbox, default `true`
- `Run pipeline waves` action
- aggregate result cards and per-wave summaries

After the run, Workspace Explorer will refresh so operators can inspect source/child states.

## 5. Real n8n Workflow Preflight Approach

Create `docs/n8n_live_workflow_preflight_b4.md`. Inspect repository workflow fixtures and any B4 workflow JSONs supplied locally. Do not call the n8n REST API and do not commit full production workflows. If no live multi-stage workflow JSON is supplied, document the precise blocker and operator instructions.

The preflight will check only practical MVP contract issues: POST/responseNode, body JSON source pointer, no old header node path, no production Search/List Bucket source selection, no `/main_dir/pocessed`, route-compatible `save_path`, and synchronous manifest return.

## 6. Real Multi-stage Pilot Scenario

Preferred real pilot:

```text
smoke_source -> real n8n stage A -> smoke_processed -> real n8n stage B -> smoke_final
```

If no second-stage real workflow URL is available, B4 will still implement and test the wave runner with local mock HTTP multi-stage tests and mark the real pilot as blocked by missing second-stage workflow URL. If the existing body-JSON smoke webhook can safely be reused as both stage A and stage B, the opt-in pilot will attempt a 3-artifact wave run against real S3/n8n.

## 7. Tests To Add

Rust/mock coverage:

- wave runner stops when no tasks are claimed
- wave runner clamps `max_waves` and `max_tasks_per_wave`
- wave runner aggregates per-wave summaries
- wave runner stops on failed/blocked when requested
- mock S3 multi-stage pipeline moves one artifact through at least two stages
- mock S3 branching response registers outputs by `save_path`
- S3 control envelope still preserves Cyrillic `source_key`
- S3 execution still excludes business JSON from HTTP body/request audit

Opt-in real test:

- `real_s3_n8n_mvp_pipeline_pilot`, ignored by default and gated by `BEEHIVE_REAL_S3_MVP_PIPELINE_PILOT=1`

## 8. Commands To Run

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
git diff --check
```

Real pilot only if environment/workflows are available:

```bash
BEEHIVE_REAL_S3_MVP_PIPELINE_PILOT=1 BEEHIVE_SMOKE_BATCH_LIMIT=3 cargo test --manifest-path src-tauri/Cargo.toml real_s3_n8n_mvp_pipeline_pilot -- --ignored --nocapture
```

## 9. What Will Not Be Implemented

B4 will not implement a background daemon, worker pool, async manifest polling, n8n REST workflow editing, credential manager UI, large production run, production workflow storage, full README rewrite, or any S3 business JSON reads/webhook body sending by Beehive.

## 10. Risks And Rollback

- The wave runner could accidentally feel like a scheduler; keep it manual, bounded, and small.
- Real pilot can be blocked by missing second-stage workflow URL, n8n credentials, workflow manifest shape, route mismatch, or S3/network availability.
- UI changes are limited to Workspace Explorer and can be rolled back independently of backend command/types.
- The backend runner is a thin loop around existing due-task execution, so rollback is removing the command/API/UI without altering single-pass behavior.

## Checkpoints

- after_plan
- after_wave_runner_design
- after_wave_runner_backend
- after_operator_ui
- after_n8n_preflight
- after_mock_multistage_tests
- after_real_pilot_attempt
- after_tests
- before_feedback
