# Stage 9 Manual QA Results

## Environment

- Date/time: 2026-04-27
- Tester: Codex desktop smoke plus automated verification
- OS: Windows development workspace
- CPU/RAM: not recorded
- Commit SHA: 4ae5854
- Branch: master
- Node version: v22.10.0
- npm version: 10.9.0
- Rust version: rustc 1.93.1 (01f6ddf75 2026-02-11)
- Tauri build environment: Visual Studio 2022 Community `vcvars64.bat` expected
- Real n8n endpoint reachable: PASS during one manual `Run due tasks` smoke, HTTP 200 for 3 tasks

## Commands Run By Codex

- `cargo fmt --manifest-path src-tauri/Cargo.toml`: PASS
- `cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'`: PASS, 92 tests passed
- `npm.cmd run build`: PASS
- `npm.cmd run release`: PASS after rerun outside sandbox and Tauri icon config fix; MSI/NSIS bundles produced
- `npm.cmd run demo:reset`: PASS during manual triage; restored 10 baseline input JSON files and removed stale `app.db`

## Manual QA Status

Overall status: PARTIAL PASS / NOT FULL MANUAL QA.

Reason: Codex performed a desktop smoke walkthrough with screenshots and SQLite instrumentation for the demo happy path, Entities, Entity Detail, real n8n run, and Workspace Explorer. Full checklist coverage is still not claimed because load testing, restart/reconciliation, release installer install/uninstall, and exhaustive mouse-driven edge cases were not replayed.

## Section Results

- 0. Test environment: PARTIAL PASS; Windows desktop, Node/Rust recorded, commit has uncommitted Stage 9 changes.
- 1. Fresh checkout / one-action launch: PARTIAL PASS; `npm.cmd run app` launched Tauri dev desktop app.
- 2. Demo workdir validation: PASS; `demo:reset` restored 10 valid incoming JSON files and no `app.db`.
- 3. Open workdir and bootstrap: PASS; opened `F:\pycharm_projects\beehive\demo\workdir` in UI, bootstrap reached `Fully Initialized`.
- 4. Scan workspace: PASS; clicked `Scan workspace` in UI. SQLite after scan: `entities=10`, `entity_files=10`, `entity_stage_states=10`, status counts `pending=10`, validation `valid=10`.
- 5. Dashboard: PARTIAL PASS; Dashboard rendered bootstrap state and manual action buttons.
- 6. Entities Table: PASS for smoke; UI displayed 10 rows, `Name` column, pending status, valid badges, and Scan workspace action. SQLite/backend checks: stage `semantic_split=10`, status `pending=10`, validation `valid=10`, search `demo-ceramic-001=1`, payload search `керамика=1`.
- 7. Entity Detail: PASS for smoke; opened table row `demo-signal-001`, detail showed metadata, latest file, manual actions, and timeline.
- 8. n8n execution and multi-output: PARTIAL PASS; automated mock HTTP PASS. Real endpoint manual smoke PASS for 3 tasks: `stage_runs=3`, HTTP 200, source states `done=3`, managed review files `3`.
- 9. Retry / failed behavior: automated regression coverage PASS; manual NOT RUN
- 10. Reconciliation / restart: automated regression coverage PASS; manual NOT RUN
- 11. Stage Editor: automated regression coverage PASS; manual NOT RUN
- 12. Workspace Explorer: PARTIAL PASS; opened UI after run, showed `entities=13`, `registered files=13`, `managed copies=3`, present/missing `13/0`.
- 13. Load test: generator 1000-file command PASS; demo reset restored baseline; manual scan/responsiveness run NOT RUN
- 14. Release readiness: automated commands PASS; full manual release readiness review NOT RUN
- 15. Documentation check: docs created; operator readability review NOT RUN

## Bugs Found

- Demo source data was generated with mojibake Cyrillic strings, which made business-name search such as `керамика` impossible. Fixed `scripts/demo-data.mjs` to generate proper UTF-8 Cyrillic.
- Entities page made the no-scan state too easy to misread as "no matching entities." Added a direct `Scan workspace` action and clearer empty-state copy.
- Entities search only covered entity id/path, while the demo checklist allows business-name search. Added backend search over latest file payload text and a `Name` column.
- UI automation through Windows `SendKeys` is unreliable for long text filters; exact filter/search counts were verified through SQLite/backend instrumentation and Rust tests.

## Screenshots / Recordings

- `tmp/stage9-opened2.png`: demo workdir opened and fully initialized.
- `tmp/stage9-after-scan.png`: Dashboard after manual scan.
- `tmp/stage9-entities.png`: Entities table with registered rows and business names.
- `tmp/stage9-detail.png`: Entity Detail opened from table.
- `tmp/stage9-workspace-explorer.png`: Workspace Explorer after real n8n run.

## Final Manual QA Decision

PARTIAL PASS. The reported Entities/Entity Detail demo path is now reproduced and fixed. Full Stage 9 manual QA remains pending for the sections not replayed above.
