# B3. Operator S3 Pipeline Console, n8n Workflow Governance, and Controlled Batch Smoke

## 0. Mission

You are Codex agent working on Beehive after B2.2.

Assume B2.2 cleanup/hardening was completed successfully before you start. In particular, assume the repository no longer contains unnecessary full smoke datasets, old header-based workflow fixtures are removed or clearly deprecated, and the JSON-body S3 control-envelope contract is the active production contract.

Your mission for B3 is to turn the successful one-artifact S3+n8n proof into an operator-friendly, repeatable workflow.

Main B3 outcome:

```text
An operator can configure/inspect an S3 pipeline, reconcile S3 source artifacts, manually register a source artifact if needed, run a small controlled batch through n8n, and verify lineage/status/output pointers from the Beehive UI and docs.
```

B3 is not a high-load scheduler. B3 is not a background daemon. B3 is not n8n workflow management through the n8n REST API. B3 is the first operator-grade usability layer on top of the S3 control plane.

## 1. Strategic decisions that must not be changed

Beehive remains the control plane.

n8n remains the data plane.

S3 remains the artifact store.

In S3 execution mode, Beehive must send a JSON technical control envelope body:

```text
POST workflow_url
Content-Type: application/json; charset=utf-8
Accept: application/json
body.schema = beehive.s3_control_envelope.v1
```

Do not reintroduce `X-Beehive-Source-Key` or other source-key headers. Real S3 keys may contain Cyrillic and other non-ASCII characters. `source_key` must be carried in the UTF-8 JSON body.

The control envelope is technical metadata only. It must not contain business JSON, article blocks, raw payload, `payload_json`, `raw_article`, or source document content.

## 2. Read first

Before writing code, read:

```text
docs/beehive_s3_b2_2_feedback.md
docs/s3_n8n_contract.md
docs/n8n_s3_pointer_workflow_adapter.md
docs/s3_control_plane_architecture.md
README.md
src-tauri/src/executor/mod.rs
src-tauri/src/s3_reconciliation.rs
src-tauri/src/s3_client.rs
src-tauri/src/commands/mod.rs
src-tauri/src/domain/mod.rs
src/lib/runtimeApi.ts
src/pages/WorkspaceExplorerPage.tsx
src/pages/StageEditorPage.tsx
src/components/stage-editor/*
```

Also inspect current workflow fixtures in the repository. Your goal is not to build a large JS program inside n8n. n8n workflow fixtures and docs should prefer n8n-native nodes such as Webhook, Edit Fields/Set, IF, Switch, Merge, Aggregate, Split Out, Extract From File, Convert to File, S3, Postgres, HTTP Request, and Respond to Webhook. Code nodes are allowed only when a native n8n node cannot reasonably perform the operation, such as creating binary file data for S3 upload or parsing a provider response that cannot be emitted as structured JSON.

## 3. Required plan before code

Create:

```text
docs/beehive_s3_b3_plan.md
```

Do not edit runtime code before the plan exists.

The plan must include:

```text
1. Current B2.2 readiness status.
2. Which UI/operator surfaces will be added or changed.
3. How S3 reconciliation will be exposed to an operator.
4. How manual S3 source registration will be exposed.
5. How controlled batch execution will work.
6. How B3 will verify JSON-body contract and Cyrillic S3 keys.
7. What n8n workflow governance/docs/linting will be added.
8. Tests to add or update.
9. Exact commands to run.
10. What B3 will not implement.
11. Risks and rollback considerations.
```

## 4. Required reread checkpoints

Reread this instruction at checkpoints:

```text
after_plan
after_operator_ui_design
after_s3_reconcile_ui
after_manual_registration_ui
after_batch_smoke_runner
after_n8n_governance
after_tests
before_feedback
```

Feedback must contain exactly:

