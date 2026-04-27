# Stage 5.5 Instruction Checklist

The authoritative task source is `instructions/beehive_stage5_5_codex_task.md`.

## Preparation

- [ ] Re-read `README.md`.
- [ ] Re-read Stage 1 instruction and delivery docs.
- [ ] Re-read Stage 2 instruction and delivery docs.
- [ ] Re-read Stage 3 instruction and delivery docs.
- [ ] Re-read Stage 4 instruction and delivery docs.
- [ ] Re-read Stage 5 instruction and delivery docs.
- [ ] Inspect current backend modules before changing code.
- [ ] Inspect current frontend types/pages before changing DTOs.

## State machine

- [ ] Formal state machine module added.
- [ ] Approved statuses are represented consistently.
- [ ] Approved transitions are explicitly encoded.
- [ ] Invalid transitions return structured errors.
- [ ] Runtime transition reasons are represented.
- [ ] Existing runtime status updates are routed through state machine validation or a single DB wrapper.
- [ ] Direct status SQL updates are removed or explicitly justified.
- [ ] Stale `queued` recovery is implemented or proven unnecessary.
- [ ] State machine unit tests cover allowed transitions.
- [ ] State machine unit tests cover rejected transitions.

## Atomic task claiming / runtime lock

- [ ] `run_due_tasks` uses atomic claim before execution.
- [ ] Task is executed only if claim succeeds.
- [ ] Duplicate claim of same state is impossible or no-op.
- [ ] `run_entity_stage` uses equivalent active-task protection.
- [ ] Active `queued`/`in_progress` states cannot be launched again manually.
- [ ] Claim does not create `stage_runs` before ownership is established.
- [ ] Claim tests cover sequential duplicate claim.
- [ ] Claim tests cover two-connection or concurrency-style duplicate claim.
- [ ] Reconciliation handles stale active states safely.

## Partially-written / unstable files

- [ ] Runtime config supports `file_stability_delay_ms` or a documented internal default.
- [ ] Scanner skips files whose mtime is too fresh.
- [ ] Scanner checks metadata before/after read.
- [ ] Scanner does not classify temporary partial JSON as permanent invalid JSON.
- [ ] Scanner records `unstable_file_skipped` or equivalent app event.
- [ ] Existing DB snapshot is not overwritten by unstable file content.
- [ ] Executor pre-flight checks source file stability.
- [ ] Executor pre-flight checks source file still matches registered DB snapshot.
- [ ] Executor does not call n8n for unstable/changed source files.
- [ ] Executor does not create `stage_runs` or increment attempts for unstable/changed skipped files.
- [ ] Tests cover unstable file skip and later stable registration.
- [ ] Tests cover executor skip before HTTP when file changed after scan.

## Terminal stage / output_folder

- [ ] Config validation allows missing/empty `output_folder` for terminal stages.
- [ ] Config validation rejects missing/empty `output_folder` for stages with `next_stage`.
- [ ] Internal model handles terminal output folder without panics.
- [ ] Dashboard displays terminal stages correctly.
- [ ] Stage Editor/read-only stage list displays terminal stages correctly.
- [ ] Successful terminal stage execution marks source state `done`.
- [ ] Successful terminal stage execution does not create target file.
- [ ] Tests cover terminal config validation.
- [ ] Tests cover successful terminal execution.

## Source JSON / SQLite source of truth

- [ ] Source JSON is not mutated during execution.
- [ ] Source JSON does not receive `ready: true` in Stage 5.5.
- [ ] SQLite remains source of runtime state.
- [ ] Reconciliation scan does not overwrite `done`/`failed` runtime state from source JSON status.
- [ ] Tests cover source immutability and scan preservation.
- [ ] README/docs explain this behavior clearly.

## Dashboard / diagnostics

- [ ] Dashboard read path remains read-only.
- [ ] Dashboard does not scan automatically.
- [ ] Dashboard does not run tasks automatically.
- [ ] Dashboard does not call n8n automatically.
- [ ] Settings/Diagnostics display new runtime config or counters if added.
- [ ] Existing errors/events remain visible through current UI panels.
- [ ] Frontend handles DTO changes without white screen.

## Schema / migration

- [ ] Schema version decision documented.
- [ ] If schema remains v4, reason is documented.
- [ ] If schema becomes v5, fresh DB bootstrap is updated.
- [ ] If schema becomes v5, v4 -> v5 migration is implemented.
- [ ] If schema becomes v5, older migration path still works.
- [ ] Migration tests added if schema changes.

## Documentation

- [ ] `docs/codex_stage5_5_progress.md` created/updated.
- [ ] `docs/codex_stage5_5_instruction_checklist.md` created/updated.
- [ ] `docs/codex_stage5_5_delivery_report.md` created/updated.
- [ ] README updated if config/runtime behavior changed.
- [ ] Delivery report includes state machine details.
- [ ] Delivery report includes claim/locking details.
- [ ] Delivery report includes file stability guard details.
- [ ] Delivery report includes terminal-stage behavior.
- [ ] Delivery report includes schema/migration decision.
- [ ] Delivery report includes known limitations.

## Verification

- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml` passed.
- [ ] Rust tests passed through the project’s Windows/MSVC command.
- [ ] `npm.cmd run build` passed.
- [ ] No real n8n endpoint was called by automated tests.
- [ ] No full manual UI walkthrough was claimed unless actually performed.

## Acceptance

- [ ] Stage 1 still works: app/workdir/config/SQLite bootstrap are not regressed.
- [ ] Stage 2 is now acceptable: state machine and runtime lock are implemented.
- [ ] Stage 3 is now acceptable: partially-written files are not processed.
- [ ] Stage 4 is now acceptable: n8n execution uses safe claim and file pre-flight checks.
- [ ] Stage 5 is still acceptable: Dashboard remains real-data read-only overview.
- [ ] Stage 5.5 is ready for architect review.
