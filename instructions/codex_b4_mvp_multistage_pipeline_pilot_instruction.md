# B4. MVP Multi-stage Pipeline Pilot & Manual Wave Runner

## 0. Mission

You are continuing the Beehive S3+n8n integration after accepted B3.

B3 proved that Beehive can reconcile S3 artifacts, manually register S3 sources, expose an operator S3 console, and run a small real batch through n8n using the JSON body control envelope.

B4 must move the project from smoke testing toward MVP usage:

```text
operator selects/approves a small S3 batch -> Beehive runs bounded pipeline waves -> real n8n stages transform artifacts -> Beehive shows stage-to-stage lineage and failures safely
```

B4 is not another fixture/linter-heavy stage. B4 is a practical MVP pilot stage.

The main output of B4 is a controlled, operator-triggered, multi-stage pipeline run path.

## 1. Strategic decisions for B4

### 1.1 MVP over excessive testing

Do not spend B4 deepening the n8n linter into a production-grade workflow validator. The linter can remain lightweight. The priority is to run a real, bounded, multi-stage pipeline and make the operator experience usable.

### 1.2 n8n workflow fixtures are examples only

Do not try to store full production n8n workflows in this repository. Full n8n pipelines may be provided by the human when needed. Keep repo fixtures small, illustrative, and non-secret. The app must work with real workflow URLs configured in `pipeline.yaml`; it should not manage n8n workflow content.

### 1.3 README is not the final product manual yet

Do not rewrite README as a final product manual in B4. Minor notes are allowed only if they prevent immediate confusion. Full functional documentation can wait until the product is stable.

### 1.4 JSON body S3 control envelope remains the production contract

S3 mode must continue to call n8n with:

```text
POST workflow_url
Content-Type: application/json; charset=utf-8
Accept: application/json
body.schema = beehive.s3_control_envelope.v1
```

Do not reintroduce source-key headers. Do not send business JSON to n8n from Beehive. n8n downloads business JSON from S3 using the pointer in the control envelope.

## 2. Current baseline

Before coding, read:

```text
docs/beehive_s3_b2_2_feedback.md
docs/beehive_s3_b3_feedback.md
docs/s3_n8n_contract.md
docs/s3_operator_runbook.md
docs/n8n_workflow_authoring_standard.md
src-tauri/src/executor/mod.rs
src-tauri/src/s3_control_envelope.rs
src-tauri/src/s3_reconciliation.rs
src-tauri/src/commands/mod.rs
src/pages/WorkspaceExplorerPage.tsx
```

Also inspect any n8n workflow JSON files supplied by the human for this B4 stage. These files may be outside the repo. Do not commit full production workflows unless explicitly instructed.

## 3. Required plan before code

Create:

```text
docs/beehive_s3_b4_plan.md
```

The plan must include:

```text
1. B3 readiness summary.
2. Exact B4 MVP goal.
3. Proposed bounded pipeline wave runner design.
4. UI/operator flow changes.
5. Real n8n workflow preflight approach.
6. Real multi-stage pilot scenario.
7. Tests to add.
8. Commands to run.
9. What will not be implemented.
10. Risks and rollback.
```

Do not write runtime code before this plan exists.

## 4. Required reread checkpoints

Reread this instruction at these checkpoints:

```text
after_plan
after_wave_runner_design
after_wave_runner_backend
after_operator_ui
after_n8n_preflight
after_mock_multistage_tests
after_real_pilot_attempt
after_tests
before_feedback
```

Feedback must contain:

```text
ТЗ перечитано на этапах: after_plan, after_wave_runner_design, after_wave_runner_backend, after_operator_ui, after_n8n_preflight, after_mock_multistage_tests, after_real_pilot_attempt, after_tests, before_feedback
```

## 5. Core backend task: bounded pipeline wave runner

B3 added `run_due_tasks_limited`, which runs one bounded executor pass.

B4 must add a safe manual pipeline wave runner that repeatedly invokes existing due-task execution for a small number of waves.

Suggested command name:

```text
run_pipeline_waves
```

Suggested Tauri command contract:

```text
run_pipeline_waves(path, max_waves, max_tasks_per_wave, stop_on_first_failure)
```

Hard safety caps:

```text
max_waves: clamp to 1..10
max_tasks_per_wave: clamp to 1..5
max_total_tasks: max_waves * max_tasks_per_wave
```

Behavior:

```text
1. Load runtime context and config.
2. For wave 1..max_waves:
   a. Call the existing executor::run_due_tasks with max_tasks_per_wave.
   b. Record the wave summary.
   c. Stop if claimed == 0.
   d. Stop if stop_on_first_failure=true and wave has failed/blocked/errors.
3. Return an aggregate summary with wave details and a stopped_reason.
```

Allowed stopped reasons:

```text
idle
max_waves_reached
failure_or_blocked
runtime_error
```

Important constraints:

```text
Do not create a background daemon.
Do not run indefinitely.
Do not introduce worker pools.
Do not bypass existing claim/state-machine/retry behavior.
Do not read business JSON from S3 in Beehive execution path.
```

Add domain types as needed, for example:

```text
PipelineWaveSummary
RunPipelineWavesResult
```

Expose the command through:

```text
src-tauri/src/commands/mod.rs
src-tauri/src/lib.rs
src/lib/runtimeApi.ts
src/types/domain.ts
```

## 6. Operator UI task

Add a minimal UI control in Workspace Explorer or the existing S3 Operator Console:

```text
Run pipeline waves
```

Controls:

```text
max_waves: default 5, min 1, max 10
max_tasks_per_wave: default 3, min 1, max 5
stop_on_first_failure: default true
```

Display after run:

```text
waves executed
total claimed
total succeeded
total retry_scheduled
total failed
total blocked
total skipped
stopped_reason
per-wave summaries
```

Refresh Workspace Explorer after the run so the operator sees source/child states.

This UI is enough for MVP. Do not build a full visual graph builder in B4.

## 7. Mini n8n live workflow preflight

Do a lightweight preflight for any n8n workflow JSONs the human provides for B4.

Create:

```text
docs/n8n_live_workflow_preflight_b4.md
```

The preflight is not a full linter and should not block B4 unless it identifies a contract-breaking issue for the pilot workflow.

Check only practical MVP issues:

```text
1. The production webhook uses POST and responseNode.
2. The production path reads S3 source from JSON body, not source-key headers.
3. No production path references a missing node name.
4. Known old references such as `Read Beehive headers` are removed or fixed.
5. `Search Bucket` / `List Bucket` is not connected to production webhook source selection.
6. `save_path` values match configured Beehive S3 routes.
7. Legacy typo `/main_dir/pocessed` is not present in the active pilot path.
8. n8n returns a Beehive manifest synchronously unless async mode is explicitly declared.
```

If the current live workflow is outside the repo and cannot be modified by the agent, write exact operator instructions rather than trying to manage n8n through API.

Do not call n8n REST API to edit workflows in B4.

## 8. Real multi-stage MVP pilot

B4 must attempt a real multi-stage pilot when the human has provided enough workflow URLs and S3 configuration.

The pilot should use a small batch:

```text
3 source artifacts by default
5 maximum
```

Preferred pilot shape:

```text
source stage -> stage A n8n workflow -> child artifacts in next S3 stage -> stage B n8n workflow -> final/next child artifacts
```

Branching pilot is also acceptable:

```text
source stage -> n8n workflow returns outputs to two save_path routes -> Beehive registers both branches -> one branch continues one more stage
```

Minimum full B4 real-pilot success:

```text
1. Real S3 is contacted.
2. Real n8n is contacted.
3. At least 3 source artifacts are run or attempted by wave runner.
4. At least 2 n8n stage executions succeed across the run.
5. At least one artifact moves beyond the first n8n stage.
6. Source and child stage states are visible in Beehive.
7. Output S3 objects exist where n8n reported them.
8. Feedback lists source keys, run IDs, output keys, and final states.
```

If the human has not provided a real second-stage workflow URL, B4 may still implement the wave runner and run a mock multi-stage test, but feedback must clearly mark the real multi-stage pilot as blocked by missing workflow URL.