```text
ТЗ перечитано на этапах: after_plan, after_operator_ui_design, after_s3_reconcile_ui, after_manual_registration_ui, after_batch_smoke_runner, after_n8n_governance, after_tests, before_feedback
```

## 5. B3 task A — Operator S3 controls in UI/API

Expose existing S3 backend capabilities to an operator.

Minimum UI/API behavior:

```text
1. Operator can run S3 reconciliation from the app.
2. Operator can see reconciliation summary counts.
3. Operator can manually register one S3 source artifact.
4. Operator can run a small controlled batch using existing run_due_tasks semantics.
5. Operator can inspect S3 artifact metadata in Workspace Explorer / Entity Detail.
```

### 5.1 S3 reconciliation action

Use the existing backend/Tauri command if present:

```text
reconcile_s3_workspace
```

Surface the result in the UI. Show at least:

```text
stage_count
listed_object_count
metadata_tagged_count
registered_file_count
updated_file_count
unchanged_file_count
missing_file_count
restored_file_count
unmapped_object_count
elapsed_ms
latest_reconciliation_at
```

Show errors clearly. Do not expose credentials.

### 5.2 Manual S3 source registration UI

Use the existing backend/Tauri command if present:

```text
register_s3_source_artifact
```

Add a minimal operator form or diagnostics action that accepts:

```text
stage_id
entity_id
artifact_id
bucket
key
version_id optional
etag optional
checksum_sha256 optional
size optional
```

Validation expectations:

```text
stage must exist and be active;
stage must be S3-capable;
bucket/key must match the stage input_uri prefix;
duplicate same artifact/location is idempotent;
conflicting duplicate is rejected;
no S3 object body is read.
```

The form can be intentionally simple. This is not full S3 browser UI.

### 5.3 S3 artifact visibility

Workspace Explorer and Entity Detail must clearly display S3 artifact pointers:

```text
storage_provider
bucket
key
artifact_id
relation_to_source
producer_run_id
file_exists/missing_since
stage_id
runtime status
```

For S3 rows, local `Open file` / `Open folder` must remain disabled or clearly unavailable. Add a safe copy-to-clipboard affordance for S3 URI if frontend conventions allow it. Do not fetch or render business JSON bodies in B3.

### 5.4 Stage Editor visibility

Stage Editor should preserve and expose S3-relevant fields:

```text
storage.provider
storage.bucket
storage.workspace_prefix
storage.region
storage.endpoint
stage.input_uri
stage.save_path_aliases
stage.allow_empty_outputs
```

If full editing is too large for B3, it must at least preserve these fields and show them read-only with clear messaging.

## 6. B3 task B — Controlled batch smoke

B2.2 proved one artifact. B3 should prove a small controlled batch without pretending to be high-load.

Add an ignored/opt-in Rust test or helper command named approximately:

```text
real_s3_n8n_smoke_batch_small
```

It should be ignored by default and require real environment variables.

Suggested behavior:

```text
1. Use existing smoke workdir or create an isolated temp smoke workdir.
2. Use storage.provider=s3 and JSON-body n8n webhook URL.
3. Reconcile S3 source prefix or manually register N known sources.
4. Run at most 3 to 5 pending source artifacts.
5. Verify each attempted source gets a terminal state: done, retry_wait, failed, or blocked.
6. For successful sources, verify one child S3 pointer exists on the target stage.
7. For successful sources, optionally head/list the S3 output key to confirm existence.
8. Print run_id, source key, output key, source state, child state.
```

The batch smoke should not run in normal `cargo test`. It must be clearly opt-in.

Use a variable such as:

```text
BEEHIVE_REAL_S3_BATCH_SMOKE=1
BEEHIVE_N8N_SMOKE_WEBHOOK=...
BEEHIVE_SMOKE_PREFIX=...
BEEHIVE_SMOKE_BATCH_LIMIT=3
```

B3 acceptance requires either a real batch smoke pass or a precise blocker. If the real n8n endpoint is unavailable, keep mock coverage and document the exact blocker.

