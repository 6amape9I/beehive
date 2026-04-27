# Stage 8 Delivery Report

Status: completed.

## Implemented

- Stage 8 Workspace Explorer read model and UI.
- Existing `get_workspace_explorer(path)` was evolved instead of adding a second endpoint.
- Explorer read path is read-only: no scan, no n8n execution, no stuck-task reconciliation, no DB writes, no file writes, and no YAML updates.
- Existing backend-safe `open_entity_file` and `open_entity_folder` commands are reused.

## Backend Read Model

- `WorkspaceExplorerResult` now includes generated timestamp, workdir path, last scan timestamp, stage tree, entity trails, totals, and structured errors.
- Stage tree includes stage metadata, active/inactive state, archived timestamp, input folder, optional output folder, folder path/existence, registered files, invalid last-scan items, and counters.
- File nodes include `entity_file_id`, `entity_id`, `stage_id`, path, presence, missing timestamp, managed-copy metadata, validation metadata, checksum, size, mtime, and safe open flags.
- Runtime status is loaded from SQLite `entity_stage_states`; JSON/file status is exposed separately as `file_status`.
- Invalid files are loaded from latest-scan `app_events`.
- Entity trails are built from `entity_files`, using `copy_source_file_id` when available and conservative inferred same-entity stage-sequence edges otherwise.
- SQLite is opened read-only for the explorer read model.

## Workspace Explorer UI

- Replaced the simple grouped file list with a stage-tree explorer using accessible `details`/`summary` panels.
- Added workdir summary cards, last scan/generated timestamps, manual Refresh, and explicit `Scan workspace`.
- Added search and filters for stage, runtime status, validation status, missing files, invalid files, inactive stages, and managed copies.
- Registered files show entity/file ids, stage, runtime status, file status, validation, presence, managed-copy source, checksum, updated time, and actions.
- Invalid files from the latest scan are shown under their stage.
- No edit/run/delete/move actions were added to Workspace Explorer.

## Entity Trail / Artifact Connectivity

- Selecting a file shows all known file instances for that entity.
- The trail panel shows stage id, file id, path, present/missing state, runtime status, managed-copy flag, and relation edges.
- Managed-copy edges are marked separately from inferred same-entity stage-sequence edges.

## Deep Linking To Entity Detail

- Workspace Explorer navigates to `/entities/:entityId?file_id=:entityFileId`.
- Entity Detail now reads `file_id` from the query string.
- If the file belongs to the entity, it is selected; otherwise Entity Detail falls back to latest/first file.

## Tests

- Added Rust coverage for:
  - fresh workdir stage tree with zero counters;
  - registered present file visibility;
  - missing registered file visibility/counter;
  - managed-copy relationship visibility;
  - same-entity multi-stage trail;
  - invalid latest-scan file visibility;
  - inactive stage visibility with historical data;
  - terminal stage with no output folder;
  - read-only state preservation.
- Existing open file/folder safety tests still pass.

## Verification

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
```

Result: pass.

```powershell
cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'
```

Result: pass, 86 passed.

```powershell
npm.cmd run build
```

Result: pass.

No real n8n endpoint was called. No mouse-driven UI walkthrough was performed.

## Known Limitations

- Live display of currently present but unregistered JSON files is deferred for Stage 8; invalid files from the latest scan are shown instead.
- Entity trail relations are conservative: exact copy links use `copy_source_file_id`; otherwise the relation is marked as inferred.

## Acceptance Status

Accepted for Stage 8 implementation criteria based on automated verification. Full manual UI walkthrough was not performed or claimed.
