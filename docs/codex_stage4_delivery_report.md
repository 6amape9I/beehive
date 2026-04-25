# Stage 4 Delivery Report

This report reflects the implemented Stage 4 runtime execution foundation.

## A. What was implemented

- Manual, bounded runtime execution for eligible entity stage states.
- n8n Webhook Trigger integration through per-stage `workflow_url`.
- Retry mechanics, stuck task reconciliation, and `stage_runs` audit history.
- Next-stage target file creation from n8n response payload.
- Minimal UI controls and visibility for task execution state.
- Polishing fixes for execution-state preservation during scan, due retry timestamps for stuck tasks, and structural copy-block handling.

## B. Files changed

- Backend: `database`, `domain`, `executor`, `file_ops`, `commands`, `config`, `lib`, Cargo dependencies.
- Frontend: runtime types/API wrappers, Dashboard, Entity Detail, Settings / Diagnostics.
- Docs: README plus Stage 4 progress, checklist, and delivery report.

## C. n8n execution behavior

- Runtime sends `POST` to the configured stage `workflow_url` with JSON headers.
- Request body contains logical entity id, stage id, entity file id, source file path, attempt number, run id, source payload, and source meta with a `meta.beehive` execution block.
- Success requires HTTP 2xx, valid JSON object, `success` not equal to `false`, and object `payload` when a next stage exists.
- Failures are classified as network, timeout, HTTP status, invalid JSON, contract, copy failure, or DB transition failure.
- If HTTP succeeds but the next-stage copy is structurally blocked by a missing or inactive target stage, the stage state becomes `blocked`, the run is recorded as unsuccessful with `error_type = copy_blocked`, and no retry is scheduled.

## D. Retry behavior

- Eligible states are `pending` and due `retry_wait`.
- Attempt number is `attempts + 1`.
- Failed attempts become `retry_wait` with `next_retry_at` while attempts remain.
- Final failed attempts become `failed`.
- Stuck `in_progress` states older than runtime timeout are reconciled before each manual batch. Retryable stuck tasks become `retry_wait` with `next_retry_at` due immediately; exhausted stuck tasks become `failed`.
- `run_due_tasks` respects future `next_retry_at`. `run_entity_stage` is intentionally a manual/debug command and may run a `retry_wait` state before its scheduled retry time.

## E. Schema/migration changes

- SQLite schema version is now v4.
- Fresh databases create v4 directly.
- v3 databases migrate by rebuilding `stage_runs` into the Stage 4 audit shape.
- v1/v2 databases continue through the existing migration path into v4.

## F. File behavior

- Source JSON files are not mutated during execution.
- Next-stage files are created from n8n response `payload`, not copied from the original source payload.
- Target meta merges source meta and n8n response meta with Stage 4 `meta.beehive` provenance.
- Existing compatible targets keep Stage 3 non-destructive checksum behavior.
- Reconciliation scans preserve `entity_stage_states` execution state; source JSON can still show `pending` while SQLite remains `done`.

## G. Tests added/updated

- Successful n8n execution with target file creation and source immutability.
- HTTP non-2xx retry and final failure.
- Contract error for missing payload when next stage exists.
- Retry wait not due vs due behavior.
- Stuck `in_progress` reconciliation with due retry timestamp and follow-up execution.
- Done state not executed again.
- Successful run followed by `scan_workspace` does not regress `done` or resend HTTP.
- Missing/inactive next-stage copy after successful HTTP becomes structural `blocked` with `copy_blocked`.
- `run_entity_stage` debug retry-delay bypass is covered while `run_due_tasks` still skips future retries.
- v4 bootstrap and migration assertions.

## H. Verification performed

- `cargo fmt --manifest-path src-tauri/Cargo.toml`: passed.
- `cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'`: passed, 45 Rust tests.
- `npm.cmd run build`: passed.
- UI smoke was not rerun; no manual UI walkthrough is claimed.

## I. Known limitations

- No background daemon or scheduler.
- No n8n REST API integration.
- No credential storage or authentication UI.
- No complex branching or polling.
- UI remains intentionally minimal.

## J. Whether Stage 4 is ready for review

- Yes. Stage 4 is ready for review because the execution core, retry mechanics, v4 migration, audit rows, response-based next-stage file creation, mock HTTP tests, and required technical verification are complete.
