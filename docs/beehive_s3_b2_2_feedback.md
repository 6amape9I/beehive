# Beehive S3 B2.2 Feedback

ТЗ перечитано на этапах: after_plan, after_backend_contract_change, after_n8n_workflow_update, after_tests, before_feedback

## 1. Contract Change

B2.2 changes the S3-mode n8n trigger from an empty POST body with `X-Beehive-*` pointer headers to a JSON technical control envelope body.

New request mode:

```text
POST workflow_url
Content-Type: application/json
Accept: application/json
```

The request body schema is `beehive.s3_control_envelope.v1` and includes workspace, run, stage, source bucket/key, source entity/artifact identity, manifest prefix, workspace prefix, target prefix, and save path.

The envelope is technical metadata, not business JSON. It does not include source document text, content blocks, `payload_json`, or `raw_article`.

## 2. Why JSON Body Replaced Headers

Real S3 object keys can contain Cyrillic and other non-ASCII characters. Moving `source_key` out of HTTP headers and into a UTF-8 JSON body removes header encoding ambiguity and gives n8n one structured control object to read.

The older empty-body plus `X-Beehive-*` header mode is deprecated for S3 object keys.

## 3. Files Changed

- `src-tauri/src/executor/mod.rs`
- `src-tauri/src/save_path.rs`
- `src-tauri/src/s3_reconciliation.rs`
- `docs/s3_n8n_contract.md`
- `docs/n8n_s3_pointer_workflow_adapter.md`
- `docs/s3_control_plane_architecture.md`
- `docs/beehive_s3_b2_2_plan.md`
- `docs/beehive_s3_b2_2_feedback.md`
- `Beehive_S3_Pointer_Smoke_Adapter_BODY_JSON.json`

## 4. Tests Added Or Updated

Updated S3 executor mock tests now read the request body as JSON instead of reading `X-Beehive-*` headers.

The primary S3 test is now:

```text
executor::tests::s3_mode_sends_json_control_body_and_registers_output_pointer
```

It asserts:

- `Content-Type: application/json`
- `schema: beehive.s3_control_envelope.v1`
- Cyrillic `source_key` is preserved in the request body
- `source_entity_id` and `source_artifact_id` are present
- `X-Beehive-Source-Key` is absent
- business payload text from `payload_json` is absent from the request body and `stage_runs.request_json`
- valid S3 manifest output is still registered as an output pointer

Related S3 manifest tests were updated to use the JSON control body for `run_id` and `source_key`.

S3 save path routing was also tightened so S3 `save_path_aliases` are normalized through the S3-aware route parser. This supports aliases such as `/beehive-smoke/test_workflow/processed` and `s3://steos-s3-data/beehive-smoke/test_workflow/processed` without treating them as local OS paths.

Local payload-only n8n tests remained unchanged and pass in the full suite.

## 5. Commands Run

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
```

Result: passed.

```bash
cargo test --manifest-path src-tauri/Cargo.toml s3_mode_sends_json_control_body_and_registers_output_pointer -- --nocapture
```

Result: passed. `1 passed; 0 failed; 122 filtered out`.

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Result: passed. `122 passed; 0 failed; 1 ignored`.

The ignored test is the real S3+n8n smoke test and remains ignored by design.

```bash
npm run build
```

Result: passed. TypeScript and Vite production build completed.

```bash
git diff --check
```

Result: passed.

After `BEEHIVE_N8N_SMOKE_WEBHOOK` was updated to the BODY_JSON production webhook, these additional checks were run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml save_path::tests::resolves_s3_routes_from_logical_legacy_and_s3_uri_values -- --nocapture
```

Result: passed. `1 passed; 0 failed`.

```bash
cargo test --manifest-path src-tauri/Cargo.toml s3_mode_sends_json_control_body_and_registers_output_pointer -- --nocapture
```

Result: passed. `1 passed; 0 failed`.

```bash
cargo test --manifest-path src-tauri/Cargo.toml real_s3_n8n_smoke_one_artifact -- --ignored --nocapture
```

Result: passed. `1 passed; 0 failed`.

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Result: passed. `122 passed; 0 failed; 1 ignored`.

## 6. Cyrillic Source Key Mock Test

Passed.

The mock S3 test sends:

```text
main_dir/raw/smoke_entity_001__порфирия.json
```

The request body contains the same Cyrillic key, and the manifest validation still registers the returned output pointer.

## 7. Real n8n/S3 Smoke

Passed after `BEEHIVE_N8N_SMOKE_WEBHOOK` was updated to:

```text
https://n8n-dev.steos.io/webhook/beehive-s3-pointer-smoke-body
```

Smoke evidence:

- source artifact key: `beehive-smoke/test_workflow/raw/smoke_entity_002__миосаркома-желчного-пузыря.json`
- run_id: `43d8f54b-e7f2-4937-a6c3-ed669188c969`
- stage_run success: `true`
- output artifact key: `beehive-smoke/test_workflow/processed/smoke-output-43d8f54b-e7f2-4937-a6c3-ed669188c969.json`
- source state: `done`
- child state: `pending`
- S3 output exists: `true`, size `1591`

SQLite evidence:

- `stage_runs`: `success=1`, `http_status=200`, `error_type=null`
- `entity_stage_states`: source `smoke_source=done`, child `smoke_processed=pending`
- `entity_files`: child pointer registered on `smoke_processed` with `producer_run_id=43d8f54b-e7f2-4937-a6c3-ed669188c969`

## 8. Real Smoke Blocker

No current blocker.

An intermediate run reached n8n and returned HTTP 200, but Beehive blocked the manifest with:

```text
save_path must not be an absolute OS path.
```

Root cause: S3 `save_path_aliases` were normalized with the local path parser, so aliases such as `/beehive-smoke/test_workflow/processed` and `s3://steos-s3-data/beehive-smoke/test_workflow/processed` could fail route validation. This was fixed in `src-tauri/src/save_path.rs`, and the real smoke then passed.

## 9. Remaining Risks

- One previous failed source, `smoke_entity_001`, remains in the smoke workdir history from the earlier 404/route-validation attempts.
- The real smoke test remains `#[ignore]` by design because it depends on real S3, credentials, and n8n availability.
- Additional real smoke runs will keep consuming the next pending smoke source unless the `/tmp/beehive_s3_smoke_workdir/app.db` state is reset.

## 10. Next Step

Use the successful B2.2 smoke evidence above as the baseline proof, then decide whether to reset `/tmp/beehive_s3_smoke_workdir` before running more one-artifact smoke checks.
