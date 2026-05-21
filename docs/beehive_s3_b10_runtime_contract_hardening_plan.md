# B10 Runtime Contract Hardening Plan

## Scope

B10 hardens the current S3 runtime contract without changing scheduler shape, auth, storage backend, or long-running timeout behavior.

The available instruction file starts with the output-registration-report shape, then defines stage cardinality, URL decoding, entity routes, tests, smoke, docs, and feedback requirements. This plan treats those visible requirements and acceptance criteria as the active source of truth.

## Plan

1. URL decode design
   - Add strict percent decoding for path segments and query values in the HTTP API.
   - Keep `+` literal in path values.
   - Decode `+` to space in query values.
   - Return HTTP 400 for invalid percent escapes instead of routing malformed identifiers into services.

2. Stage cardinality design
   - Add `allow_zero_outputs` and `allow_multiple_outputs` to config/domain/SQLite/API/TS models.
   - Preserve legacy `allow_empty_outputs` as a deprecated alias for `allow_zero_outputs`.
   - Make the default S3 stage contract exactly one output.
   - Treat zero/many output violations as `manifest_blocked`, not retryable manifest invalid errors.

3. Manifest strictness
   - Keep root manifests strict JSON objects only.
   - Reject arrays, strings, `body` wrappers, and business payload fields.
   - Keep source bucket/key literal matching so URL-encoded source keys are rejected clearly.

4. Partial output registration
   - Stop treating one output conflict as a whole-run retry.
   - Validate manifest-level shape and duplicate artifact ids before registration.
   - Register valid sibling outputs where possible.
   - Report each output as `registered`, `idempotent_skipped`, `invalid`, `conflict`, or `failed`.
   - Store the report in `app_events` with code `output_registration_report`, preserving original n8n response manifest in `stage_runs.response_json`.
   - Mark source `done` when at least one output is registered or idempotently present.
   - Block, without retry, when no output was registered or idempotently present.

5. UI simplification
   - Replace "Terminal stage" wording with two operator toggles:
     - `Разрешено 0 выходов`
     - `Разрешено несколько выходов`
   - Do not expose `isTerminal` or `next_stage` in the stage CRUD UI.

6. Entity detail routes
   - Ensure direct entity view/edit/delete/restore HTTP routes accept Cyrillic and mixed Unicode IDs.
   - Add route tests for `миллиграмм`, `symptom_Кольца_Кайзера-Флейшера_e74b3ffa92f0`, and related decoded paths.

7. Smoke and docs
   - Add or update contract smoke. If a realistic mock webhook is more reliable in Rust tests, document that choice.
   - Add `docs/n8n_s3_manifest_contract.md`.
   - Update `docs/operator_entities_upload_runbook.md`.
   - Create final B10 feedback with command results and the required checkpoint line.

## Verification Targets

- `cargo fmt --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `npm run build`
- `VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build`
- `python3 scripts/lint_n8n_workflows.py docs/n8n_workflows`
- `rg "@tauri-apps/api/core|invoke\\(" src -n`
- `git diff --check`

## Checkpoints

ТЗ перечитано на этапах: after_plan, after_url_decode_design, after_cardinality_design, after_partial_output_design, after_backend_runtime_changes, after_ui_changes, after_tests, after_smoke, before_feedback
