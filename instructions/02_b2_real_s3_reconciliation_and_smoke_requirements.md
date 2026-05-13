# B2. Real S3 Reconciliation and One-Artifact n8n Smoke Pipeline

## 0. Назначение этапа

B2 is the first real integration stage after B1/B1.1.

Goal:

```text
Make Beehive operate against real S3 object metadata and run one concrete claimed S3 artifact through n8n, producing registered output artifact pointers and visible stage transitions.
```

B2 should prove the end-to-end architecture:

```text
S3 source object pointer → Beehive claim → n8n webhook with empty body + headers → n8n downloads S3 JSON → n8n writes S3 outputs → n8n returns manifest → Beehive validates manifest → Beehive registers child artifacts → operator can see lineage/status.
```

B2 is not a high-load scheduler. High throughput and worker pools are intentionally deferred.

## 1. Precondition

Do not start B2 runtime coding unless B1.1 has been completed and `docs/beehive_s3_b1_1_feedback.md` says:

```text
B2 readiness: ready
```

If B1.1 feedback is missing or says blocked, create `docs/beehive_s3_b2_plan.md`, record the blocker, and stop before runtime code changes unless the human explicitly overrides.

## 2. Strategic rules

Beehive is control plane. n8n is data plane. S3 is artifact storage.

In production path:

```text
Beehive selects the artifact.
Beehive claims the stage state.
Beehive sends only technical S3 pointer headers to n8n.
n8n must process exactly that source bucket/key.
n8n must not choose source inputs via Search Bucket/List Bucket.
Beehive validates the returned manifest before changing source state to done.
```

Do not send business JSON to n8n.

Do not read S3 business JSON during execution just to call n8n. Reading/listing metadata is allowed for reconciliation. Optional preview can be future work, but execution must be pointer-based.

## 3. What to read first

Read:

```text
README.md
instructions/00_beehive_s3_global_vision.md
instructions/01_b1_s3_control_plane_requirements.md
docs/beehive_s3_b1_feedback.md
docs/beehive_s3_b1_1_feedback.md
docs/s3_control_plane_architecture.md
docs/s3_n8n_contract.md
src-tauri/src/domain/mod.rs
src-tauri/src/config/mod.rs
src-tauri/src/save_path.rs
src-tauri/src/s3_manifest.rs
src-tauri/src/executor/mod.rs
src-tauri/src/database/mod.rs
src-tauri/src/commands/mod.rs
src-tauri/src/dashboard/mod.rs
src-tauri/src/discovery/mod.rs
```

Also inspect the uploaded/current n8n workflow examples if available. Some examples show a manual S3 branch:

```text
Manual Trigger → Search bucket → Download file → Extract from File
```

This is useful as proof that n8n can read S3, but B2 production path must instead use Beehive-provided bucket/key headers.

## 4. Work style requirements

Start with a plan.

Create:

```text
docs/beehive_s3_b2_plan.md
```

Plan must include:

```text
1. B1.1 readiness status.
2. S3 client choice and why.
3. Credential/env strategy.
4. Reconciliation design.
5. Manual source artifact registration design.
6. Real smoke flow design.
7. n8n workflow adapter requirements.
8. Tests to add.
9. Commands to run.
10. What will not be implemented.
11. Risks.
```

Do not write runtime code before the plan.

## 5. Required reread checkpoints

Reread this instruction at checkpoints:

```text
after_plan
after_s3_client_setup
after_s3_reconciliation
after_manual_registration
after_smoke_runner
after_n8n_adapter_docs
after_tests
before_feedback
```

Feedback must contain:

```text
ТЗ перечитано на этапах: after_plan, after_s3_client_setup, after_s3_reconciliation, after_manual_registration, after_smoke_runner, after_n8n_adapter_docs, after_tests, before_feedback
```

## 6. S3 client and credentials

Implement a real S3 metadata client for reconciliation and object existence checks.

Preferred approach:

```text
Use an official or well-maintained Rust S3 client.
Do not hand-roll AWS Signature V4 if avoidable.
Support S3-compatible endpoint from config/env.
```

Credential rules:

```text
Do not hardcode credentials.
Do not commit credentials.
Do not print secrets.
Use environment variables and/or standard AWS credential chain.
Support pipeline storage.region and storage.endpoint where possible.
```

Recommended environment variables:

```text
AWS_ACCESS_KEY_ID
AWS_SECRET_ACCESS_KEY
AWS_SESSION_TOKEN optional
AWS_REGION or BEEHIVE_S3_REGION
BEEHIVE_S3_ENDPOINT optional for S3-compatible storage
```

If real S3 credentials are absent, B2 must still compile and tests must run with mocks. Feedback must clearly say real S3 smoke was not run due to missing credentials.

## 7. S3 reconciliation

Add S3 reconciliation capability. It may be a new command or integrated with existing `scan_workspace` as storage-aware scan.

Preferred command names:

```text
reconcile_s3_workspace
or storage-aware scan_workspace that detects storage.provider=s3
```

