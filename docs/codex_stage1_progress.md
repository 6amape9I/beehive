# Stage 1 Progress Log

## 2026-04-23

- Re-read `instructions/beehive_stage1_codex_task.md`.
- Confirmed repository is a near-empty foundation: `README.md`, `.gitignore`, `.idea`, `.venv`, and `instructions/`.
- Created initial Tauri/React project configuration.
- Created Stage 1 execution, question, progress, checklist, and delivery report documents.
- Implemented Rust backend boundaries for domain, config validation, workdir initialization/opening, SQLite schema bootstrap, stage sync, and Tauri commands.
- Implemented React shell, navigation, workdir setup, dashboard bootstrap visibility, Stage Editor, diagnostics, and required placeholder routes.
- Re-read the Stage 1 specification and execution plan after the backend/UI implementation checkpoint.
- Installed npm dependencies.
- Adjusted Vite dependencies to versions compatible with Node v22.10.0 in this environment.
- Ran `npm.cmd run build` successfully.
- Downloaded Rust crates, but Rust/Tauri verification is blocked by missing Windows SDK linker libraries. The regular shell cannot find `link.exe`; Visual Studio `vcvars64.bat` exposes `link.exe`, but `kernel32.lib` is not installed.

## 2026-04-24

- Re-read `instructions/beehive_stage1_codex_task.md` and `instructions/beehive_stage1_codex_polish_task.md` before the Stage 1 polish pass.
- Fixed stage sync so the `stages` table is now an exact mirror of `pipeline.yaml`, including hard deletion of stale stage rows removed from config.
- Simplified the initialization phase model to the states that are actually reachable in Stage 1: `app_not_configured`, `config_invalid`, `bootstrap_failed`, `fully_initialized`.
- Expanded Rust tests for workdir initialization, opening an existing workdir, invalid config handling, duplicate stage ids, SQLite schema bootstrap, and stage sync update/removal behavior.
- Re-verified Rust/Tauri commands through `vcvars64.bat`; `cargo test --manifest-path src-tauri/Cargo.toml` now passes in this environment.
- Manually verified fresh app launch, new workdir initialization, filesystem artifacts, valid config loading, stage visibility, SQLite stage sync, invalid config handling, and stage update/removal sync behavior.
- Reproduced a dev-only relaunch when a relative workdir path resolved inside `src-tauri/` and SQLite writes triggered the `tauri dev` watcher.
- Fixed the workdir path flow so manual input must be absolute and outside the application directory, which prevents the `Open Existing Workdir` relaunch scenario caused by choosing a workdir inside the app tree.
- Re-tested `Open Existing Workdir` with `F:/pycharm_projects/beehive/test-workdirs/manual-open-existing`; the app stayed on the same PID and reached `fully_initialized`.
