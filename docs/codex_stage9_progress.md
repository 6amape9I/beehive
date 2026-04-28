# Stage 9 Progress

## 2026-04-27

- Re-read `instructions/beehive_stage9_codex_task.md` and `instructions/beehive_stage9_manual_qa_checklist.md`.
- Inspected current Stage 8 implementation around executor, file operations, pipeline editor, Workspace Explorer, scripts, and README.
- Implemented Stage 9 multi-output n8n response support using mock-test-only HTTP coverage:
  - root array responses;
  - wrapper `payload` arrays;
  - backward-compatible single object payloads;
  - deterministic child entity ids and filenames;
  - idempotent compatible target reuse;
  - terminal-stage array success without target copies.
- Applied required stabilization fixes:
  - collision-safe `pipeline.yaml` backup/temp filenames;
  - backend draft-validation command now returns an error validation issue on backend failure;
  - Workspace Explorer no longer reloads on selected file changes;
  - trail-node file/folder actions use backend-provided open policy.
- Added demo workdir reset/generation scripts and generated baseline demo workdir.
- Rust tests passed through the MSVC `vcvars64.bat` wrapper during development.
- Final verification completed:
  - `cargo fmt --manifest-path src-tauri/Cargo.toml` passed.
  - Rust tests through `vcvars64.bat` passed, 92 tests.
  - `npm.cmd run build` passed.
  - `npm.cmd run release` passed after fixing Tauri `bundle.icon` to use existing icon files.
- Ran `npm.cmd run demo:generate -- --count 1000` successfully, then restored baseline with `npm.cmd run demo:reset`.

## Feedback

- Stage 9 child outputs are now modeled as separate child entities in the target stage, which matches the multi-output requirement and avoids the existing `UNIQUE(entity_id, stage_id)` constraint.
- Config repair mode remains deferred. Implementing a real repair workflow would be a product feature, not a stabilization patch.
- Full mouse-driven manual QA is still not claimed by Codex. The checklist and results file are prepared for an operator/architect pass.

## 2026-04-27 Manual QA Triage Follow-up

- Re-read Stage 9 instructions and the manual QA checklist before triage.
- Ran `npm.cmd run demo:reset` and confirmed baseline state:
  - 10 incoming JSON files restored;
  - `app.db` removed before opening the app.
- Launched the Tauri desktop app through `npm.cmd run app`, opened `demo/workdir` through the UI, and confirmed `Fully Initialized`.
- Clicked `Scan workspace` in the real app and checked SQLite immediately after:
  - `entities=10`;
  - `entity_files=10`;
  - `entity_stage_states=10`;
  - status counts: `pending=10`;
  - validation: `valid=10`.
- Found and fixed demo-facing issues:
  - demo data source used mojibake Cyrillic strings, so business-name search could not work reliably;
  - Entities page did not provide a direct scan action and the empty state hid the required operator step;
  - Entities search did not cover payload business names.
- Added a lightweight `display_name` to `EntityTableRow`, derived from latest registered file payload `entity_name`.
- Extended `list_entities` search to cover entity id, latest path, latest filename, and latest payload text.
- Added `Scan workspace` action and clearer empty state to Entities page.
- Manually opened Entities after scan: rows, business names, pending status, and valid badges rendered.
- Opened Entity Detail from a table row: metadata, latest file, manual actions, and timeline rendered.
- Ran one real manual `Run due tasks` smoke against the demo n8n webhook:
  - 3 `stage_runs`;
  - HTTP 200 for all 3;
  - 3 source states moved to `done`;
  - 3 managed review artifacts registered.
- Opened Workspace Explorer after run: totals showed 13 entities/files, 3 managed copies, 13 present / 0 missing.
- Re-ran technical verification after fixes:
  - `cargo fmt --manifest-path src-tauri/Cargo.toml`: PASS;
  - Rust tests through `vcvars64.bat`: PASS, 92 tests;
  - `npm.cmd run build`: PASS.

## Follow-up Feedback

- The original “no entities” observation was valid before scan: the reset demo starts without `app.db` and the app intentionally does not auto-scan. The UI now makes the manual scan step reachable from Entities itself.
- The runtime chain is working after scan: demo JSON -> scan -> SQLite -> Entities table -> Entity Detail.
- Search by `керамика` is now backend-supported and test-covered. Windows `SendKeys` was unreliable for typing that exact UI filter during Codex QA, so the final evidence combines screenshot UI smoke with SQLite/backend checks.
