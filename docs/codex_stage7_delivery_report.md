# Stage 7 Delivery Report

## Implemented

- Stage Editor now loads `pipeline.yaml` into a draft model.
- Operators can edit project name, runtime settings, stage fields, retry policy, folders, workflow URL, and `next_stage`.
- Operators can add stages and remove stages from active YAML config.
- Validation and save are backend-authoritative.
- Save writes `pipeline.yaml` atomically with a timestamped backup.
- Save syncs SQLite stages and provisions active stage directories.
- Save records `pipeline_config_saved`.

## Backend Commands

- `get_pipeline_editor_state(path)`
- `validate_pipeline_config_draft(path, draft)`
- `save_pipeline_config(path, draft, operator_comment?)`

The editor state command and validation command do not scan files, run tasks, call n8n, or mutate execution state.

## YAML Save / Validation

- YAML is generated deterministically with `serde_yaml`.
- Comments from the original YAML are not preserved.
- Terminal stages serialize with `next_stage: null` and may use an empty `output_folder`.
- Invalid drafts are rejected before write.
- Failed validation does not overwrite existing `pipeline.yaml`.
- Saved stage IDs are immutable.
- Stage folder paths must be relative and stay inside the workdir.
- Cycles, self-loops, duplicate IDs, bad workflow URLs, missing required fields, and unsafe paths are rejected.

## Stage Removal And History Preservation

- Removing a stage means removing it from active `pipeline.yaml`.
- Existing SQLite stage sync marks removed stages inactive/archived.
- Entity files, stage states, and stage runs are preserved.
- Removal is blocked while another draft stage references the target through `next_stage`.

## Frontend UI

- Replaced the read-only Stage Editor with a draft-based editor.
- Added Project/Runtime form, Stage list, Stage form, validation panel, usage summary, and YAML preview components.
- Added dirty state, reload, validate, save, discard, add stage, and remove stage behavior.
- Save reloads editor state and refreshes bootstrap context.

## Tests

Added Rust tests for:

- editor state loading;
- valid save, backup, sync, directory provisioning, app event;
- invalid draft preserving existing YAML;
- duplicate ID, invalid ID, unsafe paths, invalid workflow URL;
- invalid next stage, self-loop, cycle;
- terminal/non-terminal output folder rules;
- saved stage rename rejection;
- removal archiving and runtime history preservation.

## Verification

- PASS: `cargo fmt --manifest-path src-tauri/Cargo.toml`
- PASS: `cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'`
  - Result: 83 passed, 0 failed.
- PASS: `npm.cmd run build`
  - Result: TypeScript compile and Vite production build completed.

No real n8n endpoint was called. No mouse-driven UI walkthrough was performed.

## Known Limitations

- Stage 7 does not preserve YAML comments.
- Stage rename is intentionally deferred.
- No React Flow graph editor, n8n REST workflow management, credential management, scheduler, or file movement is implemented.

## Acceptance Status

Stage 7 is ready for review against the requested scope.
