# B7 Web Pilot Hardening Feedback

## 1. What Was Implemented

B7 added a browser/API-selected S3 pilot path: operators can select eligible S3 source artifacts in Workspace Explorer, run only those roots and descendants, and inspect selected-run output lineage. The same behavior is exposed through HTTP and Tauri-compatible APIs.

## 2. Preserved B6/B5/B4 Pieces

Preserved:

- B6 workspace registry and browser workspace selector.
- B6 Workspace Explorer, Stage Editor, S3 stage creation, and stage linking.
- B6 broad `run-small-batch` and `run-pipeline-waves` endpoints.
- B5 `producer_run_id` multi-output lineage read model.
- B4 S3+n8n JSON body envelope, manifest validation, save_path routing, retries, blocked states, and stage run audit.

## 3. Backend/API Changes

Added:

- `src-tauri/src/services/selected_runner.rs`
- `POST /api/workspaces/{workspace_id}/run-selected-pipeline-waves`
- Tauri command `run_selected_pipeline_waves_by_id`
- selected-run domain types in `src-tauri/src/domain/mod.rs`
- structured selected-batch logs for start/finish

## 4. Frontend/UI Changes

Workspace Explorer now has:

- checkbox selection for eligible S3 rows;
- selected count and clear action;
- `Run selected pipeline waves` as the recommended pilot action;
- max waves, max tasks per wave, and stop-on-first-failure controls;
- selected-run summary with root results, run ids, output counts, child artifacts, target stages, statuses, relation, and S3 URI;
- Workspace Explorer output expansion for rows with `producer_run_id`.

## 5. Selected Batch Behavior And Safety Rules

The selected runner:

- rejects empty roots and more than 10 roots;
- clamps `max_waves` to `1..10`;
- clamps `max_tasks_per_wave` to `1..5`;
- rejects missing, non-S3, missing-state, missing-file, and already-done roots;
- allows conservative pilot roots with `pending` or `retry_wait`;
- runs exact selected `entity_file_id` values first;
- follows only children produced by selected runs via `entity_files.producer_run_id`;
- never falls back to the broad pending queue.

## 6. One-To-Many Lineage Behavior

The B5/B6 `GET /api/workspaces/{workspace_id}/stage-runs/{run_id}/outputs` endpoint remains the source for output expansion. The B7 selected-run response also includes an `output_tree` so one source producing multiple children remains visible as multiple child rows.

## 7. Real Web Pilot Status

Passed through scripted HTTP web path against real S3 and real n8n.

Workspace:

```text
workspace_id = b7-smoke
server_url = http://127.0.0.1:8788
workdir = /tmp/beehive-b7-web-pilot/workdir
```

## 8. Browser UI Manual Inspection

Browser UI was not manually clicked. The built frontend was served by `beehive-server`, and the browser-facing HTTP API path was exercised with `curl` and `scripts/web_operator_smoke.mjs`.

## 9. Real S3 Contacted

Yes. S3 was listed with AWS CLI using `--no-verify-ssl` because the local AWS CLI certificate chain did not trust Selectel. HTTP reconciliation registered 50 metadata-tagged raw source objects.

## 10. Real n8n Contacted

Yes. The selected run called:

```text
https://n8n-dev.steos.io/webhook/beehive-s3-pointer-smoke-body
```

n8n returned a valid manifest and uploaded the processed S3 object.

## 11. Source Artifact Keys Used

Selected source:

```text
beehive-smoke/test_workflow/raw/smoke_entity_001__порфирия.json
```

## 12. Selected Entity File IDs

```text
[1]
```

## 13. Run IDs

```text
b4f89c99-1900-4ccd-bffa-f994bdf6092b
```

## 14. Output Artifact Keys

```text
beehive-smoke/test_workflow/processed/smoke-output-b4f89c99-1900-4ccd-bffa-f994bdf6092b.json
```

## 15. Final Source/Child States

```text
smoke_entity_001 | smoke_source    | done    | file_exists=1
smoke_entity_001 | smoke_processed | pending | file_exists=1
```

Child file:

