# Beehive S3 B3 Feedback

## 1. B2.2 readiness result

B2.2 was ready for B3: the active production contract is the JSON body S3 control envelope, full smoke datasets/zips are removed from the working tree, and minimal fixtures remain under `beehive_s3_smoke_kit/fixtures/minimal_raw/`.

## 2. What was implemented

Implemented the operator S3 console, centralized S3 control envelope module, limited batch execution command, B3 opt-in real batch smoke, n8n authoring standard, active body-JSON workflow fixture, workflow linter, linter tests, and S3 operator runbook.

## 3. Files changed

Main B3 files:

- `docs/beehive_s3_b3_plan.md`
- `docs/beehive_s3_b3_feedback.md`
- `docs/n8n_workflow_authoring_standard.md`
- `docs/n8n_workflows/beehive_s3_pointer_smoke_body_json.json`
- `docs/s3_operator_runbook.md`
- `scripts/lint_n8n_workflows.py`
- `tests/test_n8n_workflow_linter.py`
- `src-tauri/src/s3_control_envelope.rs`
- `src-tauri/src/executor/mod.rs`
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/s3_reconciliation.rs`
- `src/lib/runtimeApi.ts`
- `src/pages/WorkspaceExplorerPage.tsx`
- `src/components/stage-editor/ProjectRuntimeForm.tsx`
- `src/app/styles.css`
- `README.md`

## 4. Operator UI/API surfaces added

Workspace Explorer now exposes `Reconcile S3`, manual `Register S3 source`, `Run small batch`, S3 reconciliation summary counts, and copyable S3 URI actions. Tauri/API adds `run_due_tasks_limited` for a 1-5 task operator batch cap.

## 5. S3 reconciliation UI behavior

The UI calls `reconcile_s3_workspace` and displays `stage_count`, `listed_object_count`, `metadata_tagged_count`, `registered_file_count`, `updated_file_count`, `unchanged_file_count`, `missing_file_count`, `restored_file_count`, `unmapped_object_count`, `elapsed_ms`, and `latest_reconciliation_at`.

## 6. Manual registration UI behavior

The form accepts `stage_id`, `entity_id`, `artifact_id`, `bucket`, `key`, optional `version_id`, `etag`, `checksum_sha256`, and `size`, then calls `register_s3_source_artifact`. Backend validation remains authoritative and does not read S3 object bodies.

## 7. S3 artifact visibility behavior

Workspace Explorer file rows explicitly distinguish S3 pointers, show bucket/key/artifact/relation/producer run/status/missing state, keep local `File`/`Folder` actions disabled through backend flags, and add a safe `Copy S3 URI` action.

Stage Editor now exposes and preserves `storage.provider`, `storage.bucket`, `storage.workspace_prefix`, `storage.region`, and `storage.endpoint`; existing stage fields `input_uri`, `save_path_aliases`, and `allow_empty_outputs` remain in the stage form.

## 8. Controlled batch smoke status

Real B3 batch smoke passed with `batch_limit=3`.

Reconciliation:

```text
listed=56 tagged=50 registered=50 updated=0 unchanged=0 unmapped=6 missing=0 restored=0
```

Run summary:

```text
claimed=3 succeeded=3 retry_scheduled=0 failed=0 blocked=0 skipped=0
```

Source keys attempted:

```text
beehive-smoke/test_workflow/raw/smoke_entity_001__порфирия.json
beehive-smoke/test_workflow/raw/smoke_entity_002__миосаркома-желчного-пузыря.json
beehive-smoke/test_workflow/raw/smoke_entity_003__цистицеркоз.json
```

Run IDs:

```text
822745ad-9152-409f-bd89-1e802526651e
4e0bf894-8949-4310-a0e3-d312b0936220
f26d5d0f-f3ae-4c87-89c0-866bab990467
```

Output keys created:

```text
beehive-smoke/test_workflow/processed/smoke-output-822745ad-9152-409f-bd89-1e802526651e.json
beehive-smoke/test_workflow/processed/smoke-output-4e0bf894-8949-4310-a0e3-d312b0936220.json
beehive-smoke/test_workflow/processed/smoke-output-f26d5d0f-f3ae-4c87-89c0-866bab990467.json
```

Source states: all `done`.

Child states: all `pending`.

S3 output existence: all `true`; sizes were `2186`, `1591`, and `3029` bytes.

Report: `/tmp/beehive_s3_batch_smoke_workdir/batch_smoke_report.json`.

## 9. Whether real S3 was contacted

Yes. The escalated real smoke listed `s3://steos-s3-data/beehive-smoke/test_workflow/`, verified source objects, and confirmed all three S3 outputs exist.

