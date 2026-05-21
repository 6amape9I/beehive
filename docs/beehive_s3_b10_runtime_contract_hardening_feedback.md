# B10 Runtime Contract Hardening Feedback

## What Changed

- Added strict HTTP URL decoding for workspace/entity/stage routes and query parameters.
- Added `allow_zero_outputs` and `allow_multiple_outputs` across Rust domain/config/API/SQLite/TS models.
- Kept legacy `allow_empty_outputs` as a deprecated alias for `allow_zero_outputs`.
- Enforced success-manifest output cardinality as a non-retryable `manifest_blocked` contract violation.
- Made n8n manifest parsing stricter for root shape, business payload fields, duplicate artifact ids, and URL-encoded source keys.
- Added best-effort S3 output pointer registration: valid siblings are kept when another output conflicts.
- Stored per-output registration details as app event `output_registration_report`.
- Replaced operator UI wording for zero/multiple output settings.
- Fixed Cyrillic entity route decoding and Cyrillic search behavior.
- Added B10 contract smoke with a local mock webhook.

## Files Changed

Backend:

- `src-tauri/src/http_api/mod.rs`
- `src-tauri/src/s3_manifest.rs`
- `src-tauri/src/executor/mod.rs`
- `src-tauri/src/database/mod.rs`
- `src-tauri/src/domain/mod.rs`
- `src-tauri/src/config/mod.rs`
- `src-tauri/src/services/pipeline.rs`
- model/test fixture touch-ups in `dashboard`, `pipeline_editor`, `selected_runner`, `file_ops`, `save_path`, `discovery`, `s3_reconciliation`, `services/*`.

Frontend:

- `src/pages/StageEditorPage.tsx`
- `src/components/stage-editor/StageDraftForm.tsx`
- `src/components/stage-editor/StageDraftList.tsx`
- `src/components/dashboard/StageGraph.tsx`
- `src/types/domain.ts`

Scripts/docs:

- `scripts/web_operator_contract_smoke.mjs`
- `scripts/web_operator_crud_smoke.mjs`
- `scripts/web_operator_entities_smoke.mjs`
- `docs/n8n_s3_manifest_contract.md`
- `docs/operator_entities_upload_runbook.md`
- `docs/s3_n8n_contract.md`
- `docs/stage_creation_s3_ui_contract.md`
- `docs/beehive_s3_b10_runtime_contract_hardening_plan.md`

## URL Decode Behavior

- Path segments are percent-decoded before routing to services.
- Invalid percent escapes return HTTP 400 with `request_url_invalid`.
- `+` stays literal in path segments.
- Query values percent-decode and `+` becomes a space.
- Direct entity GET/PATCH/DELETE/restore now works for Cyrillic IDs such as `symptom_Кольца_Кайзера-Флейшера_e74b3ffa92f0`.
- Entity table search also handles Cyrillic exact-case fragments; this avoids SQLite `LOWER()` Unicode limitations.

## Cardinality Behavior

- Default stage contract is exactly one output.
- Zero outputs require `allow_zero_outputs = true`.
- Multiple outputs require `allow_multiple_outputs = true`.
- Both flags together allow zero, one, or many outputs.
- Violations are blocked with `manifest_blocked`; they are not retried.

## Legacy Migration Behavior

- YAML/API input `allow_empty_outputs: true` still maps to `allow_zero_outputs = true`.
- New serialization/UI uses `allow_zero_outputs`.
- SQLite schema migrated to version 8 with `stages.allow_multiple_outputs`.

## Partial Output Registration

- Beehive validates manifest-level shape first.
- Duplicate `artifact_id` inside one manifest is a manifest contract block.
- During DB registration, each output is reported as `registered`, `idempotent_skipped`, `invalid`, `conflict`, or `failed`.
- If at least one output is registered or idempotently skipped, the source run succeeds and the report is stored in `app_events`.
- If every output is invalid/conflicting/failed, the run is blocked with `manifest_blocked` and is not retried.

## Duplicate And Conflict Behavior

