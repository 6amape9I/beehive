# Follow-up Orphan Stage Runs Checklist

Source of truth: `instructions/beehive_followup_orphan_stage_runs_codex_task.md`.

## Implementation

- [x] Follow-up instruction re-read.
- [x] Current executor start flow inspected.
- [x] Current `stage_runs` schema inspected.
- [x] Atomic start helper added.
- [x] Stage-run insert and `queued -> in_progress` happen in one transaction.
- [x] Executor no longer calls unsafe separate `insert_stage_run` then start transition sequence.
- [x] HTTP is sent only after atomic start transaction commits.
- [x] State machine/database wrapper is still used for `queued -> in_progress`.
- [x] Stale `queued` recovery handles unfinished orphan `stage_runs`.
- [x] Orphan runs are marked unsuccessful.
- [x] Orphan runs use `error_type = claim_recovered_before_start`.
- [x] Attempts are not incremented during orphan claim recovery.
- [x] `orphan_stage_run_reconciled` app event is written.
- [x] Schema remains v4; no migration needed.

## Tests

- [x] Atomic start success test added.
- [x] Failed start transition leaves no partial `stage_runs` row.
- [x] Legacy orphan queued reconciliation test added.
- [x] No duplicate HTTP after orphan recovery test added.
- [x] Existing Stage 1-5.5 tests still pass locally.

## Verification

- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml` passed.
- [x] Rust tests through MSVC/vcvars command passed.
- [x] `npm.cmd run build` passed.
- [x] No real n8n endpoint was used by automated tests.
- [x] No manual UI walkthrough was claimed.

## Acceptance

- [x] Crash-window is closed for future runs.
- [x] Legacy orphan rows are reconciled explicitly.
- [x] Audit history remains clear after recovery and next actual run.
- [x] Follow-up is ready for review after required verification commands pass.
