# Stage 9 Instruction Checklist

## Source Of Truth

- [x] Re-read `instructions/beehive_stage9_codex_task.md`.
- [x] Re-read `instructions/beehive_stage9_manual_qa_checklist.md`.
- [x] Checked current code before implementing.

## Runtime And Stabilization

- [x] n8n root array response creates multiple target JSON files.
- [x] n8n wrapper `payload` array creates multiple target JSON files.
- [x] Backward-compatible single object `payload` remains supported.
- [x] Invalid/scalar output fails without creating target files.
- [x] Child target JSON wraps business payload under `payload` and keeps beehive metadata under `meta.beehive`.
- [x] Child ids/filenames are deterministic and collision-safe.
- [x] Compatible repeated output reuse is idempotent.
- [x] Terminal stage array response marks source `done` without target copies.
- [x] Automated tests use mock HTTP only.
- [x] Pipeline backup/temp filenames are collision-safe for rapid saves.
- [x] Draft validation backend error returns a validation issue.
- [x] Workspace Explorer selected file no longer triggers a reload.
- [x] Workspace Explorer trail open actions use backend-derived open policy.
- [x] Config repair mode documented as deferred.

## Demo And Scripts

- [x] `npm run app` script exists.
- [x] `npm run demo:reset` script exists.
- [x] `npm run demo` script exists.
- [x] `npm run demo:generate` script exists.
- [x] `npm run verify` script exists.
- [x] `npm run release` script exists.
- [x] Demo workdir contains `pipeline.yaml`, stage folders, logs, invalid samples, and 10 input JSON files.
- [x] Demo pipeline uses the provided n8n endpoint only in demo config/docs.
- [x] Demo integrity is covered by a Rust test.
- [x] Demo reset was rerun during manual triage.
- [x] Demo Cyrillic fixture data generates valid UTF-8 business names.

## Manual QA Triage Follow-up

- [x] Real Tauri desktop app launched through `npm.cmd run app`.
- [x] `demo/workdir` opened through the UI and reached `Fully Initialized`.
- [x] `Scan workspace` clicked through the UI.
- [x] SQLite after scan verified `entities=10`, `entity_files=10`, `entity_stage_states=10`.
- [x] Entities table rendered rows after scan.
- [x] Entities table exposes business-name `Name` column.
- [x] Entities search covers entity id/path/filename/latest payload text.
- [x] Entity Detail opened from an Entities row.
- [x] One real demo webhook `Run due tasks` smoke executed.
- [x] Workspace Explorer opened after managed copy creation.
- [x] Full manual checklist remains partial, not claimed as complete.

## Documentation And QA

- [x] Stage 9 progress doc exists.
- [x] Stage 9 checklist exists.
- [x] Stage 9 delivery report exists.
- [x] Manual QA checklist doc exists.
- [x] Manual QA results doc exists and is honest about unperformed UI QA.
- [x] User guide exists.
- [x] Demo guide exists.
- [x] Release checklist exists.
- [x] README links Stage 9 docs and scripts.

## Verification

- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml`
- [x] Rust tests through `vcvars64.bat` passed during implementation.
- [x] Final Rust tests through `vcvars64.bat`
- [x] `npm.cmd run build`
- [x] `npm.cmd run release`
- [x] Partial Codex desktop smoke/manual triage for the reported Entities issue
- [ ] Full manual UI QA performed by an operator

## Deferred / Not Claimed

- [ ] Config repair mode.
- [ ] Full manual QA pass by Codex.
- [ ] Automated tests against the real n8n endpoint.
