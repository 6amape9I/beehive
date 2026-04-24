# beehive

beehive is a local desktop operator tool for JSON stage pipelines.

Stage 3 turns the Stage 2 runtime foundation into a file lifecycle foundation:

`workdir -> active stages -> stage folders -> JSON file instances -> logical entities -> entity_files -> stage states -> managed next-stage copy`

## Stack

- Tauri v2
- React
- TypeScript
- Rust
- SQLite through `rusqlite`
- YAML through `serde_yaml`

## Install

```powershell
npm.cmd install
```

In this PowerShell environment use `npm.cmd`, not `npm`.

## Run

```powershell
npm.cmd run tauri dev
```

On Windows, Rust/Tauri builds require Visual Studio C++ tools and a Windows SDK.

If the shell cannot find `link.exe` or `kernel32.lib`, run from a Developer Command Prompt or load MSVC/SDK paths with `vcvars64.bat`.

## Technical Verification

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'
npm.cmd run build
```

## Workdir Rules

Use an absolute path outside the application directory.

- relative paths are rejected;
- nested paths inside the app/runtime directory are rejected;
- path validation uses normalized/canonical checks to avoid disguised `..` traversal or case-mismatch issues.

Initializing a new workdir creates:

```text
pipeline.yaml
app.db
stages/
logs/
```

## Stage 3 Runtime Model

Stage 3 separates logical entities from physical file instances:

- `entities` = logical aggregate by `entity_id`
- `entity_files` = physical JSON file instances in stage folders
- `entity_stage_states` = logical per-stage runtime state linked to the latest relevant file instance

This allows one logical entity to exist in multiple stage folders at once:

```text
stages/incoming/entity-001.json
stages/normalized/entity-001.json
stages/enriched/entity-001.json
```

Still invalid:

- the same `entity_id` twice in the same stage;
- a registered file path changing to a different `entity_id`;
- destructive overwrite of an existing target file during managed copy.

## `pipeline.yaml`

The app expects `pipeline.yaml` with:

- `project`
- `runtime`
- `stages`

Each stage should define:

- `id`
- `input_folder`
- `output_folder`
- `workflow_url`
- `max_attempts`
- `retry_delay_sec`
- optional `next_stage`

## JSON File Requirements

Eligible files must be:

- `.json` files, case-insensitive;
- regular readable files;
- inside an active stage input folder;
- JSON objects at the root;
- with non-empty `id`;
- with non-null `payload`.

Example:

```json
{
  "id": "entity-0001",
  "current_stage": "incoming",
  "next_stage": "normalized",
  "status": "pending",
  "payload": {
    "value": 1
  },
  "meta": {
    "source": "manual"
  }
}
```

## Reconciliation Scan

`Scan workspace` now performs reconciliation, not only discovery.

It:

1. loads active stages;
2. provisions missing input/output directories;
3. scans active input folders non-recursively;
4. registers new file instances;
5. updates changed file instances;
6. marks missing files instead of deleting rows;
7. restores missing files if they reappear;
8. recomputes logical entity summaries;
9. records file lifecycle app events.

Inactive stages are not scanned for new files, but their historical rows remain visible.

## Managed Next-Stage Copy

Stage 3 adds a safe backend-managed copy operation for future runtime use.

Behavior:

- source file stays unchanged;
- target stage is resolved from file `next_stage`, then stage config `next_stage`;
- target JSON is updated in memory before write;
- write uses a temporary file plus rename for atomicity;
- destructive overwrite is not allowed.

Managed copy updates target JSON fields:

- `current_stage`
- `status`
- `next_stage`
- `meta.updated_at`
- `meta.beehive.copy_source_stage`
- `meta.beehive.copy_target_stage`
- `meta.beehive.copy_created_at`

## UI

Stage 3 surfaces:

- Dashboard: reconciliation summary, present/missing files, managed copy count
- Entities: logical entity rows
- Entity Detail: all file instances plus stage states and managed copy action
- Workspace Explorer: present/missing/invalid/managed-copy file visibility
- Settings / Diagnostics: schema v3, reconciliation summary, file lifecycle events

## Intentionally Deferred

Stage 3 does not implement:

- n8n workflow execution
- HTTP workflow calls
- retry runtime
- task queue worker
- background watcher
- file moves between stages
- automatic source-stage completion
- full JSON editor
- advanced UI polish
