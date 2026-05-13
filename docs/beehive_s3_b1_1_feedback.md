# Beehive S3 B1.1 Feedback

## Summary

B1.1 runtime hardening is implemented.

- Added explicit `allow_empty_outputs` across config, draft, SQLite schema v6, Rust domain, and TypeScript domain.
- S3 success manifests with zero outputs now require `allow_empty_outputs: true`; the default is false.
- Manifest outputs now require separate `artifact_id`, `entity_id`, and `relation_to_source`.
- S3 pointer registration now validates all outputs before mutation and registers them in one SQLite transaction.
- Registration replay is idempotent for the same run/artifact/location and rejects conflicts.
- Stage Editor preserves and exposes `input_uri`, `save_path_aliases`, and `allow_empty_outputs`.
- S3 stages with empty `input_folder` do not create local input directories.
- Workspace Explorer exposes S3 pointer metadata and does not offer local file/folder open actions for S3 rows.

## Verification

```text
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
```

Rust tests were run with localhost permission because executor tests bind mock HTTP servers.

## B2 Readiness

Ready for B2. Remaining B2 work is real S3 reconciliation/smoke execution and any S3 credentials/runtime integration beyond the current control-plane pointer contract.

ТЗ перечитано на этапах: after_plan, after_empty_output_policy, after_entity_artifact_identity, after_registration_transaction, after_stage_editor_docs_visibility, after_tests, before_feedback