## 7. B3 task C — JSON-body contract hardening

Centralize and harden the S3 control-envelope contract.

Required checks:

```text
1. There is one canonical schema string: beehive.s3_control_envelope.v1.
2. S3 execution request body includes all required fields.
3. source_key with Cyrillic is preserved as UTF-8 body JSON.
4. X-Beehive-Source-Key is absent from S3 execution requests.
5. stage_runs.request_json stores the exact technical envelope.
6. business payload/source document body is absent from request_json and HTTP body.
7. source_entity_id and source_artifact_id come from Beehive DB/S3 registration, not from filename guessing.
8. target_prefix/save_path are resolved from configured target stage when possible.
```

If the current implementation duplicates envelope-building logic, refactor it into a small helper or module and test it.

Do not change local-mode payload-only behavior unless a test requires a compatibility fix.

## 8. B3 task D — n8n workflow governance

Create:

```text
docs/n8n_workflow_authoring_standard.md
```

This document must explain how Beehive-compatible n8n workflows should be authored.

It must include:

```text
1. S3 mode receives a JSON control envelope body.
2. n8n must download exactly source_bucket/source_key from body.
3. n8n must not use Search Bucket/List Bucket as production source selection.
4. n8n must upload outputs before returning manifest.
5. n8n must return beehive.s3_artifact_manifest.v1 synchronously unless async mode is explicitly implemented later.
6. output manifest requires artifact_id, entity_id, relation_to_source, bucket, key, save_path.
7. save_path must match active Beehive S3 route/prefix.
8. Code nodes are discouraged and allowed only for justified operations.
```

### 8.1 Workflow fixture policy

Move workflow examples into a clear docs or fixtures location, for example:

```text
docs/n8n_workflows/
```

Keep only body-JSON workflow fixtures as active examples. Header-based workflows must be removed from active examples or renamed with `deprecated_header_mode` and documented as not production-safe.

If the repository currently contains old smoke kit artifacts, full datasets, or zip files, do not re-add them. B3 should keep examples small and reproducible.

### 8.2 Optional workflow linter

If feasible in B3, add a simple script such as:

```text
scripts/lint_n8n_workflows.py
```

It should scan workflow JSON fixtures and warn or fail on:

```text
X-Beehive-Source-Key usage;
Search Bucket/List Bucket connected to production Webhook path;
legacy typo paths such as /main_dir/pocessed;
absolute local-looking save_path outside allowed legacy/workspace forms;
more than a small number of Code nodes without allowlist comments;
webhook nodes without POST/responseNode when meant for Beehive execution.
```

If implemented, add tests for the linter. If not implemented, document why and include it as B4 work.

## 9. B3 task E — Operator runbook

Create or update:

```text
docs/s3_operator_runbook.md
```

It must give a human a concrete sequence:

```text
1. Configure S3 env variables without committing secrets.
2. Import/use a body-JSON n8n workflow.
3. Configure pipeline.yaml storage and stage input_uri values.
4. Run S3 reconciliation in Beehive.
5. Manually register one source artifact if reconciliation metadata is absent.
6. Run due tasks for a small batch.
7. Verify source done / child pending in UI.
8. Verify stage_runs request_json/response_json.
9. Verify output object exists in S3.
10. Reset/retry/skip failed or blocked tasks.
```

Include troubleshooting for:

```text
HTTP 404 n8n webhook;
manifest_invalid;
manifest_blocked / save_path route mismatch;
missing artifact_id;
S3 credentials missing;
S3 object unmapped;
Cyrillic source_key problems;
old header workflow accidentally used.
```

## 10. What B3 must not do

Do not:

```text
implement a background daemon;
implement high-load worker pools;
implement async manifest polling unless it is already trivial and isolated;
implement n8n REST API workflow editing;
read S3 business JSON in Beehive execution path;
send business JSON to n8n;
commit secrets;
commit large smoke datasets or zip archives;
make arbitrary S3 objects runnable without identity metadata or manual registration;
make Search Bucket/List Bucket the production source selector;
reintroduce source_key headers.
```

