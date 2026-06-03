# V1 Patch: Entity S3 View and Reset Actions Feedback

## What Changed

- Added backend S3 JSON viewing for registered entity file artifacts.
- Added backend workspace reset-to-pending route for entity stage states.
- Added S3 object body read support in the S3 client.
- Tightened reset policy to only allow `failed`, `blocked`, and `retry_wait`.
- Reset now sets `attempts = 0`, clears retry/error fields, preserves `stage_runs`, and writes audit event `entity_stage_state_manual_reset`.
- Added Entity Detail UI actions for viewing S3 JSON and confirming reset to pending.
- Added frontend API client methods for HTTP and Tauri modes.
- Added backend unit/route tests for S3 JSON behavior, reset policy, audit event, history preservation, active lease rejection, and Cyrillic route decoding.

## Files Changed

- `docs/v1_patch_entity_view_reset_plan.md`
- `docs/v1_patch_entity_view_reset_feedback.md`
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/database/entities.rs`
- `src-tauri/src/database/mod.rs`
- `src-tauri/src/domain/mod.rs`
- `src-tauri/src/http_api/mod.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/s3_client.rs`
- `src-tauri/src/services/entities.rs`
- `src/app/styles.css`
- `src/components/entity-detail/EntityFileInstances.tsx`
- `src/components/entity-detail/ManualActionsPanel.tsx`
- `src/lib/apiClient/httpClient.ts`
- `src/lib/apiClient/tauriClient.ts`
- `src/lib/apiClient/types.ts`
- `src/lib/runtimeApi.ts`
- `src/pages/EntityDetailPage.tsx`
- `src/types/domain.ts`

## API Routes Added

- `GET /api/workspaces/{workspace_id}/entity-files/{entity_file_id}/s3-json`
- `POST /api/workspaces/{workspace_id}/entities/{entity_id}/stages/{stage_id}/reset-to-pending`

Tauri commands added:

- `view_workspace_entity_file_s3_json`
- `reset_workspace_entity_stage_to_pending`

## UI Actions Added

- Entity Detail -> File Instances: `View S3 JSON` button for S3 file records.
- S3 JSON modal with S3 URI, JSON preview, `Copy JSON`, `Copy S3 URI`, and `Close`.
- Entity Detail -> Manual Actions: `Reset to pending` button only for resettable states.
- Reset confirmation modal with optional reason and confirm/cancel actions.

## Tests Run

- `cargo fmt --manifest-path src-tauri/Cargo.toml`
  - Current PowerShell did not have `cargo` on `PATH`, so equivalent command was run as `& 'C:\Users\Тимур\.cargo\bin\cargo.exe' fmt --manifest-path src-tauri/Cargo.toml`.
  - Result: passed.
- `cargo test --manifest-path src-tauri/Cargo.toml`
  - Current PowerShell did not have `cargo` on `PATH`, so equivalent command was run as `& 'C:\Users\Тимур\.cargo\bin\cargo.exe' test --manifest-path src-tauri/Cargo.toml`.
  - Result: failed before project tests due missing MSVC linker: `link.exe not found`.
  - Cargo/rustc detected: `cargo 1.96.0`, `rustc 1.96.0`.
- `npm run build`
  - Initial run failed because `node_modules` was absent and `tsc` was not installed locally.
  - Ran `npm install`.
  - Result after install: passed.
- `VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build`
  - PowerShell equivalent: `$env:VITE_BEEHIVE_API_BASE_URL='http://127.0.0.1:8787'; npm run build`.
  - Result: passed.
- `python3 scripts/lint_n8n_workflows.py docs/n8n_workflows`
  - Result: failed in this Windows shell with output `Python`.
  - Fallback `python scripts/lint_n8n_workflows.py docs/n8n_workflows`: passed.
- `git diff --check`
  - Result: passed with CRLF warnings only.

## What Was Not Implemented

- No V2 architecture, Postgres, RabbitMQ, or new queue architecture.
- No bulk reset action.
- No reset from `done`, `queued`, or `in_progress`.
- No forced worker lease release inside reset.
- No S3 object deletion.
- No browser-side S3 access or credential exposure.
- No JSON editing for S3 artifacts.
- No streaming large JSON viewer; the UI uses a capped preview.
- No secondary `View S3 JSON` action in the Entities list.
- No real S3 integration test; S3 reads are covered with a mock reader in unit tests.

## Manual Test Instructions

1. Ensure Rust MSVC build tools are installed so `link.exe` is available, then rerun:
   `cargo test --manifest-path src-tauri/Cargo.toml`.
2. Start the existing Beehive HTTP/Tauri workflow for an S3 workspace with valid S3 credentials.
3. Open Entity Detail for an entity with S3 file instances.
4. In File Instances, click `View S3 JSON` on an S3 row.
5. Verify the modal shows the expected `s3://bucket/key`, structured JSON, and copy buttons.
6. Put a stage state into `failed`, `blocked`, or `retry_wait`.
7. Click `Reset to pending`, enter an optional reason, and confirm.
8. Verify the detail refresh shows `status = pending`, `attempts = 0`, cleared retry/error fields, preserved stage run history, and app event `entity_stage_state_manual_reset`.
9. Verify reset is rejected for active leased work with `active_worker_lease_exists`; use existing stuck-state reconciliation before retrying.