## 9. Mock tests required

Add Rust tests for the wave runner and S3 pipeline behavior.

Required cases:

```text
run_pipeline_waves stops when no tasks are claimed;
run_pipeline_waves clamps max_waves and max_tasks_per_wave;
run_pipeline_waves aggregates per-wave summaries;
run_pipeline_waves stops on failed/blocked when stop_on_first_failure=true;
mock S3 multi-stage pipeline moves one artifact through at least two stages;
mock S3 branching response registers outputs by save_path;
S3 control envelope still preserves Cyrillic source_key;
S3 execution still does not send business JSON to n8n.
```

Use local mock HTTP servers for n8n. Do not require real S3/n8n in normal tests.

The real pilot test must be ignored/opt-in, for example:

```text
real_s3_n8n_mvp_pipeline_pilot
```

## 10. Documentation required

Create/update:

```text
docs/beehive_s3_b4_plan.md
docs/beehive_s3_b4_feedback.md
docs/s3_mvp_pipeline_pilot_runbook.md
docs/n8n_live_workflow_preflight_b4.md
```

Minor updates are allowed in:

```text
docs/s3_operator_runbook.md
docs/s3_n8n_contract.md
```

Do not do a full README rewrite in B4. Add only a short pointer if absolutely needed.

## 11. B4 feedback requirements

Create:

```text
docs/beehive_s3_b4_feedback.md
```

It must include:

```text
1. B3 readiness result.
2. What was implemented.
3. Files changed.
4. Wave runner behavior and safety caps.
5. UI/operator flow changes.
6. n8n live workflow preflight findings.
7. Mock multi-stage test result.
8. Real multi-stage pilot status.
9. Whether real S3 was contacted.
10. Whether real n8n was contacted.
11. Source artifact keys attempted.
12. Stage run IDs.
13. Output artifact keys.
14. Final source and child states.
15. Commands run and exact results.
16. Tests passed/failed/ignored.
17. What could not be verified.
18. Ubuntu notes.
19. Windows notes.
20. Remaining risks.
21. What should be done in B5.
22. Reread checkpoints.
```

If real pilot is blocked, state the exact blocker:

```text
missing second-stage workflow URL
workflow does not return manifest
workflow route/save_path mismatch
n8n S3 credentials missing
S3/network unavailable
other
```

## 12. Verification commands

Run:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
git diff --check
```

Run the real pilot only when env/workflows are available:

```bash
BEEHIVE_REAL_S3_MVP_PIPELINE_PILOT=1 \
BEEHIVE_SMOKE_BATCH_LIMIT=3 \
cargo test --manifest-path src-tauri/Cargo.toml real_s3_n8n_mvp_pipeline_pilot -- --ignored --nocapture
```

Do not claim real pilot success unless it actually ran.

## 13. B4 acceptance criteria

B4 is accepted if:

```text
bounded run_pipeline_waves backend command exists;
operator UI exposes pipeline waves with safe limits;
existing single-pass run_due_tasks behavior remains intact;
mock multi-stage S3 pipeline test passes;
JSON body S3 control envelope remains the only production S3 contract;
no source-key headers are reintroduced;
manual n8n preflight report exists for supplied live workflows;
small real multi-stage pilot is either passed or honestly blocked with exact reason;
S3 operator runbook explains the MVP flow;
normal Rust tests and frontend build pass;
feedback is complete and concrete.
```

## 14. Non-goals

Do not implement:

```text
background daemon;
high-load worker pool;
async manifest polling;
n8n REST workflow editing;
credential manager UI;
full n8n workflow storage in repo;
full production README rewrite;
22,000-file production run;
large fixture/linter expansion;
business JSON reads from S3 inside Beehive execution path;
business JSON webhook body from Beehive to n8n.
```

## 15. Expected B5 focus

B5 should focus on reliability after MVP pilot:

```text
controlled repeated execution / backpressure;
operator QA report across real workflows;
retry/block recovery UX;
optional async manifest polling if real n8n workflows become long-running;
first production-sized dry run planning, not full 22,000 execution yet.
```
