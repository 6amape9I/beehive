# Beehive S3 B2 Plan

## 1. B1.1 readiness status

`docs/beehive_s3_b1_1_feedback.md` says `B2 readiness: ready`. B2 may proceed.

## 2. S3 client choice and why

Use the official AWS Rust SDK for the real implementation and a small Beehive-owned trait for tests:

- `S3MetadataClient` trait for list/head metadata operations;
- `AwsS3MetadataClient` for real S3/S3-compatible endpoints;
- mock client structs in Rust tests.

This avoids hand-rolled AWS Signature V4 and keeps reconciliation testable without secrets.

## 3. Credential/env strategy

Do not hardcode or print secrets.

Resolution order:

- config `storage.region`, then `BEEHIVE_S3_REGION`, `AWS_REGION`, `S3_REGION`;
- config `storage.endpoint`, then `BEEHIVE_S3_ENDPOINT`, `S3_HOST`;
- standard AWS credential chain, with aliases `S3_KEY` and `S3_SEC_KEY` supported for the user's current `.env`;
- optional `AWS_SESSION_TOKEN`.

Load `.env` opportunistically for local development. Missing credentials must not break unit tests.

## 4. Reconciliation design

Add `reconcile_s3_workspace` as a storage-aware command/backend function. It will:

- load active S3 stages from SQLite;
- parse stage `input_uri` into bucket/prefix;
- list objects under each prefix;
- head each object to get metadata;
- register only objects that expose Beehive identity metadata;
- record unmapped objects as events without making them runnable;
- mark previously registered S3 artifacts missing when absent from the current prefix listing;
- restore missing S3 artifacts when they reappear.

Reconciliation is metadata/pointer-based and does not read object bodies.

## 5. Manual source artifact registration design

Add `register_s3_source_artifact` command/backend function with the B2 contract:

```text
workdir_path, stage_id, entity_id, artifact_id, bucket, key,
version_id?, etag?, checksum_sha256?, size?
```

It validates the stage, S3 prefix, and identity fields, then registers a pending S3 pointer without reading business JSON. Optional existence checks can be done by S3 reconciliation or future smoke tooling; manual registration itself remains usable without credentials.

## 6. Real smoke flow design

Keep `run_due_tasks` as the smoke executor. The S3 branch already sends empty body plus pointer headers and validates synchronous manifests.

B2 adds:

- a documented manual flow for real S3/n8n;
- mock smoke tests proving the one-artifact flow without secrets;
- clear feedback about whether real S3/n8n was actually contacted.

## 7. n8n workflow adapter requirements

Create `docs/n8n_s3_pointer_workflow_adapter.md`.

The doc will convert:

```text
Manual Trigger -> Search bucket -> Download file -> Extract from File
```

into:

```text
Webhook -> read X-Beehive-Source-Bucket/Key headers -> download exactly that object -> transform -> upload outputs -> return beehive.s3_artifact_manifest.v1
```

It must forbid Search Bucket/List Bucket as production input selection.

## 8. Tests to add

- Mockable S3 client abstraction.
- S3 reconciliation registers metadata-tagged objects.
- S3 reconciliation records unmapped objects without runnable state.
- S3 reconciliation marks missing/restored S3 artifacts.
- Manual registration creates pending S3 source state.
- Manual registration rejects unknown stage and wrong prefix.
- Manual duplicate registration is idempotent.
- Manual conflicting duplicate is rejected.
- Existing S3 mock n8n tests remain green for empty body and pointer headers.

## 9. Commands to run

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
git diff --check
```

If dependency fetch is needed, run Cargo with approved network/escalated permissions and report exact failures.

## 10. What will not be implemented

- High-load scheduler or worker pool.
- Credential manager UI.
- n8n REST API workflow editing.
- Business JSON reads from S3 during execution.
- Async manifest polling unless trivial after synchronous path.
- S3 browser/editor UI.
- Real smoke claims unless credentials, source object, and endpoint are all available.

## 11. Risks

- Adding the AWS SDK may increase build time and require dependency download.
- S3-compatible endpoint quirks may require path-style/endpoint tuning.
- Object identity depends on S3 user metadata or manual registration; arbitrary unknown objects must stay unmapped.
- Real smoke may be blocked by missing n8n workflow URL, missing credentials, or absent source artifact.

## 12. Execution checklist

- [x] after_plan reread.
- [x] S3 client abstraction and real AWS SDK adapter.
- [x] after_s3_client_setup reread.
- [x] S3 reconciliation implemented.
- [x] after_s3_reconciliation reread.
- [x] Manual source registration implemented.
- [x] after_manual_registration reread.
- [x] Smoke runner/path verified with mock n8n.
- [x] after_smoke_runner reread.
- [x] n8n adapter docs written.
- [x] after_n8n_adapter_docs reread.
- [x] Required commands run.
- [x] after_tests reread.
- [x] Feedback written.
- [x] before_feedback reread recorded.
