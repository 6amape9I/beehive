# Beehive S3 B2 Feedback

## 1. B1.1 readiness result

`docs/beehive_s3_b1_1_feedback.md` contains the required exact line:

```text
B2 readiness: ready
```

B2 runtime work was allowed to proceed.

## 2. What was implemented

- Real S3 metadata client abstraction and AWS SDK adapter.
- `reconcile_s3_workspace` backend/Tauri command.
- `register_s3_source_artifact` backend/Tauri command.
- S3 reconciliation events for discovered, updated, restored, missing, unmapped, and completed scans.
- TS domain/API bindings for the new commands.
- n8n S3 pointer adapter documentation and B2 updates to the S3 architecture/contract docs.
- Mock S3 reconciliation/manual-registration tests.

## 3. Files changed

- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock`
- `src-tauri/src/lib.rs`
- `src-tauri/src/domain/mod.rs`
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/database/mod.rs`
- `src-tauri/src/executor/mod.rs`
- `src-tauri/src/s3_client.rs`
- `src-tauri/src/s3_reconciliation.rs`
- `src/types/domain.ts`
- `src/lib/runtimeApi.ts`
- `docs/beehive_s3_b1_1_feedback.md`
- `docs/beehive_s3_b2_plan.md`
- `docs/n8n_s3_pointer_workflow_adapter.md`
- `docs/s3_control_plane_architecture.md`
- `docs/s3_n8n_contract.md`
- `docs/beehive_s3_b2_feedback.md`

## 4. S3 client/credential strategy

The real client uses the official AWS Rust SDK (`aws-config`, `aws-sdk-s3`, `aws-credential-types`) behind a small Beehive `S3MetadataClient` trait.

Credential/config resolution:

- region: `storage.region`, then `BEEHIVE_S3_REGION`, `AWS_REGION`, `S3_REGION`;
- endpoint: `storage.endpoint`, then `BEEHIVE_S3_ENDPOINT`, `S3_HOST`;
- credentials: standard AWS env/chain, with explicit local aliases `S3_KEY` and `S3_SEC_KEY`;
- optional token: `AWS_SESSION_TOKEN`.

The adapter uses path-style requests for S3-compatible endpoints. Secrets are not hardcoded, committed, or printed.

## 5. S3 reconciliation behavior

`reconcile_s3_workspace`:

- loads active S3 stages from SQLite;
- lists objects under each stage `input_uri` bucket/prefix;
- heads objects for S3 user metadata;
- registers only objects with Beehive identity metadata;
- leaves unknown objects unmapped and not runnable;
- marks missing S3 artifacts when absent from the current prefix listing;
- restores missing S3 artifacts when they reappear;
- writes reconciliation settings and `app_events`.

It does not read S3 business JSON bodies.

## 6. Manual source artifact registration behavior

`register_s3_source_artifact` accepts:

```text
stage_id, entity_id, artifact_id, bucket, key, version_id?, etag?, checksum_sha256?, size?
```

It validates that the target stage is active, S3-capable, and owns the provided key prefix. It creates a pending S3 pointer without reading the object body. Re-registering the same object is idempotent. A conflicting `artifact_id` in the same stage is rejected.

## 7. n8n pointer workflow adapter summary

`docs/n8n_s3_pointer_workflow_adapter.md` documents the production adapter:

```text
Webhook -> read Beehive S3 pointer headers -> download exactly that object -> transform -> upload outputs -> return synchronous manifest
```

It explicitly forbids Search Bucket/List Bucket as production source selection and requires output manifest fields `artifact_id`, `entity_id`, `relation_to_source`, `bucket`, `key`, and `save_path`.

## 8. One-artifact smoke flow status

Mock smoke status: passed through Rust tests.

Real smoke status: not run
Reason: no S3-enabled workdir/pipeline, known source object, and confirmed n8n pointer-adapter endpoint were provided for an opt-in real smoke run in this session.

## 9. Whether real S3 was contacted

Real S3 contacted: no.

## 10. Whether real n8n was contacted

Real n8n contacted: no.

## 11. Mock tests added/updated

- `s3_client::tests::dotenv_values_preserve_process_env_precedence`
- `s3_client::tests::dotenv_value_trimming_handles_quotes_and_comments`
- `s3_reconciliation::tests::reconciliation_registers_metadata_tagged_objects`
- `s3_reconciliation::tests::reconciliation_records_unmapped_objects_without_registration`
- `s3_reconciliation::tests::reconciliation_marks_missing_and_restored_s3_artifacts`
- `s3_reconciliation::tests::manual_s3_source_registration_validates_stage_prefix_and_conflicts`

Existing executor S3 smoke tests remained green, including empty-body pointer headers, valid manifest registration, invalid route blocking, and empty-output opt-in behavior.

## 12. Commands run and exact results

- `cargo fmt --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo test --manifest-path src-tauri/Cargo.toml`: first sandbox run failed to fetch new AWS SDK dependencies from crates.io; escalated run downloaded dependencies. A later non-escalated test run compiled but failed executor tests on local mock HTTP bind permission. Final escalated run passed: `122 passed; 0 failed`.
- `npm run build`: passed; Vite built 86 modules.
- `git diff --check`: passed.

## 13. What could not be verified

- Real S3 list/head against the user's bucket was not verified.
- Real n8n pointer workflow was not verified.
- End-to-end real S3 source object to real n8n to S3 output artifact was not verified.

## 14. Ubuntu notes

Rust/Tauri build now pulls the AWS SDK dependency tree. The local machine needed the native GTK/WebKit dependencies already installed before this B2 pass. Executor tests that bind local mock HTTP servers may require running outside strict network sandboxing.

## 15. Windows notes

S3 `save_path` remains logical and must not use Windows drive paths or UNC paths. AWS credentials should be supplied through standard environment variables or the supported aliases; do not place secrets in tracked files.

## 16. Remaining risks

- S3-compatible endpoint behavior may still need endpoint-specific tuning.
- Real smoke depends on an n8n workflow that obeys the B2 pointer contract.
- Objects without Beehive metadata require manual registration or a future seed manifest flow.
- Reconciliation is metadata/list based; it does not validate object body JSON.

## 17. What should be done in B3

- Add an operator-facing S3 reconciliation/manual-registration UI.
- Add an opt-in real smoke script or ignored test once a source object and pointer-adapter n8n URL are agreed.
- Consider seed manifest ingestion if metadata tagging is inconvenient.
- Consider async manifest polling if n8n needs long-running workflows.
- Add credential diagnostics that report presence/shape without revealing secret values.

## 18. TZ reread checkpoints

ТЗ перечитано на этапах: after_plan, after_s3_client_setup, after_s3_reconciliation, after_manual_registration, after_smoke_runner, after_n8n_adapter_docs, after_tests, before_feedback

B3 readiness: ready for UI/operator workflow hardening; real smoke remains pending until a concrete S3 source object and n8n pointer-adapter endpoint are supplied.