## 10. Whether real n8n was contacted

Yes. Three due tasks called the configured JSON-body n8n webhook and returned valid manifests.

## 11. n8n workflow governance changes

Added `docs/n8n_workflow_authoring_standard.md` and `scripts/lint_n8n_workflows.py`. The linter fails active fixtures on deprecated source-key headers, S3 list/search source selection, `/main_dir/pocessed`, unsafe absolute save paths, excessive unjustified Code nodes, and non-POST/non-responseNode Webhook nodes.

## 12. Workflow fixtures added/removed/deprecated

Added active body-JSON fixture: `docs/n8n_workflows/beehive_s3_pointer_smoke_body_json.json`.

Old root/header smoke workflow artifacts remain removed in the working tree from the B2 cleanup and were not reintroduced.

## 13. Commands run and exact results

```text
cargo fmt --manifest-path src-tauri/Cargo.toml
result: passed

cargo test --manifest-path src-tauri/Cargo.toml
result: passed; 123 passed, 0 failed, 2 ignored

npm run build
result: passed; tsc and vite build completed

python3 -m py_compile scripts/lint_n8n_workflows.py
result: passed

python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
result: passed

python3 -m unittest discover -s tests -p '*n8n*'
result: passed; 4 tests

BEEHIVE_REAL_S3_BATCH_SMOKE=1 BEEHIVE_SMOKE_BATCH_LIMIT=3 cargo test --manifest-path src-tauri/Cargo.toml real_s3_n8n_smoke_batch_small -- --ignored --nocapture
result: passed after escalated/network-enabled rerun; 1 passed

git diff --check
result: passed
```

The first non-escalated real smoke attempt failed with `dispatch failure` while listing S3; the network-enabled rerun passed.

## 14. Tests passed/failed/ignored

Passed: Rust unit/integration tests, frontend build, Python linter compile, workflow fixture lint, Python linter unittest, real B3 batch smoke.

Ignored by normal Rust test run: `real_s3_n8n_smoke_one_artifact` and `real_s3_n8n_smoke_batch_small`.

Failed transiently: first sandboxed real smoke attempt failed with S3 `dispatch failure`; rerun with real network access passed.

## 15. What could not be verified

No unverified B3 acceptance item remains. UI behavior was type/build verified, not manually inspected in a browser during this turn.

## 16. Ubuntu notes

Use `.env` or shell variables for S3 credentials. If Selectel certificate trust fails in AWS CLI, keep using the already validated local CA/profile approach from the prior smoke setup.

## 17. Windows notes

Keep `.env` secrets out of Git. Use PowerShell env variables for opt-in smoke and quote Cyrillic paths carefully. Tauri local open actions remain disabled for S3 pointer rows.

## 18. Remaining risks

The B3 UI is intentionally minimal and not a full S3 browser. Real smoke depends on the imported n8n workflow, n8n S3 credentials, and reachable Selectel S3 endpoint. The linter is conservative and only scans fixture JSON, not live n8n workflows.

## 19. What should be done in B4

B4 should focus on repeated controlled execution/backpressure, optional async manifest polling, richer S3 operator UI polish, production-style multi-stage walkthroughs, and larger subset runs after this small batch proof.

## 20. ТЗ reread checkpoints

ТЗ перечитано на этапах: after_plan, after_operator_ui_design, after_s3_reconcile_ui, after_manual_registration_ui, after_batch_smoke_runner, after_n8n_governance, after_tests, before_feedback
