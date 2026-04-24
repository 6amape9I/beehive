# Stage 1 Delivery Report

## A. What was fixed

- Completed the Stage 1 polish pass on top of the existing foundation.
- Fixed stage synchronization so SQLite now mirrors `pipeline.yaml` exactly, including hard deletion of stale stage rows.
- Simplified the initialization phase model to real Stage 1 states only.
- Fixed the workdir path flow so manual input must be absolute and outside the application directory.
- Closed the dev-mode relaunch scenario reproduced when a relative workdir path resolved inside `src-tauri/`.

## B. Files changed

- Frontend: `src/app/styles.css`, `src/features/workdir/WorkdirSetupPanel.tsx`, `src/types/domain.ts`
- Backend: `src-tauri/src/bootstrap/mod.rs`, `src-tauri/src/config/mod.rs`, `src-tauri/src/database/mod.rs`, `src-tauri/src/domain/mod.rs`, `src-tauri/src/workdir/mod.rs`
- Docs: `README.md`, `docs/codex_stage1_progress.md`, `docs/codex_stage1_questions.md`, `docs/codex_stage1_instruction_checklist.md`, `docs/codex_stage1_delivery_report.md`

## C. Stage sync behavior after the fix

- Sync behavior is explicit hard delete.
- On bootstrap/reload, current YAML stages are upserted into SQLite by `stage_id`.
- Any row in `stages` that is no longer present in `pipeline.yaml` is deleted in the same sync transaction.
- Repeated syncs are deterministic and idempotent.

## D. Initialization state model after the fix

- The app now uses only these phases:
  - `app_not_configured`
  - `config_invalid`
  - `bootstrap_failed`
  - `fully_initialized`
- Removed dead phases that were never reached by the real bootstrap flow.
- `config_status` and `database_status` remain visible, but are derived from actual bootstrap outcomes.

## E. Tests added or updated

- Added or expanded tests for:
  - new workdir initialization
  - opening an existing workdir
  - valid config loading
  - invalid config handling
  - duplicate stage ids
  - stage sync updates
  - stage sync removals
  - SQLite schema bootstrap and required tables
  - bootstrap state behavior
  - relative path rejection
  - rejecting workdirs inside the application directory
- Current automated verification:
  - `npm.cmd run build` passed
  - `cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'` passed

## F. What was manually verified by you

- Verified manually: fresh app launch.
- Verified manually: new workdir initialization.
- Verified manually: creation of `pipeline.yaml`, `app.db`, `stages/`, `logs/`.
- Verified manually: valid config loading.
- Verified manually: stage visibility in Dashboard and Stage Editor.
- Verified manually: stage sync into SQLite.
- Verified manually: opening an existing workdir using `F:/pycharm_projects/beehive/test-workdirs/manual-open-existing`.
- Verified manually: invalid config scenario.
- Verified manually: stage update/removal sync behavior.
- Reproduced: in `tauri dev`, a relative path like `123123` resolved inside `src-tauri/` and SQLite writes triggered a rebuild/relaunch.
- Fixed and re-tested: opening an existing workdir with an absolute path outside the application directory keeps the same app PID and reaches `fully_initialized`.

## G. .gitignore changes

- Reviewed `.gitignore`.
- The required Stage 1 ignores are already present, including `.vscode/`, `.env`, `.env.*`, `*.db`, `*.db-shm`, `*.db-wal`, `*.sqlite`, `*.sqlite3`, and `*.tsbuildinfo`.
- No additional `.gitignore` changes were required in this polish pass.

## H. Remaining blockers for Stage 1

- No blocker remains in the Stage 1 foundation scope.
- Stage 2 features remain intentionally out of scope: n8n runtime execution, retries, scanning runtime, entity processing, stage graph execution, advanced editor work, and orchestration scheduling.
