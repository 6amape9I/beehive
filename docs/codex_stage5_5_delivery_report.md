# Stage 5.5 Delivery Report

## Summary

Stage 5.5 closes the architectural stabilization gaps identified after Stages 1-5:

- runtime status transitions now pass through a formal state machine;
- due-task selection now uses atomic SQLite claim semantics;
- stale `queued` and `in_progress` states are reconciled safely;
- scanner and executor paths now guard against unstable or partially-written JSON files;
- terminal stages may omit `output_folder`;
- SQLite remains the source of runtime execution state and source JSON remains immutable during execution.

No daemon, scheduler, worker pool, real n8n integration, reset/requeue UX, or mouse-driven UI QA was added.

## State Machine

Implemented `src-tauri/src/state_machine/mod.rs`.

The state machine uses the approved public statuses:

`pending`, `queued`, `in_progress`, `retry_wait`, `done`, `failed`, `blocked`, `skipped`.

Runtime transitions now include explicit reasons such as:

- `runtime_claim`
- `runtime_start`
- `runtime_success`
- `runtime_retry_scheduled`
- `runtime_failed`
- `runtime_blocked`
- `stuck_reconciliation`
- `claim_recovery`

Invalid transitions return structured context with from/to status, reason, state id, entity id, and stage id where available. Database wrappers validate transitions before mutating runtime status.

## Atomic Claiming / Locking

`run_due_tasks` no longer uses select-then-update execution. It now claims tasks in SQLite before execution by moving eligible rows from `pending` or due `retry_wait` to `queued` in a transaction.

Only successfully claimed rows are executed. Duplicate claims of the same entity/stage state become no-op after the first successful claim.

`run_entity_stage` remains a manual debug path that may bypass retry delay for `retry_wait`, but it now uses the same claim protection and refuses already active `queued`/`in_progress` states.

Stale `queued` claims are reconciled back to `pending` without incrementing attempts or creating `stage_runs`. Stale `in_progress` handling continues to move retryable states to due `retry_wait` or exhausted states to `failed`.

## File Stability Guard

Added `src-tauri/src/file_safety/mod.rs`.

Scanner behavior:

- reads metadata before file read;
- skips files whose modified time is younger than `runtime.file_stability_delay_ms`;
- reads bytes;
- re-checks metadata after read;
- skips files that change during read;
- records `unstable_file_skipped` instead of permanent invalid JSON for unstable files.

Executor behavior:

- validates the source file again after claim and before any `stage_runs` row is created;
- checks file existence, stable read, checksum, size, and mtime against the registered DB snapshot;
- releases the claim back to `pending` if the source changed or is unstable;
- does not call HTTP, create `stage_runs`, or increment attempts for this skipped case.

## Terminal Stage Behavior

Config validation now allows terminal stages to omit or leave empty `output_folder` when `next_stage` is absent.

Stages with `next_stage` still require non-empty `output_folder`.

The internal model keeps `output_folder: String` and normalizes terminal missing output to an empty string to avoid a schema migration. Dashboard read data exposes terminal output as absent, and Stage Editor displays it as not required.

Successful terminal execution marks the source state `done`, writes a successful `stage_run`, and creates no target file.

## Schema / Migration

SQLite remains at `user_version = 4`.

No schema v5 was added because Stage 5.5 did not require new persistent columns. Atomic claim, transition validation, stale claim recovery, and file pre-flight checks are implemented with existing v4 columns:

- `entity_stage_states.status`
- `entity_stage_states.updated_at`
- `entity_stage_states.attempts`
- existing stage/file relationships

This avoids unnecessary migration risk while still closing the Stage 5.5 stabilization gaps.

## UI And Docs

Frontend changes were intentionally small:

- TypeScript `RuntimeConfig` includes `file_stability_delay_ms`.
- Settings / Diagnostics displays file stability delay.
- Stage Editor displays terminal output folder as `Not required`.
- Dashboard read model maps empty terminal output folder to `null`.

README was updated to document:

- state machine and SQLite runtime source of truth;
- atomic claim behavior;
- file stability guard;
- terminal `output_folder` rules;
- debug behavior of `run_entity_stage`.

## Tests Added / Updated

Added or updated tests covering:

- state machine allowed and rejected transitions;
- database transition wrapper rejecting invalid `pending -> done`;
- duplicate atomic claim no-op after first claim;
- stale `queued` release with no attempts increment and no `stage_runs`;
- `run_entity_stage` refusing active `queued`;
- scanner unstable file skip and later stable registration;
- executor source file changed after scan skip before HTTP;
- terminal stage config validation;
- terminal execution success without target copy;
- existing source immutability and scan-preservation regression.

The full Rust suite now has 62 passing tests.

## Verification Commands

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
```

Result: pass.

```powershell
cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'
```

Result: pass. 62 Rust tests passed.

```powershell
npm.cmd run build
```

Result: pass. TypeScript and Vite production build completed.

## Known Limitations

- Runtime execution is still manual. No background daemon, watcher, scheduler, or worker pool exists.
- Atomic claim is local SQLite transactional protection, not distributed locking across machines.
- `queued` recovery uses existing `updated_at` age because no v5 claim metadata columns were added.
- No full manual UI walkthrough was performed in this pass.
- Automated tests use local mock HTTP only; no real n8n endpoint was called.

## Acceptance Status

Stage 5.5 is ready for architect review.

The stabilization gaps listed in the Stage 5.5 instruction are closed by code changes and automated verification, with schema remaining intentionally at v4.