Minimum behavior:

```text
Load active S3 stages from pipeline config/SQLite.
For each active stage with input_uri, list objects under that bucket/prefix.
Fetch metadata/HeadObject where needed.
Register known objects with Beehive metadata or report unmapped objects.
Mark previously registered S3 artifacts missing if they no longer appear.
Restore missing S3 artifacts if they reappear.
Record app_events for discovered/restored/missing/unmapped S3 artifacts.
Produce a reconciliation summary.
```

Important: do not require reading object body. B2 reconciliation is metadata/pointer-based.

### 7.1 How to identify entity_id/artifact_id during S3 listing

Do not guess logical entity_id from arbitrary file names unless a documented rule exists.

Supported B2 identification methods:

1. S3 object user metadata, if present:

```text
x-amz-meta-beehive-entity-id
x-amz-meta-beehive-artifact-id
x-amz-meta-beehive-stage-id optional
x-amz-meta-beehive-source-artifact-id optional
```

2. A Beehive source manifest / seed manifest, if implemented:

```json
{
  "schema": "beehive.s3_seed_manifest.v1",
  "artifacts": [
    {
      "entity_id": "entity_smoke_001",
      "artifact_id": "art_smoke_001",
      "stage_id": "raw",
      "bucket": "steos-s3-data",
      "key": "main_dir/raw/smoke/input_001.json"
    }
  ]
}
```

3. Manual registration command/action.

If an object has no entity metadata and is not in a seed/registration list, record it as unmapped, but do not silently create a runnable entity.

## 8. Manual source artifact registration

B2 must provide a way to register one known S3 object as a runnable source artifact without reading its business JSON.

This can be a Tauri command, backend function, or CLI/helper script. UI polish is not required.

Suggested command contract:

```text
register_s3_source_artifact(
  workdir_path,
  stage_id,
  entity_id,
  artifact_id,
  bucket,
  key,
  version_id optional,
  etag optional,
  checksum_sha256 optional,
  size optional
)
```

Behavior:

```text
Validate stage exists and is active.
Validate stage is S3-capable and key is under stage input_uri prefix or an allowed alias/prefix.
Optionally HeadObject to verify existence if credentials are available.
Register entity/artifact pointer.
Create pending entity_stage_state.
Do not read object body.
Emit app_event.
Return detail/summary.
```

Tests:

```text
manual registration creates S3 entity artifact pointer and pending state;
wrong stage prefix rejected;
unknown stage rejected;
duplicate registration idempotent;
conflicting duplicate rejected;
no local file read attempted.
```

## 9. n8n workflow adapter requirement

B2 should not manage n8n through the n8n REST API. But it must provide clear docs and, if practical, an example fixture workflow for the n8n operator.

Create:

```text
docs/n8n_s3_pointer_workflow_adapter.md
```

This doc must explain how to convert the manual S3 pattern into Beehive-triggered production pattern:

Old demo/manual pattern:

```text
Manual Trigger → Search bucket → Download file → Extract from File
```

Production pattern:

```text
Webhook → Read X-Beehive-Source-Bucket/Key headers → Download exactly that S3 object → Extract JSON → Existing workflow logic → Upload each output to S3 → Return beehive.s3_artifact_manifest.v1
```

The doc must explicitly say:

```text
Do not use Search bucket to select production input.
Use X-Beehive-Run-Id as manifest run_id.
Use X-Beehive-Manifest-Prefix for optional manifest object writes.
Return manifest synchronously for B2 unless async mode is explicitly implemented.
Each output must include artifact_id, entity_id, relation_to_source, bucket, key, save_path.
```

Also add a workflow preflight checklist:

```text
Webhook uses POST and response node/mode.
The workflow reads headers, not hardcoded file keys.
All save_path values match pipeline stage aliases/input_uri.
No legacy typo routes such as /main_dir/pocessed/... unless intentionally configured.
Manifest outputs have entity_id separate from artifact_id.
S3 upload keys are under resolved save_path prefix.
```

## 10. One-artifact real smoke runner

Add a supported smoke flow. It may be a documented manual procedure plus backend commands, or a small helper script/command if appropriate.

Required smoke scenario:

```text
1. pipeline.yaml configures storage.provider=s3.
2. At least one source stage has input_uri.
3. At least one target stage has input_uri/save_path_aliases.
4. One known source S3 object is manually registered or discovered via metadata/seed manifest.
5. Run due tasks claims exactly that artifact.
6. Beehive calls n8n with empty body and S3 pointer headers.
7. n8n returns a valid manifest.
8. Beehive registers output artifact pointers.
9. Source state becomes done.
10. Child artifact states become pending or terminal done depending stage config.
11. Dashboard/entity detail/stage runs show the lineage.
```

If real n8n endpoint is unavailable in the agent environment, add a mock smoke test using local HTTP server and document the real steps for the human.

Do not require real secrets in tests.

## 11. Optional async manifest support

B2 may remain synchronous: n8n returns manifest in webhook response.

If async is easy and safe, support:

