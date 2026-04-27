# Follow-up Orphan Stage Runs Delivery Report

## Implemented

Closed the narrow crash window between `stage_runs` insertion and `queued -> in_progress`.

The executor now starts a claimed task by calling a single database helper that commits the audit row and runtime state transition together before HTTP is sent.

## Crash-Window Closed

Previous sequence:

```text
insert stage_runs
update queued -> in_progress
send HTTP
```

This could leave `queued` plus an unfinished `stage_runs` row if the app crashed between the first two operations.

New sequence:

```text
atomic transaction:
  validate queued -> in_progress
  insert stage_runs
  update state to in_progress
commit
send HTTP
```

If the transaction does not commit, no new `stage_runs` row remains.

## Atomic Start Behavior

Added `database::start_claimed_stage_run`.

The helper:

- requires the state to still be `queued`;
- validates `queued -> in_progress` through the Stage 5.5 state machine path;
- inserts the `stage_runs` row;
- updates attempts and start timestamps;
- commits as one SQLite transaction.

## Orphan Stage Runs Reconciliation

Stale `queued` recovery now checks for unfinished `stage_runs` rows matching the same entity, stage, and file instance.

Each orphan run is finished as:

- `success = false`;
- `error_type = claim_recovered_before_start`;
- `error_message = "Queued claim was recovered before workflow request was sent."`;
- `duration_ms = 0`;
- `finished_at = now`.

The state is then released with the existing `ClaimRecovery` transition back to `pending`, attempts remain unchanged, and `app_events` records `orphan_stage_run_reconciled`.

## Schema Decision

SQLite remains at schema v4.

No new columns are required because `stage_runs` already has `success`, `error_type`, `error_message`, `finished_at`, and `duration_ms`.

## Tests Added

- Atomic start creates one run, moves state to `in_progress`, and sets attempts.
- Atomic start failure when state is not `queued` does not insert a partial run.
- Legacy orphan queued reconciliation marks the orphan run unsuccessful and releases the claim.
- Next `run_due_tasks` after orphan recovery sends exactly one HTTP request and leaves clear audit history.

## Verification Commands

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
```

Result: pass.

```powershell
cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'
```

Result: pass. 66 Rust tests passed.

```powershell
npm.cmd run build
```

Result: pass. TypeScript and Vite production build completed.

## Known Limitations

- This remains local SQLite transactional protection, not distributed locking.
- No manual UI walkthrough was performed.
- Automated tests use local mock HTTP only.

## Acceptance Status

Ready for review.