- Idempotent duplicates for the same producer run and same artifact/location do not retry and do not fail the run.
- Conflicts against the same `producer_run_id + artifact_id`, same S3 object, or same `entity_id + target stage` are reported per output.
- A conflicting sibling no longer discards valid sibling outputs.

## Zero And Many Outputs

- Zero outputs with default stage: blocked as `manifest_blocked`.
- Zero outputs with `allow_zero_outputs`: source becomes done.
- Many outputs with default stage: blocked as `manifest_blocked`.
- Many outputs with `allow_multiple_outputs`: outputs are registered normally.

## n8n Manifest Contract

Documented in `docs/n8n_s3_manifest_contract.md`:

- request envelope expectations;
- success/error manifest fields;
- output fields;
- cardinality flags;
- strict forbidden response shapes;
- literal source key requirement;
- output registration report statuses.

## UI Changes

- Stage create/edit now shows:
  - `Разрешено 0 выходов`
  - `Разрешено несколько выходов`
- Old `Terminal stage` wording was removed from the stage operator controls.
- Stage draft list now describes output cardinality instead of terminal mode.
- `next_stage` is not used for the current S3 operator route contract.

## Commands Run

- `cargo fmt --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `npm run build`
- `VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build`
- `python3 scripts/lint_n8n_workflows.py docs/n8n_workflows`
- `rg "@tauri-apps/api/core|invoke\\(" src -n`
- `git diff --check`
- `BEEHIVE_API_BASE_URL=http://127.0.0.1:8789 node scripts/web_operator_contract_smoke.mjs`

## Test Results

- `cargo test --manifest-path src-tauri/Cargo.toml`: passed, 182 passed, 0 failed, 3 ignored.
- `npm run build`: passed.
- `VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build`: passed.
- `python3 scripts/lint_n8n_workflows.py docs/n8n_workflows`: passed.
- `rg "@tauri-apps/api/core|invoke\\(" src -n`: passed, only `src/lib/apiClient/tauriClient.ts` imports/invokes Tauri.
- `git diff --check`: passed.

## Smoke Results

Smoke command:

```bash
BEEHIVE_API_BASE_URL=http://127.0.0.1:8789 node scripts/web_operator_contract_smoke.mjs
```

Result:

```json
{"ok":true,"api_base":"http://127.0.0.1:8789","workspace_id":"b10-contract-1779180456389-44a27e","source_entity_id":"symptom_Кольца_Кайзера-Флейшера_e74b3ffa92f0","run_id":"110332e6-8f21-47a7-a51c-bed43887a00d","registered_outputs":2,"mock_requests":1,"workspace_cleanup":"archived"}
```

Notes:

- The smoke uses `register-s3-source` against a temporary workspace instead of uploading to real S3.
- The mock webhook returns 3 outputs: 2 valid and 1 conflicting sibling.
- The run succeeds with 2 persisted child outputs.
- Direct Cyrillic entity GET/PATCH/DELETE/restore is covered in the smoke.
- `output_registration_report` existence and counts are covered by Rust executor tests because app events are not exposed through the HTTP API.

## Known Risks

- SQLite still does not provide full Unicode case-insensitive search. B10 adds raw `LIKE` fallback so exact Cyrillic fragments work, but full Unicode case folding remains a future enhancement.
- The JS smoke does not verify real S3 object existence; it verifies pointer-based runtime behavior with a mock webhook.
- Historical docs still mention old terminal/local pipeline concepts in older stage reports; current operator runbook and B10 manifest docs are updated.

## B11

- Expose app events or a concise run diagnostic endpoint in HTTP so smoke can assert `output_registration_report` directly.
- Decide whether to add full Unicode search collation/tokenization.
- Consider a UI surface for viewing the last output registration report from the entity detail/run detail screen.
- Keep real S3/n8n smoke as opt-in because it depends on credentials and imported workflows.

ТЗ перечитано на этапах: after_plan, after_url_decode_design, after_cardinality_design, after_partial_output_design, after_backend_runtime_changes, after_ui_changes, after_tests, after_smoke, before_feedback