```text
entity_file_id = 51
artifact_id = smoke-output-b4f89c99-1900-4ccd-bffa-f994bdf6092b
producer_run_id = b4f89c99-1900-4ccd-bffa-f994bdf6092b
```

## 16. Server Hardening Changes

Added:

- `BEEHIVE_SERVER_MAX_BODY_BYTES`, default `1048576`;
- `413 Payload Too Large` for oversized HTTP requests;
- `BEEHIVE_ALLOWED_ORIGIN` allow-list;
- local CORS defaults without unconditional wildcard;
- rejection of `BEEHIVE_ALLOWED_ORIGIN='*'` for non-local bind;
- retained B6 token behavior;
- frontend token support through `VITE_BEEHIVE_OPERATOR_TOKEN` or `localStorage.BEEHIVE_OPERATOR_TOKEN`;
- JSON-line logs for server start, request completion/failure, workspace actions, and selected batch start/finish.

## 17. n8n Preflight Findings

Created `docs/n8n_live_web_pilot_preflight_b7.md`. The repo smoke fixture uses body JSON for `source_bucket`, `source_key`, and `save_path`; no active checked fixture references `X-Beehive-Source-Key`, `Read Beehive headers`, or `/main_dir/pocessed`. Runtime pilot evidence confirms the live smoke webhook returned a valid manifest for one selected source.

## 18. Commands Run And Exact Results

```text
cargo fmt --manifest-path src-tauri/Cargo.toml
result: passed

cargo test --manifest-path src-tauri/Cargo.toml
result: ok, 148 passed, 0 failed, 3 ignored

npm run build
result: passed, dist/assets/index-BO7eYwgG.js 400.72 kB

VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
result: passed, dist/assets/index-DNN9t2kE.js 397.10 kB

python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
result: passed, no output

rg "@tauri-apps/api/core|invoke\(" src -n
result: only src/lib/apiClient/tauriClient.ts

git diff --check
result: passed, no output

curl -sS http://127.0.0.1:8788/api/health
result: {"status":"ok"}

curl -sS http://127.0.0.1:8788/api/workspaces
result: returned workspace b7-smoke

BEEHIVE_API_BASE_URL=http://127.0.0.1:8788 BEEHIVE_SMOKE_WORKSPACE_ID=b7-smoke node scripts/web_operator_smoke.mjs
result: passed after escalated localhost access, ok=true, stage_count=2, selected_validation_code=run_selected_pipeline_waves_failed
```

## 19. Tests Passed/Failed/Ignored

Passed:

- full Rust unit/doc test suite;
- selected-runner exact-root and descendant-scoped tests;
- HTTP selected route parse test;
- HTTP server CORS/body-limit tests;
- frontend production build;
- frontend HTTP-mode production build;
- n8n workflow lint;
- web smoke helper against local server.

Ignored:

- 3 real S3+n8n Rust tests remained ignored by design.

Failed:

- no final verification failures.
- one non-escalated Node smoke attempt failed with `fetch failed` due sandbox localhost networking; the same command passed with approved escalated localhost access.

## 20. What Could Not Be Verified

Manual browser click QA and screenshots were not performed. The selected web pilot was verified through the same HTTP API used by the browser, with the built frontend served by `beehive-server`.

## 21. Remaining Risks

- Live n8n workflows outside the tested smoke webhook may still contain old header-based, Search Bucket, or typo branches.
- B7 selected waves are synchronous MVP execution, not a background worker.
- B7 does not add full RBAC, production workflow management, or large-run scheduling.
- Selectel certificate trust still required `--no-verify-ssl` for AWS CLI checks in this environment.

## 22. What Should Be Done In B8

- Add manual browser QA or Playwright screenshots for the selected operator flow.
- Add an operator reset/retry UX for failed or blocked selected roots.
- Move pilot server auth/CORS configuration into deployment docs.
- Decide whether selected runs should become async jobs with progress polling.
- Audit real production n8n workflows against the B7 preflight checklist before larger batches.

## 23. Reread Checkpoints

ТЗ перечитано на этапах: after_plan, after_selected_batch_design, after_backend_selected_runner, after_frontend_selected_batch_ui, after_server_hardening, after_n8n_preflight, after_real_web_pilot_attempt, after_tests, before_feedback
