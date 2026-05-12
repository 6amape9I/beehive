# Beehive S3 B1 Feedback

## 1. Что сделано

- Добавлена storage-agnostic domain model: `StorageProvider`, `ArtifactLocation`, `S3StorageConfig`, `StorageConfig`, `StageStorageConfig`.
- `pipeline.yaml` parser расширен optional `storage`, stage `input_uri`, `save_path_aliases`.
- Добавлен S3 route resolver для logical `save_path` / legacy `/main_dir/...` / `s3://bucket/prefix`.
- Добавлен manifest parser/validator для `beehive.s3_artifact_manifest.v1`.
- Добавлен S3 executor branch: empty-body webhook request, S3 pointer headers, manifest validation, output pointer registration.
- SQLite schema поднята до v5 с явной S3 metadata в `stages` и `entity_files`.
- Local mode сохранён через старые `input_folder`, local scan, payload-only local executor и local `save_path` resolver.
- Добавлены docs: `docs/s3_control_plane_architecture.md`, `docs/s3_n8n_contract.md`.

## 2. Изменённые файлы

- `src-tauri/src/domain/mod.rs`
- `src-tauri/src/config/mod.rs`
- `src-tauri/src/save_path.rs`
- `src-tauri/src/s3_manifest.rs`
- `src-tauri/src/executor/mod.rs`
- `src-tauri/src/database/mod.rs`
- `src-tauri/src/database/entities.rs`
- `src-tauri/src/discovery/mod.rs`
- `src-tauri/src/file_ops/mod.rs`
- `src-tauri/src/file_open/mod.rs`
- `src-tauri/src/dashboard/mod.rs`
- `src-tauri/src/pipeline_editor/mod.rs`
- `src-tauri/src/lib.rs`
- `src/types/domain.ts`
- `docs/beehive_s3_b1_plan.md`
- `docs/s3_control_plane_architecture.md`
- `docs/s3_n8n_contract.md`
- `docs/beehive_s3_b1_feedback.md`

## 3. Schema/config changes

- `PipelineConfig.storage: Option<StorageConfig>`.
- `StageDefinition.input_uri: Option<String>`.
- `StageDefinition.save_path_aliases: Vec<String>`.
- SQLite schema v5:
  - `stages.input_uri`;
  - `stages.save_path_aliases_json`;
  - `entity_files.storage_provider`;
  - `entity_files.bucket`;
  - `entity_files.object_key`;
  - `entity_files.version_id`;
  - `entity_files.etag`;
  - `entity_files.checksum_sha256`;
  - `entity_files.artifact_size`;
  - `entity_files.producer_run_id`.

## 4. Как local mode сохранён

Absent `storage` still means local mode. Old stages with `input_folder` still parse and sync. `Scan workspace` still registers local files as `storage_provider = local`. Existing local executor still sends payload-only JSON body and still uses local `file_ops` for next-stage copies.

## 5. Как S3 artifact location представлена

S3 artifacts are stored as explicit pointers: `storage_provider = s3`, `bucket`, `object_key`, optional `version_id`, `etag`, `checksum_sha256`, `artifact_size`, and `producer_run_id`. The compatibility `file_path` is a display/key identity like `s3://bucket/key`, but execution uses provider metadata, not local filesystem reads.

## 6. Как S3 route resolver работает

`resolve_s3_save_path_route` resolves `save_path` through active stage `input_uri`, `input_folder` compatibility, and `save_path_aliases`. It accepts logical routes, legacy `/main_dir/...`, and matching `s3://bucket/prefix`. It rejects unsafe, unknown, wrong-bucket, and ambiguous routes.

## 7. Как manifest parser работает

`src-tauri/src/s3_manifest.rs` parses manifest JSON, checks schema, workspace, run id, claimed source bucket/key, success/error status, error fields, business-payload leakage, output bucket, route resolution, and output key prefix. Route failures return `BlockedRoute`; malformed manifests return `Invalid`.

## 8. Как S3-mode executor запускает n8n

When the source file row has `storage_provider = s3`, executor skips local file preflight and starts S3 mode. The webhook request has empty body, `Content-Type: application/octet-stream`, `Accept: application/json`, and `X-Beehive-*` pointer headers. `stage_runs.request_json` stores a technical audit envelope only.

## 9. Доказательство, что business JSON не отправляется

Added Rust mock test `s3_mode_sends_empty_body_headers_and_registers_output_pointer` asserts:

- captured request body is empty;
- source bucket/key are present in headers;
- `stage_runs.request_json` contains `s3_artifact_pointer`;
- `stage_runs.request_json` does not contain local business text like `hello beehive`.

This test was executed successfully as part of the final `cargo test` run.

## 10. Как output artifact pointers регистрируются

Valid manifest outputs call `register_s3_artifact_pointer`. It creates/updates `entity_files` with S3 metadata, creates pending `entity_stage_states` for the resolved target stage, links `copy_source_file_id`, and stores `producer_run_id`.

## 11. Tests added/updated

- Config parsing tests for S3 config, missing bucket, invalid input URI, unsafe aliases.
- S3 route resolver tests for logical, legacy, S3 URI, unknown bucket/prefix, unsafe paths, ambiguous aliases.
- S3 manifest tests for valid success, valid error, wrong schema, run/source mismatch, bucket mismatch, payload leakage.
- S3 executor mock tests for empty body/headers, output pointer registration, error manifest failure, invalid save_path blocked, terminal no-output success.
- Local test helpers updated with explicit `storage: None`, `input_uri: None`, and empty `save_path_aliases`.
- Legacy v2→v5 migration test covered the S3 column migration path.
- Local payload-only executor test now checks forbidden Beehive metadata keys without rejecting legitimate business text values that contain the word `beehive`.

## 12. Commands run and exact results

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
```

Result: passed.

Earlier in the session this command failed with `cargo: command not found`, but
after the environment was fixed it completed successfully with no output.

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Result: passed.

Earlier in the session this command failed with `cargo: command not found`,
then with missing Linux Tauri `.pc` files. After the required dependencies were
installed, the final run completed:

```text
test result: ok. 110 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.15s
```

```bash
npm run build
```

Result: passed.

```text
tsc && vite build
86 modules transformed.
✓ built in 728ms
```

```bash
git diff --check
```

Result: passed with no output.

## 13. Что не удалось проверить

All required commands were verified successfully. No remaining verification
blocker is known for B1.

## 14. Ubuntu compatibility notes

The S3 route logic uses slash-separated logical paths and does not rely on
Windows commands. Verification on this Ubuntu-like shell passed after the Tauri
Linux development packages were installed.

## 15. Windows compatibility notes

Windows drive paths and UNC paths are explicitly rejected by route validation.
Windows verification still requires `cargo test` and `npm.cmd run build` from an
environment with Rust/MSVC tools.

## 16. Риски

- Stage Editor is still primarily local-mode UI; it preserves storage fields in the draft model, but it is not a full S3 config editor.
- S3 output registration is pointer-only and does not reconcile real S3 object existence.
- Multi-output S3 registration is validated before registration, but DB registration itself is not yet a single all-output transaction.

## 17. Что передать B2

Главный output следующего этапа:
B2 real S3 reconciliation and one-artifact n8n smoke pipeline.

B2 should add real S3 list/metadata reconciliation, manual registration of existing S3 artifacts, optional manifest polling if n8n is async, and a real one-artifact smoke run through n8n.

## 18. ТЗ reread checkpoints

ТЗ перечитано на этапах: after_plan, after_config_model, after_route_resolver, after_manifest_model, after_executor_s3_mode, after_tests, before_feedback