## 11. Tests required

Add or update tests where appropriate:

### 11.1 Rust/backend

```text
cargo test --manifest-path src-tauri/Cargo.toml
```

Required coverage:

```text
S3 control envelope builder emits JSON body fields;
Cyrillic source_key remains intact;
source_key is not sent as a header;
missing source artifact_id blocks before n8n call;
S3 reconciliation/manual registration tests still pass;
small-batch helper is ignored by default;
local mode tests still pass.
```

### 11.2 Frontend

```text
npm run build
```

If frontend tests exist, run them. If none exist, TypeScript/Vite build is the minimum.

### 11.3 n8n workflow linting if implemented

```bash
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
python3 -m unittest discover -s tests -p '*n8n*'
```

### 11.4 Real batch smoke

Only if credentials and n8n endpoint are available:

```bash
BEEHIVE_REAL_S3_BATCH_SMOKE=1 \
BEEHIVE_SMOKE_BATCH_LIMIT=3 \
cargo test --manifest-path src-tauri/Cargo.toml real_s3_n8n_smoke_batch_small -- --ignored --nocapture
```

Do not claim it passed unless it actually ran and printed concrete run evidence.

## 12. Verification commands

Run at minimum:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
git diff --check
```

If a Python workflow linter is added:

```bash
python3 -m py_compile scripts/lint_n8n_workflows.py
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
```

If real batch smoke is available, run the ignored smoke command and record exact evidence.

## 13. Required feedback

Create:

```text
docs/beehive_s3_b3_feedback.md
```

Feedback must include:

```text
1. B2.2 readiness result.
2. What was implemented.
3. Files changed.
4. Operator UI/API surfaces added.
5. S3 reconciliation UI behavior.
6. Manual registration UI behavior.
7. S3 artifact visibility behavior.
8. Controlled batch smoke status.
9. Whether real S3 was contacted.
10. Whether real n8n was contacted.
11. n8n workflow governance changes.
12. Workflow fixtures added/removed/deprecated.
13. Commands run and exact results.
14. Tests passed/failed/ignored.
15. What could not be verified.
16. Ubuntu notes.
17. Windows notes.
18. Remaining risks.
19. What should be done in B4.
20. ТЗ reread checkpoints.
```

If real batch smoke ran, include:

```text
batch_limit
source keys attempted
run_ids
success/retry/failed/blocked counts
output keys created
source states
child states
S3 output existence check
```

If real batch smoke did not run, include exact blocker:

```text
missing credentials
missing n8n endpoint
workflow not imported
S3 route mismatch
network sandbox
other
```

## 14. Acceptance criteria

B3 is acceptable if:

```text
1. The app exposes S3 reconciliation to an operator.
2. The app exposes manual S3 source registration or a clearly documented equivalent operator path.
3. The app visibly distinguishes S3 pointers from local files.
4. Stage Editor preserves or exposes S3 fields.
5. JSON-body control envelope remains the only production S3 n8n request contract.
6. Tests prove Cyrillic source_key is in body and not headers.
7. n8n authoring standard exists.
8. Active workflow fixtures use body JSON contract, not source-key headers.
9. A small controlled batch smoke is implemented as opt-in, or a precise blocker is documented.
10. cargo fmt, cargo test, npm run build, and git diff --check pass or failures are honestly reported.
11. No secrets or large smoke datasets are committed.
```

## 15. Main output for the next stage

B3 should prepare B4.

Expected B4 focus:

```text
controlled repeated execution / backpressure;
optional async manifest polling;
better S3 operator UI polish;
real multi-stage S3 pipeline walkthrough;
first production-style run over a larger subset, after B3 proves operator workflow and small-batch behavior.
```
