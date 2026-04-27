# Follow-up Orphan Stage Runs Progress

## 2026-04-27

- Re-read `instructions/beehive_followup_orphan_stage_runs_codex_task.md`.
- Inspected current `stage_runs` schema, executor flow, start helpers, and stale `queued` recovery.
- Confirmed the pre-fix crash window:
  - task was claimed as `queued`;
  - source pre-flight passed;
  - `stage_runs` row was inserted;
  - `queued -> in_progress` happened in a separate call.
- Added `database::start_claimed_stage_run` so stage-run insert and `queued -> in_progress` commit atomically.
- Updated executor to call the atomic start helper before HTTP.
- Extended stale `queued` recovery to finish legacy unfinished orphan `stage_runs` rows as unsuccessful with `error_type = claim_recovered_before_start`.
- Added app event `orphan_stage_run_reconciled`.
- Kept schema at v4; existing `stage_runs` fields are sufficient.
- Added Rust tests for atomic start, failed transition rollback, orphan reconciliation, and no duplicate HTTP after orphan recovery.
- Ran required verification:
  - `cargo fmt --manifest-path src-tauri/Cargo.toml`: pass
  - `cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'`: pass, 66 tests
  - `npm.cmd run build`: pass

## Feedback To Instruction Creator

- The criticism was accurate: Stage 5.5 eliminated duplicate processing, but left an audit-history crash window between run insert and runtime start.
- The fix is intentionally narrow and does not require schema v5.
- The only remaining limitation is the same Stage 5.5 limitation: this is local SQLite protection, not distributed locking across machines.
