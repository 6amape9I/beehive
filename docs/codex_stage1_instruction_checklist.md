# Stage 1 Instruction Checklist

The authoritative checklist source is `instructions/beehive_stage1_codex_task.md`.

## Definition Of Done

- [x] Desktop application runs locally.
- [x] Stable UI shell with working navigation.
- [x] New workdir can be initialized.
- [x] Existing workdir can be opened.
- [x] `pipeline.yaml` is loaded and validated.
- [x] `app.db` is created/opened.
- [x] SQLite schema bootstrap works.
- [x] Stage definitions sync from YAML into SQLite.
- [x] Dashboard shows real bootstrap data.
- [x] Stage Editor shows real stage information.
- [x] Diagnostics/Settings shows technical initialization data.
- [x] README explains setup and launch.
- [x] Code structure is clean enough for Stage 2.

## Manual Verification Scenarios

- [x] Fresh app launch.
- [x] Create a new workdir.
- [x] Confirm `pipeline.yaml`, `app.db`, `stages/`, and `logs/` are created.
- [x] Load a valid `pipeline.yaml`.
- [x] Verify stages are visible in UI.
- [x] Verify stage definitions are synced into SQLite.
- [x] Open an existing workdir.
- [x] Test invalid config scenario and confirm meaningful UI errors.

## Verification Notes

- `npm.cmd run build` passed.
- `cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'` passed.
- `tauri dev` works when launched through the same `vcvars64.bat` wrapper.
- Stage sync behavior was re-checked after update/removal of stages in `pipeline.yaml`; SQLite now mirrors YAML exactly, including removal of stale rows.
- Relative manual workdir paths are now rejected, and workdir paths inside the application directory are rejected to prevent dev-mode rebuild/relaunch loops.
