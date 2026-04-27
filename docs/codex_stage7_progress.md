# Stage 7 Progress

## 2026-04-27

- Re-read `instructions/beehive_stage7_codex_task.md`.
- Confirmed Stage 7 scope: safe GUI editor for `pipeline.yaml`, not a visual workflow builder and not n8n management.
- Added backend DTOs for pipeline draft/editor state, usage summaries, validation result, and save result.
- Added `pipeline_editor` backend module with draft validation, usage loading, deterministic YAML serialization, atomic save with backup, SQLite stage sync, directory provisioning, and `pipeline_config_saved` app event.
- Registered Tauri commands for editor state, draft validation, and save.
- Added TypeScript DTOs and runtime API wrappers for Stage 7 editor commands.
- Replaced read-only Stage Editor with a draft-based UI: project/runtime form, stage list, stage form, add/remove, validate/save/discard, validation panel, and backend YAML preview.

## Feedback

- Saved stage IDs are treated as immutable for all saved stages, not only stages with runtime data. This matches the preferred Stage 7 policy and avoids accidental rename/history ambiguity.
- Stage removal is implemented as YAML removal only; existing database lifecycle behavior marks removed stages inactive/archived.
- `project.workdir` remains in the draft for compatibility but will be displayed read-only in the UI because runtime uses the selected workdir path.
- Stage removal is blocked while another draft stage references it via `next_stage`; stages with history require a second explicit click in the UI before removal from the draft.

## Verification

- PASS: `cargo fmt --manifest-path src-tauri/Cargo.toml`
- PASS: `cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'`
  - 83 tests passed.
- PASS: `npm.cmd run build`
