# Stage 4 Instruction Checklist

The authoritative checklist source is `instructions/beehive_stage4_codex_task.md`.

## Core acceptance

- [x] n8n webhook execution uses configurable `workflow_url` from `pipeline.yaml`.
- [x] Real n8n URL is not hardcoded in application runtime code.
- [x] Successful HTTP 2xx valid JSON response marks source stage `done`.
- [x] Successful response creates next-stage file from response `payload`.
- [x] Source file is not mutated during execution.
- [x] `stage_runs` audit rows are written.
- [x] Failed attempts schedule `retry_wait` or become `failed`.
- [x] `retry_wait` respects `next_retry_at`.
- [x] Stuck `in_progress` states are reconciled; retryable stuck tasks get a due `next_retry_at`.
- [x] Scanner reconciliation preserves SQLite execution state and does not regress `done` from source JSON `status`.
- [x] Structural blocked next-stage copy after successful HTTP becomes `blocked` with `error_type = copy_blocked`, not retry/failure.
- [x] `run_entity_stage` debug/manual path may bypass retry delay; `run_due_tasks` remains strict about `next_retry_at`.
- [x] Schema migrates to v4 and fresh DBs bootstrap at v4.
- [x] Automated Rust tests use mock HTTP, not the real n8n instance.
- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml` passes in final verification.
- [x] Rust tests pass through `vcvars64.bat` in final verification.
- [x] `npm.cmd run build` passes in final verification.
- [ ] Minimal app-start smoke checked if convenient.

## Out of scope respected

- [x] No background daemon.
- [x] No n8n REST API workflow management.
- [x] No credential manager/authentication UI.
- [x] No full manual UI walkthrough or screenshots claimed.

## Verification notes

- `cargo fmt --manifest-path src-tauri/Cargo.toml`: passed.
- `cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'`: passed, 45 Rust tests.
- `npm.cmd run build`: passed.
- UI smoke was not rerun in this implementation pass; no manual UI walkthrough is claimed.
