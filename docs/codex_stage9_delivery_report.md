# Stage 9 Delivery Report

## Implemented

- Added Stage 9 demo/release scripts in `package.json`.
- Added resettable demo workdir and demo data generator.
- Added multi-output n8n response handling in the runtime executor/file-copy path.
- Applied Stage 9 stabilization fixes for pipeline backups, draft validation errors, Workspace Explorer reload behavior, and safe trail open actions.
- Added Rust coverage for multi-output runtime and demo fixture integrity.
- Performed a Stage 9 manual QA triage pass for the reported Entities issue and added demo-facing fixes:
  - direct `Scan workspace` action on Entities;
  - clearer no-entities empty state;
  - business-name display/search from latest registered file payload;
  - corrected UTF-8 demo data source strings.

## Demo workdir

- Demo lives under `demo/workdir`.
- `npm.cmd run demo:reset` restores `pipeline.yaml`, required folders, 10 valid incoming JSON files, invalid samples, and removes stale `app.db`.
- Demo pipeline uses `semantic_split -> review`.
- The provided n8n webhook URL appears only in demo config/docs, not automated tests.

## Multi-output n8n support

- Supported response forms:
  - root array of JSON objects;
  - wrapper object with `payload` array;
  - wrapper object with single `payload` object.
- Each output item is treated as business payload and wrapped into full beehive JSON.
- Child ids use safe explicit output `id` when possible, otherwise deterministic generated ids.
- Existing compatible targets are reused non-destructively; incompatible collisions fail safely.
- Source stage is marked `done` only after all required outputs are registered.

## One-action launch

- `npm.cmd run app` starts the Tauri app in dev/demo mode.
- `npm.cmd run demo` resets the demo workdir and starts the app.

## Manual QA

- `docs/stage9_manual_qa_checklist.md` exists and is based on the architect checklist.
- `docs/stage9_manual_qa_results.md` exists for actual operator results.
- Codex performed a partial desktop smoke/manual triage after the initial delivery:
  - opened demo workdir in the real Tauri app;
  - clicked `Scan workspace`;
  - verified `entities=10`, `entity_files=10`, `entity_stage_states=10` in SQLite;
  - opened Entities and Entity Detail in the UI;
  - ran one real `Run due tasks` smoke against the demo webhook;
  - opened Workspace Explorer after managed copies were created.
- Codex still does not claim full checklist completion; unreplayed sections remain listed in `docs/stage9_manual_qa_results.md`.

## Load testing

- `npm.cmd run demo:generate -- --count 1000` generates volume demo JSON files.
- Heavy 5000+ file scenarios are documented for manual execution and are not part of default CI/test runs.
- Practical generator run completed: 1000 files generated successfully, then `npm.cmd run demo:reset` restored the committed 10-file baseline.
- Manual UI scan of the 1000-file set was not performed by Codex.

## Release build

- `npm.cmd run release` is available and maps to Tauri release build.
- Release build passes after adding the existing Tauri icon assets to `bundle.icon`.
- Produced bundles:
  - `src-tauri/target/release/bundle/msi/beehive_0.1.0_x64_en-US.msi`
  - `src-tauri/target/release/bundle/nsis/beehive_0.1.0_x64-setup.exe`

## Tests

- Added/updated Rust tests for:
  - root array multi-output;
  - wrapper payload array multi-output;
  - single object payload compatibility;
  - invalid output item failure;
  - idempotent compatible rerun;
  - child metadata and payload wrapping;
  - terminal array response;
  - incompatible target collision;
  - demo fixture integrity.

## Verification commands

| Command | Result | Notes |
| --- | --- | --- |
| `cargo fmt --manifest-path src-tauri/Cargo.toml` | PASS | Completed with exit code 0. |
| `cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'` | PASS | 92 tests passed. |
| `npm.cmd run build` | PASS | TypeScript and Vite production build completed. |
| `npm.cmd run release` | PASS | First sandbox run hit `spawn EPERM`; escalated rerun exposed missing `bundle.icon`; after configuring existing icons, final escalated release build passed and produced MSI/NSIS bundles. |

## Manual triage verification, 2026-04-27

| Check | Result | Notes |
| --- | --- | --- |
| `npm.cmd run demo:reset` | PASS | Restored 10 input JSON files and removed stale `app.db`. |
| Open `demo/workdir` in Tauri app | PASS | Bootstrap reached `Fully Initialized`. |
| Click `Scan workspace` | PASS | SQLite after scan: `entities=10`, `entity_files=10`, `entity_stage_states=10`, statuses `pending=10`. |
| Entities table | PASS | UI rendered 10 rows with business names, pending states, valid badges. |
| Entity Detail | PASS | Table row opened detail with file metadata, manual actions, and timeline. |
| Real `Run due tasks` smoke | PASS | 3 stage runs, HTTP 200, 3 source states `done`, 3 managed review files registered. |
| Workspace Explorer | PASS | UI showed 13 entities/files, 3 managed copies, 13 present / 0 missing. |
| Full manual QA checklist | PARTIAL | Load/restart/installer/reconciliation edge walkthrough not fully replayed by Codex. |

## Known limitations

- Full config repair mode is deferred.
- Full manual QA requires an operator-controlled desktop pass and is not claimed unless results are filled in.
- Real n8n endpoint availability is external and must be recorded during manual QA.

## Acceptance status

Technically closed for Stage 9 automated/build/release criteria. Full product acceptance remains pending operator manual QA using `docs/stage9_manual_qa_checklist.md`.
