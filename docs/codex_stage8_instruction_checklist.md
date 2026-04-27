# Stage 8 Instruction Checklist

## Source Of Truth

- [x] Re-read `instructions/beehive_stage8_codex_task.md` before implementation.
- [x] Reviewed current Workspace Explorer, database, commands, file-open, Entity Detail, domain types, README, and relevant Stage 1-7 context.

## Backend Read Model

- [x] Existing `get_workspace_explorer(path)` is evolved, not duplicated.
- [x] Read path does not scan, run tasks, reconcile, write DB rows, write files, or update YAML.
- [x] Result includes generated time, workdir path, last scan timestamp, stage tree, entity trails, totals, and structured errors.
- [x] Stage tree includes active/inactive stage metadata, folder path/existence, files, invalid files, and counters.
- [x] Runtime status comes from `entity_stage_states`, not JSON status.
- [x] Invalid files from the latest scan are grouped by stage from `app_events`.
- [x] Managed-copy and inferred same-entity trail relations are visible.
- [x] Terminal stages without output folder do not break the read model.

## Frontend

- [x] Workspace Explorer shows grouped/tree stage folders.
- [x] Filters/search cover entity/file/path, stage, runtime status, validation, missing, invalid, inactive, and managed copies.
- [x] Registered file nodes show entity id, file id, stage id, status, validation, presence, managed-copy state, and actions.
- [x] Open file/folder uses backend-managed commands only.
- [x] Go to Entity uses `/entities/:entityId?file_id=:entityFileId`.
- [x] Entity Detail selects `file_id` from query string when it belongs to the entity.
- [x] Empty/loading/error states are handled.

## Tests And Verification

- [x] Rust tests cover fresh tree, registered file, missing file, managed copy, entity trail, invalid event, inactive stage, terminal stage, and read-only behavior.
- [x] Existing open file/folder safety tests still pass.
- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml` was run and recorded.
- [x] Rust tests through `vcvars64.bat` were run and recorded.
- [x] `npm.cmd run build` was run and recorded.

## Documentation

- [x] README describes Stage 8 Workspace Explorer behavior.
- [x] Delivery report records implemented behavior, verification, limitations, and acceptance status.
- [x] No full mouse-driven UI walkthrough is claimed unless actually performed.
