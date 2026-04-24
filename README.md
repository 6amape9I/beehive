# beehive

beehive is a desktop foundation for a local JSON pipeline operator tool. Stage 1 proves the bootstrap chain:

`desktop app -> workdir -> pipeline.yaml -> validation -> SQLite bootstrap -> stage sync -> UI visibility`

## Stack

- Tauri v2
- React
- TypeScript
- SQLite through Rust `rusqlite`
- YAML through Rust `serde_yaml`

## Install

From the repository root:

```powershell
npm.cmd install
```

In this Windows PowerShell environment, use `npm.cmd` instead of `npm` because direct `npm` resolves to a blocked PowerShell script.

Rust/Tauri builds on Windows require Visual Studio C++ tools and a Windows SDK that provides `kernel32.lib`.

## Run

```powershell
npm.cmd run tauri dev
```

If Rust fails with `link.exe not found` or `cannot open input file 'kernel32.lib'`, install the Visual Studio C++ build tools plus a Windows 10/11 SDK, then retry.

## Frontend Build

```powershell
npm.cmd run build
```

This runs TypeScript checking and the Vite production build.

## Backend Tests

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
```

On Windows, run this from a Developer Command Prompt or an environment where MSVC and Windows SDK library paths are available.

## Workdir Flow

The Settings / Diagnostics and Dashboard screens include a Workdir Setup panel.

Use it to:

- choose a folder with the native folder picker,
- initialize a new workdir,
- open an existing workdir,
- reload the current workdir.

Initializing a new workdir creates:

```text
pipeline.yaml
app.db
stages/
logs/
```

The generated `pipeline.yaml` is a valid default config with `ingest` and `normalize` stages.

## Stage 1 Supports

- routable app shell: Dashboard, Entities, Entity Detail, Stage Editor, Workspace Explorer, Settings / Diagnostics,
- explicit initialization state model,
- structured YAML validation issues,
- workdir health checks,
- SQLite schema bootstrap for `settings`, `stages`, `entities`, `entity_stage_states`, `stage_runs`, `app_events`,
- deterministic stage upsert from YAML into SQLite,
- UI visibility for project name, workdir path, config path, database path, config status, database status, stage count, and stage IDs.

## Intentionally Deferred

Stage 1 does not implement:

- n8n workflow execution,
- task queues,
- retry runtime,
- file scanning runtime,
- JSON entity processing,
- stage transition execution,
- graph routing logic,
- full stage CRUD editing,
- runtime history operations.
