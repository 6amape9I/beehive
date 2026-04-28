# beehive Release Checklist

## Prerequisites

- Node.js and npm installed.
- Rust toolchain installed.
- Windows: Visual Studio C++ tools and Windows SDK available through `vcvars64.bat`.

## Technical Commands

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'
npm.cmd run build
npm.cmd run release
```

## Demo Cleanup

```powershell
npm.cmd run demo:reset
```

Confirm no generated bulk files are left in `demo/workdir/stages/incoming` unless intentionally included.

## Manual QA

Complete `docs/stage9_manual_qa_checklist.md` and record actual results in `docs/stage9_manual_qa_results.md`.

Do not mark release ready if the manual QA pass is incomplete or if release build fails.

## Artifacts

Tauri release artifacts are produced by the configured Tauri build under `src-tauri/target/release/bundle` when `npm.cmd run release` succeeds.

## Known Limitations

- No background daemon or scheduler.
- No n8n REST workflow management.
- No config repair mode.
- Real n8n endpoint availability is external to the app.
