# Stage 5.5 Instruction Checklist

The authoritative task source is `instructions/beehive_stage5_5_codex_task.md`.

## Preparation

- [x] Re-read `README.md`.
- [x] Re-read Stage 1 instruction and delivery docs.
- [x] Re-read Stage 2 instruction and delivery docs.
- [x] Re-read Stage 3 instruction and delivery docs.
- [x] Re-read Stage 4 instruction and delivery docs.
- [x] Re-read Stage 5 instruction and delivery docs.
- [x] Inspect current backend modules before changing code.
- [x] Inspect current frontend types/pages before changing DTOs.

## State Machine

- [x] Formal state machine module added.
- [x] Approved statuses are represented consistently.
- [x] Approved transitions are explicitly encoded.
- [x] Invalid transitions return structured errors.
- [x] Runtime transition reasons are represented.
- [x] Existing runtime status updates are routed through state machine validation or a single DB wrapper.
- [x] Direct runtime status SQL updates are removed from executor runtime paths.
- [x] Remaining direct status SQL is limited to DB wrappers, scanner non-runtime metadata, migrations, and test setup.
- [x] Stale `queued` recovery is implemented.
- [x] State machine unit tests cover allowed transitions.
- [x] State machine unit tests cover rejected transitions.

## Atomic Task Claiming / Runtime Lock

- [x] `run_due_tasks` uses atomic claim before execution.
- [x] Task is executed only if claim succeeds.
- [x] Duplicate claim of same state is no-op after first claim.
- [x] `run_entity_stage` uses equivalent active-task protection.
- [x] Active `queued` states cannot be launched again manually.
- [x] Claim does not create `stage_runs` before ownership is established.
- [x] Claim tests cover sequential duplicate claim.
- [x] Claim tests cover two-connection duplicate claim.
- [x] Reconciliation handles stale active states safely.

## Partially-Written / Unstable Files

- [x] Runtime config supports `file_stability_delay_ms`.
- [x] Scanner skips files whose mtime is too fresh.
- [x] Scanner checks metadata before/after read.
- [x] Scanner does not classify temporary partial JSON as permanent invalid JSON.
- [x] Scanner records `unstable_file_skipped`.
- [x] Existing DB snapshot is not overwritten by unstable file content.
- [x] Executor pre-flight checks source file stability.
- [x] Executor pre-flight checks source file still matches registered DB snapshot.
- [x] Executor does not call HTTP for unstable/changed source files.
- [x] Executor does not create `stage_runs` or increment attempts for unstable/changed skipped files.
- [x] Tests cover unstable file skip and later stable registration.
- [x] Tests cover executor skip before HTTP when file changed after scan.

## Terminal Stage / Output Folder

- [x] Config validation allows missing/empty `output_folder` for terminal stages.
- [x] Config validation rejects missing/empty `output_folder` for stages with `next_stage`.
- [x] Internal model handles terminal output folder without panics by normalizing it to an empty string in schema v4.
- [x] Dashboard read model represents terminal output folder as absent.
- [x] Stage Editor displays terminal output folder as not required.
- [x] Successful terminal stage execution marks source state `done`.
- [x] Successful terminal stage execution does not create target file.
- [x] Tests cover terminal config validation.
- [x] Tests cover successful terminal execution.

## Source JSON / SQLite Source Of Truth

- [x] Source JSON is not mutated during execution.
- [x] Source JSON does not receive `ready: true` in Stage 5.5.
- [x] SQLite remains source of runtime state.
- [x] Reconciliation scan does not overwrite `done`/`failed` runtime state from source JSON status.
- [x] Tests cover source immutability and scan preservation.
- [x] README/docs explain this behavior clearly.

## Dashboard / Diagnostics

- [x] Dashboard read path remains read-only.
- [x] Dashboard does not scan automatically.
- [x] Dashboard does not run tasks automatically.
- [x] Dashboard does not call n8n automatically.
- [x] Settings/Diagnostics displays file stability delay.
- [x] Existing errors/events remain visible through current UI panels.
- [x] Frontend handles DTO changes; `npm.cmd run build` passed.

## Schema / Migration

- [x] Schema version decision documented.
- [x] Schema remains v4; reason documented.
- N/A: Schema v5 fresh bootstrap was not needed because no v5 schema was added.
- N/A: Schema v5 migration was not needed because no v5 schema was added.
- N/A: Older v5 migration path was not needed because no v5 schema was added.
- N/A: Migration tests for v5 were not needed because no schema change was made.

## Documentation

- [x] `docs/codex_stage5_5_progress.md` created.
- [x] `docs/codex_stage5_5_instruction_checklist.md` created.
- [x] `docs/codex_stage5_5_delivery_report.md` created.
- [x] README updated for runtime behavior changes.
- [x] Delivery report includes state machine details.
- [x] Delivery report includes claim/locking details.
- [x] Delivery report includes file stability guard details.
- [x] Delivery report includes terminal-stage behavior.
- [x] Delivery report includes schema/migration decision.
- [x] Delivery report includes known limitations.

## Verification

- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml` passed.
- [x] Rust tests passed through the project Windows/MSVC command.
- [x] `npm.cmd run build` passed.
- [x] No real n8n endpoint was called by automated tests.
- [x] No full manual UI walkthrough was claimed.

## Acceptance

- [x] Stage 1 bootstrap tests still pass.
- [x] Stage 2 runtime-lock gap is closed by state machine and atomic claim.
- [x] Stage 3 partially-written file gap is closed by scanner stability guard.
- [x] Stage 4 execution uses safe claim and source pre-flight checks.
- [x] Stage 5 Dashboard remains real-data read-only overview.
- [x] Stage 5.5 is ready for architect review.
