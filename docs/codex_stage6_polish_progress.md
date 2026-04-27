# Stage 6 Polish Progress

## 2026-04-27

- Re-read `instructions/beehive_stage6_polish_codex_task.md`.
- Confirmed scope is a stabilization patch only: no Stage 7, no schema migration, no new runtime feature.
- Added backend-owned file edit policy for `save_entity_file_business_json`.
- Added file-level allowed actions to Entity Detail DTO so the UI reflects backend policy instead of deriving rules locally.
- Started database decomposition with `src-tauri/src/database/entities.rs`; the submodule now owns Stage 6 action-policy builders and JSON edit policy helpers.
- Updated Entity Detail UI so the JSON editor disables edit/save when backend policy forbids it and shows the backend reason.
- Added Rust tests for allowed save statuses, forbidden save statuses, missing stage state, rejected-save immutability, rejected-save event logging, and detail policy exposure.

## Feedback

- The policy belongs in backend because the file save command can be called independently of the UI.
- The decomposition is deliberately small: it removes Stage 6 policy/action responsibility from `database/mod.rs` without touching schema, executor, discovery, or migration code.
- `done` is now treated as an audit-complete runtime state. Business JSON edits require an explicit future workflow rather than silent mutation of completed artifacts.

## Verification

- PASS: `cargo fmt --manifest-path src-tauri/Cargo.toml`
- PASS: `cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'`
  - 76 tests passed.
- PASS: `npm.cmd run build`