```text
HTTP 202 with manifest bucket/key or manifest_prefix.
Beehive leaves state in in_progress or retry_wait/pending_manifest.
A reconcile command checks manifest object later.
```

But do not overreach. Synchronous manifest response is acceptable for B2.

If async is not implemented, state clearly in docs:

```text
B2 supports synchronous manifest response only.
```

## 12. UI/operator visibility for B2

Minimum operator visibility:

```text
Dashboard shows S3 artifacts in counts.
Entity Detail shows source artifact pointer, stage_run request headers/audit envelope, manifest response, output artifact pointers.
Workspace Explorer or equivalent read model shows bucket/key/provider for S3 artifacts.
Open local file/folder disabled for S3 artifacts.
Manual retry/reset/skip works for S3 artifact stage states.
```

UI polish is not required, but no misleading local path behavior should remain for S3 pointers.

## 13. What B2 must not do

Do not:

```text
implement high-load worker pool;
implement background daemon if it is not already safe;
build credential manager UI;
call n8n REST API to edit workflows;
read business JSON from S3 in Beehive execution path;
send business JSON to n8n;
let n8n choose source object by Search bucket in production;
make unknown S3 objects runnable without entity/artifact identity;
ignore manifest route errors;
change local mode behavior unless required for compatibility;
commit secrets or real bucket dumps;
```

## 14. Tests required

Add or update Rust tests for:

```text
S3 client abstraction can be mocked.
S3 reconciliation registers metadata-tagged objects.
S3 reconciliation records unmapped objects without making them runnable.
S3 reconciliation marks missing/restored artifacts.
Manual registration creates pending S3 source state.
Manual registration validates prefix/stage.
S3 smoke with mock HTTP n8n sends empty body and pointer headers.
S3 smoke with valid manifest registers output pointers.
S3 smoke with invalid route blocks run.
S3 smoke with missing output entity_id rejects manifest.
Retry/reset/skip still work for S3 states.
Local mode tests still pass.
```

If real S3 smoke is run, do not make it a unit test that requires secrets. Put it behind an opt-in command or documented manual command.

## 15. Verification commands

Run:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
git diff --check
```

If an opt-in real S3 smoke command exists, document it and run it only when credentials and endpoint are present:

```bash
# Example only; final command may differ
BEEHIVE_REAL_S3_SMOKE=1 cargo test --manifest-path src-tauri/Cargo.toml s3_real_smoke -- --ignored
```

Do not claim real S3 smoke passed unless it actually ran.

## 16. Required docs and feedback

Create/update:

```text
docs/beehive_s3_b2_plan.md
docs/n8n_s3_pointer_workflow_adapter.md
docs/s3_control_plane_architecture.md
docs/s3_n8n_contract.md
docs/beehive_s3_b2_feedback.md
```

Feedback must include:

```text
1. B1.1 readiness result.
2. What was implemented.
3. Files changed.
4. S3 client/credential strategy.
5. S3 reconciliation behavior.
6. Manual source artifact registration behavior.
7. n8n pointer workflow adapter summary.
8. One-artifact smoke flow status.
9. Whether real S3 was contacted.
10. Whether real n8n was contacted.
11. Mock tests added/updated.
12. Commands run and exact results.
13. What could not be verified.
14. Ubuntu notes.
15. Windows notes.
16. Remaining risks.
17. What should be done in B3.
18. ТЗ reread checkpoints.
```

If real S3/n8n smoke could not run, state exactly why:

```text
Real smoke status: not run
Reason: missing S3 credentials / missing n8n endpoint / no source object provided / other
```

If real smoke ran, include:

```text
Real smoke status: passed|failed
source bucket/key
source stage
run_id
output count
created child artifact pointers
final source state
next child states
```

Do not include secrets.

## 17. Acceptance criteria

B2 is acceptable if:

```text
B1.1 readiness was checked;
S3 metadata client or abstraction exists and is testable;
S3 reconciliation can list/check configured prefixes or mock equivalent;
unmapped S3 objects are not made runnable silently;
one known S3 source artifact can be registered as pending without reading business JSON;
run_due_tasks can launch that S3 artifact through mock or real n8n with empty body + pointer headers;
valid manifest registers output pointers with entity_id/artifact_id separation;
invalid route/manifest blocks or fails safely;
source state and child states update correctly;
operator-facing detail/stage_runs show enough S3 pointer information;
local mode still passes tests;
docs explain how n8n workflow must use Beehive headers instead of Search bucket;
feedback honestly states real-smoke status.
```

## 18. Main output for next stage

The main output of B2 is:

```text
A demonstrable S3+n8n pipeline path where one concrete S3 artifact is selected by Beehive, processed by n8n, and tracked through Beehive as source done plus child output artifact pointers.
```

Expected B3 focus:

```text
operator-friendly S3 configuration UI;
repeatable smoke/demo fixtures;
optional async manifest polling;
controlled batch execution/backpressure;
higher-load scheduling;
real pipeline walkthrough from raw to final stage.
```
