# Stage 1 Instruction Checklist

The authoritative checklist source is `instructions/beehive_stage1_codex_task.md`.

## Definition Of Done

- [ ] Desktop application runs locally.
- [ ] Stable UI shell with working navigation. Implemented; desktop runtime verification blocked by Windows SDK.
- [ ] New workdir can be initialized. Implemented; desktop runtime verification blocked by Windows SDK.
- [ ] Existing workdir can be opened. Implemented; desktop runtime verification blocked by Windows SDK.
- [ ] `pipeline.yaml` is loaded and validated. Implemented; Rust test verification blocked by Windows SDK.
- [ ] `app.db` is created/opened. Implemented; Rust test verification blocked by Windows SDK.
- [ ] SQLite schema bootstrap works. Implemented; Rust test verification blocked by Windows SDK.
- [ ] Stage definitions sync from YAML into SQLite. Implemented; Rust test verification blocked by Windows SDK.
- [ ] Dashboard shows real bootstrap data. Implemented; desktop runtime verification blocked by Windows SDK.
- [ ] Stage Editor shows real stage information. Implemented; desktop runtime verification blocked by Windows SDK.
- [ ] Diagnostics/Settings shows technical initialization data. Implemented; desktop runtime verification blocked by Windows SDK.
- [x] README explains setup and launch.
- [x] Code structure is clean enough for Stage 2.

## Manual Verification Scenarios

- [ ] Fresh app launch.
- [ ] Create a new workdir.
- [ ] Confirm `pipeline.yaml`, `app.db`, `stages/`, and `logs/` are created.
- [ ] Load a valid `pipeline.yaml`.
- [ ] Verify stages are visible in UI.
- [ ] Verify stage definitions are synced into SQLite.
- [ ] Open an existing workdir.
- [ ] Test invalid config scenario and confirm meaningful UI errors.

## Verification Notes

- `npm.cmd run build` passed.
- `cargo test --manifest-path src-tauri/Cargo.toml` could not complete because the Windows SDK library `kernel32.lib` is missing from the machine.
- `npm.cmd run tauri dev` starts Vite when run with elevated command permissions, then fails at Rust linking because the regular shell cannot find `link.exe`.
- Running Cargo through `vcvars64.bat` finds `link.exe`, but still fails because the Windows SDK library `kernel32.lib` is absent.
