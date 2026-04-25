# Stage 5 Instruction Checklist

The authoritative checklist source is `instructions/beehive_stage5_codex_task.md`.

## Core acceptance

- [x] Dashboard overview command implemented.
- [x] Dashboard frontend wired to real data.
- [x] Stage graph displays stages and edges.
- [x] Stage graph does not draw fake order-based arrows.
- [x] Stage graph shows real edges explicitly.
- [x] Invalid/missing/inactive edges are visible.
- [x] Stage counters are aggregated from SQLite.
- [x] Stage counters table exposes total, queued, skipped, unknown, existing files, and missing files.
- [x] Active tasks block is shown.
- [x] Queued tasks are included consistently in active tasks.
- [x] Last errors block is shown.
- [x] Recent runs/activity block is shown.
- [x] Operational buttons refresh data.
- [x] Empty/loading/error states handled.
- [x] Backend tests added.
- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml` passed.
- [x] Rust tests passed through `vcvars64.bat`.
- [x] `npm.cmd run build` passed.
- [x] No full manual UI walkthrough claimed.

## Scope guardrails

- [x] Stage 5 plan keeps runtime execution manual only.
- [x] Stage 5 plan avoids background daemon, scheduler, worker pool, and n8n REST API.
- [x] Stage 5 plan keeps Dashboard read path from scanning filesystem or running tasks automatically.

## Verification notes

- `cargo fmt --manifest-path src-tauri/Cargo.toml`: passed.
- `cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'`: passed, 51 Rust tests.
- `npm.cmd run build`: passed.
- UI smoke/manual walkthrough was not run; no manual UI QA is claimed.
