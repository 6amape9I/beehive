# Beehive S3 B2.2 Plan

## 1. Contract Change

Replace S3-mode n8n trigger requests from empty body plus `X-Beehive-*` headers to a JSON technical control envelope body.

The body remains control-plane metadata only. It must not contain business JSON, content blocks, source payload text, or document bodies.

## 2. Backend Work

- Add a typed S3 control envelope in `src-tauri/src/executor/mod.rs`.
- Send `Content-Type: application/json` and `Accept: application/json`.
- Keep `stage_runs.request_json` equal to the exact envelope sent to n8n.
- Include `source_entity_id` and `source_artifact_id`.
- Preserve local mode payload-only behavior.

## 3. Tests

- Update S3 mock smoke tests to capture JSON body instead of header-only pointer mode.
- Assert Cyrillic `source_key` is preserved in request body.
- Assert source identity fields exist.
- Assert business payload JSON is absent from the control envelope.
- Keep valid manifest/output pointer registration coverage.
- Keep local n8n payload-only tests unchanged.

## 4. Docs

Update:

- `docs/s3_n8n_contract.md`
- `docs/n8n_s3_pointer_workflow_adapter.md`
- `docs/s3_control_plane_architecture.md`

They must state that B2.2 uses JSON control envelope body and that headers are deprecated for S3 object keys.

## 5. Verification

Run:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
git diff --check
```

## 6. Checkpoints

- [x] after_plan
- [x] after_backend_contract_change
- [x] after_n8n_workflow_update
- [x] after_tests
- [x] before_feedback
