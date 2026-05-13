# Beehive S3 B3 Plan

## 1. Current B2.2 readiness status

B2.2 is treated as ready for B3: the active production contract is the S3 JSON body control envelope, header-based source-key delivery is deprecated, and the repository should keep only small reproducible smoke fixtures rather than full datasets or zip artifacts. Existing S3 reconciliation, manual registration, manifest validation, and the B2.2 real smoke evidence are the foundation for B3.

## 2. UI/operator surfaces to add or change

- Workspace Explorer: add an operator S3 panel for reconciliation, manual registration, and controlled batch execution.
- Workspace Explorer file rows: make S3 pointers visually explicit and expose copyable S3 URIs while keeping local open actions unavailable for S3 rows.
- Stage Editor project/runtime form: expose S3 storage fields from `pipeline.yaml` and preserve `stage.input_uri`, `stage.save_path_aliases`, and `stage.allow_empty_outputs`.
- Runtime API/Tauri commands: keep existing `reconcile_s3_workspace` and `register_s3_source_artifact`; add a small limited run command if needed for operator-controlled batches.

## 3. S3 reconciliation exposure

The UI will call `reconcile_s3_workspace` for the selected workdir and render the summary fields required by B3: stage/listed/tagged/registered/updated/unchanged/missing/restored/unmapped counts, elapsed time, and latest reconciliation timestamp. Errors will be shown as operator-facing messages without exposing secrets.

## 4. Manual S3 source registration exposure

Workspace Explorer will include a minimal form with `stage_id`, `entity_id`, `artifact_id`, `bucket`, `key`, optional `version_id`, `etag`, `checksum_sha256`, and `size`. The backend already validates active S3-capable stages, prefix matching, idempotent duplicates, conflicting duplicates, and avoids reading S3 object bodies.

## 5. Controlled batch execution

B3 will expose a "small batch" action that runs due tasks with existing scheduler semantics but with an operator-specified cap of 1-5 tasks. Backend support will clamp the requested limit and reuse the current `Executor::run_due_tasks` path.

An ignored Rust smoke test named `real_s3_n8n_smoke_batch_small` will require `BEEHIVE_REAL_S3_BATCH_SMOKE=1`, use `BEEHIVE_SMOKE_BATCH_LIMIT` with a default of 3, and print source keys, run IDs, final states, child pointers, and output keys when real S3/n8n are available.

## 6. JSON-body contract and Cyrillic S3 key verification

The S3 control envelope schema string will be centralized. Tests will verify that:

- `source_key` with Cyrillic is serialized in the JSON body.
- `X-Beehive-Source-Key` is absent from S3 requests.
- `stage_runs.request_json` stores the technical envelope.
- the request body does not include business payload fields.
- target prefix and save_path come from configured S3 routes where available.

## 7. n8n workflow governance, docs, and linting

B3 will add `docs/n8n_workflow_authoring_standard.md` and a clear body-JSON workflow fixture location under `docs/n8n_workflows/`. A small Python linter will scan active fixtures for old source-key headers, production list/search bucket source selection, typo paths, unsafe save paths, unjustified Code-node density, and webhook nodes not configured for POST/response-node execution.

## 8. Tests to add or update

- Rust backend tests for S3 envelope construction, Cyrillic key preservation, no source-key header, and opt-in batch smoke naming/guard.
- Existing reconciliation/manual registration tests must keep passing.
- Frontend TypeScript/Vite build must pass.
- Python linter compile and fixture lint must pass.
- Real batch smoke will run only if credentials and n8n endpoint are available.

## 9. Exact commands to run

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
python3 -m py_compile scripts/lint_n8n_workflows.py
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
BEEHIVE_REAL_S3_BATCH_SMOKE=1 BEEHIVE_SMOKE_BATCH_LIMIT=3 cargo test --manifest-path src-tauri/Cargo.toml real_s3_n8n_smoke_batch_small -- --ignored --nocapture
git diff --check
```

## 10. Non-goals

B3 will not implement a daemon, high-load worker pools, async manifest polling, n8n REST workflow editing, S3 business JSON reads in Beehive execution, source-key headers, arbitrary S3 object execution without identity metadata/manual registration, or production Search/List Bucket source selection.

## 11. Risks and rollback

- UI changes may expose backend errors more directly; rollback is limited to the new Workspace Explorer and Stage Editor form sections.
- Limited batch execution must not change default scheduler behavior; rollback is removing the new command and UI action.
- Workflow linting may be stricter than existing ad hoc fixtures; active fixtures will be kept small and body-JSON only.
- Real smoke can fail for external reasons: n8n workflow import, n8n S3 credentials, S3/network access, or manifest/save_path route mismatches. These will be documented as blockers rather than hidden.

## Checkpoints

- after_plan
- after_operator_ui_design
- after_s3_reconcile_ui
- after_manual_registration_ui
- after_batch_smoke_runner
- after_n8n_governance
- after_tests
- before_feedback
