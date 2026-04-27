# beehive — Stage 7 Codex Task

# Stage Editor and Pipeline Configuration UI

Ты работаешь в проекте **beehive**.

Этот документ является **единственным источником истины** для реализации **Stage 7**. Перед началом работы обязательно перечитай:

- `README.md`;
- `instructions/beehive_stage1_codex_task.md`;
- `instructions/beehive_stage2_codex_task.md`;
- `instructions/beehive_stage3_codex_task.md`;
- `instructions/beehive_stage4_codex_task.md`;
- `instructions/beehive_stage5_codex_task.md`;
- `instructions/beehive_stage5_5_codex_task.md`;
- `instructions/beehive_stage6_codex_task.md`;
- последнюю polish-инструкцию после Stage 6, если она уже выполнена;
- delivery reports/checklists по Stage 1–6;
- текущий код `src-tauri/src/config`, `database`, `commands`, `workdir`, `StageEditorPage`.

Не полагайся на память.

---

# 0. Контекст текущего состояния

К текущему моменту проект уже имеет:

- Tauri + React + TypeScript desktop foundation.
- Workdir model.
- `pipeline.yaml`.
- SQLite runtime DB.
- Stage config loading and validation.
- Stage sync from YAML into SQLite.
- Discovery scanner.
- Entity/file runtime model.
- n8n execution foundation.
- Retry/reconciliation.
- Dashboard.
- Entities Table.
- Entity Detail with manual actions and JSON viewer/editor.
- Current `StageEditorPage`, который пока является read-only table of stages.

Stage 7 должен превратить read-only Stage Editor в реальный UI управления `pipeline.yaml`.

---

# 1. Главная цель Stage 7

Реализовать операторский редактор stages и pipeline configuration через GUI.

Пользователь должен иметь возможность без ручного редактирования `pipeline.yaml`:

1. видеть текущий pipeline config;
2. редактировать project/runtime settings;
3. создавать stage;
4. редактировать stage;
5. удалять stage из active config с безопасными ограничениями;
6. задавать input/output folders;
7. задавать `workflow_url`;
8. задавать retry policy;
9. задавать `next_stage`;
10. валидировать draft перед сохранением;
11. сохранить изменения в `pipeline.yaml`;
12. автоматически перечитать config и синхронизировать SQLite stages;
13. видеть ошибки валидации и последствия изменений.

---

# 2. Что НЕ входит в Stage 7

Не реализовывать:

- React Flow drag-and-drop graph editor.
- Complex branching/routing rules beyond single `next_stage`.
- n8n REST API workflow management.
- Credential manager.
- Secrets vault.
- Background scheduler.
- Multi-user conflict resolution.
- Git-like version history.
- Full low-code pipeline builder.
- Batch migration of existing entities between renamed stages.
- Automatic movement of files after config changes.
- Deleting historical runtime rows from SQLite.

Stage 7 — это YAML pipeline editor, а не visual workflow builder.

---

# 3. Product rules

## 3.1. Source of truth

- `pipeline.yaml` remains source of truth for pipeline configuration.
- SQLite `stages` table is synchronized from YAML.
- Historical rows in SQLite must not be destructively deleted just because a stage is removed from YAML.
- Runtime state remains in SQLite.
- JSON files remain business artifacts.

## 3.2. Save model

Stage Editor should use a draft model:

```text
load current pipeline.yaml
        ↓
edit draft in UI
        ↓
validate draft
        ↓
save pipeline.yaml atomically
        ↓
bootstrap/sync SQLite stages
        ↓
reload app initialization/runtime state
```

Do not mutate YAML on every keystroke.

## 3.3. Stage delete model

Deleting a stage in Stage 7 means:

```text
remove stage from pipeline.yaml active config
```

It does **not** mean deleting SQLite historical records.

Existing `sync_stages` behavior may mark missing stages inactive/archived. Preserve this behavior.

## 3.4. Stage ID immutability

For existing stages that already have runtime data or DB entity_count > 0:

- `stage.id` must be immutable.
- UI should disable editing stage ID or show that rename is not allowed.

For newly created draft stages:

- `id` is editable until saved.

For existing stages with no runtime data:

- ID rename may be allowed only if implementation is simple and safe.
- Preferred Stage 7 policy: keep existing stage IDs immutable for all saved stages.
- Rename can be implemented later as explicit “clone + archive old” workflow.

## 3.5. Terminal stage

Terminal stage:

```text
next_stage = null / empty
```

Rules:

- `output_folder` may be empty.
- UI should show output folder as not required when no `next_stage`.
- If `next_stage` is set, `output_folder` is required.

## 3.6. `workflow_url`

For Stage 7:

- `workflow_url` is required for executable stages.
- URL must start with `http://` or `https://`.
- Do not call n8n during validation.
- Do not manage n8n workflows.

---

# 4. Backend requirements

## 4.1. Add pipeline editor backend model

Add Rust DTOs for editor state.

Suggested shapes:

```rust
PipelineEditorState {
    config: PipelineConfig,
    yaml_text: String,
    validation: ConfigValidationResult,
    stage_usages: Vec<StageUsageSummary>,
    loaded_at: String,
}

StageUsageSummary {
    stage_id: String,
    is_active: bool,
    entity_count: u64,
    entity_file_count: u64,
    stage_state_count: u64,
    run_count: u64,
    can_remove_from_config: bool,
    can_rename: bool,
    warnings: Vec<String>,
}
```

Names can differ, but frontend must get:

- current typed config;
- current YAML text or serialized YAML preview;
- validation result;
- usage info per stage;
- allowed structural actions.

## 4.2. Backend commands

Add Tauri commands:

```text
get_pipeline_editor_state(path)
validate_pipeline_config_draft(path, draft_config)
save_pipeline_config(path, draft_config, operator_comment?)
```

Alternative naming is okay if consistent with project conventions.

### 4.2.1. `get_pipeline_editor_state`

Must:

1. load runtime context/workdir;
2. read `pipeline.yaml`;
3. parse typed config;
4. run validation;
5. load stage usage from SQLite;
6. return editor DTO.

Must not:

- run scanner;
- run n8n;
- mutate execution state.

### 4.2.2. `validate_pipeline_config_draft`

Must:

1. validate a draft config submitted from frontend;
2. return structured validation issues;
3. return normalized config if valid;
4. return warnings for dangerous-but-allowed changes, such as removing a stage with historical data.

Must not write YAML or DB.

### 4.2.3. `save_pipeline_config`

Must:

1. validate draft config using the same validation as loader.
2. reject save if validation has errors.
3. apply safety rules:
   - no duplicate stage IDs;
   - no invalid next_stage refs;
   - no missing output_folder for non-terminal;
   - no invalid workflow_url;
   - no rename of existing stage ID through accidental edit;
   - no delete of all stages unless intentionally allowed. Prefer reject empty stage list for Stage 7.
4. serialize draft to YAML deterministically.
5. create backup of previous `pipeline.yaml`, unless there is a strong reason not to.
6. write `pipeline.yaml` atomically:
   - write temp file;
   - fsync if practical;
   - rename.
7. bootstrap/sync SQLite stages with saved config.
8. provision missing input/output directories for active stages.
9. record app event:
   - `pipeline_config_saved`;
   - operator comment;
   - stage count;
   - added/removed/updated stage ids.
10. return updated editor state and app initialization state if convenient.

## 4.3. Atomic YAML write

Implement helper such as:

```rust
write_pipeline_yaml_atomic(path, yaml_text)
```

Rules:

- Never leave partial `pipeline.yaml`.
- Temp file must be in same directory.
- On failure, leave original file intact.
- If backup is implemented, backup name can be:

```text
pipeline.yaml.bak.20260427T150000Z
```

Do not write backups endlessly in tests without tempdir cleanup.

## 4.4. YAML serialization

Use `serde_yaml` or existing config tooling.

Output does not need to preserve comments from original YAML in Stage 7.

But output must be:

- deterministic enough for review;
- valid for existing loader;
- include:
  - project;
  - runtime;
  - stages.

Ensure terminal `next_stage` serializes as null/empty consistently. Prefer `next_stage: null` or omit field if loader supports it. Pick one and document.

## 4.5. Validation rules

Stage 7 validation must include all existing validation plus additional editor-level checks.

Required:

### Project

- `project.name` non-empty.
- `project.workdir` non-empty.
- In most workdir-based usage, keep `project.workdir` as `.` unless user explicitly changes it. If editing is supported, warn that runtime still uses selected workdir.

### Runtime

- `scan_interval_sec >= 1`.
- `max_parallel_tasks >= 1`.
- `stuck_task_timeout_sec >= 1`.
- `request_timeout_sec >= 1`.
- `file_stability_delay_ms >= 0`.

### Stages

- at least one stage.
- `id` required.
- `id` must be unique.
- `id` format should be safe:
  - letters, numbers, `_`, `-`;
  - no spaces;
  - no slash/backslash;
  - no `..`.
- `input_folder` required.
- `input_folder` must be relative to workdir.
- `input_folder` must not escape workdir.
- `output_folder` required when `next_stage` is configured.
- `output_folder`, if present, must be relative to workdir.
- `workflow_url` required.
- `workflow_url` starts with `http://` or `https://`.
- `max_attempts >= 1`.
- `retry_delay_sec >= 0`.
- `next_stage`, if set, references an existing stage.
- `next_stage` cannot equal current stage.
- full cycle detection should warn or error. Preferred: error for cycles in Stage 7, because one-primary-next-stage model is expected to be mostly linear and cycles can cause operator confusion.

### Deletion/removal warnings

If draft removes a stage that exists in SQLite with historical data:

- allowed as archive/remove-from-active-config;
- show warning:
  - stage will become inactive/archived;
  - existing entities/files/runs remain visible historically;
  - scanner will no longer scan its folders as active input.

Do not delete DB rows.

## 4.6. Stage usage summaries

Backend must compute usage per stage from SQLite:

- active/inactive in DB;
- entity_count;
- entity_file_count;
- stage_state_count;
- run_count;
- last_seen_in_config_at;
- archived_at.

Use this to decide:

- can_remove_from_config;
- can_rename;
- warnings.

For Stage 7:

- can_remove_from_config may be true even with usage, but with warning.
- can_rename should be false for saved stages.

## 4.7. Directory provisioning after save

After successful save:

- create missing active stage input directories;
- create output directories for non-terminal stages;
- do not create output directory for terminal stage with empty output;
- log created directories if any.

Use existing discovery directory provisioning logic if possible.

---

# 5. Frontend requirements

## 5.1. Replace read-only StageEditorPage with editor UI

Current StageEditorPage is read-only. Replace or extend it into a real editor with draft state.

Page must include:

1. Header with workdir/project context.
2. Load/reload button.
3. Unsaved changes indicator.
4. Project/runtime settings section.
5. Stage list/table.
6. Stage form for selected stage.
7. Add stage button.
8. Remove from config button.
9. Validation issues panel.
10. Save pipeline config button.
11. Discard changes button.
12. Optional YAML preview panel.

## 5.2. Project/runtime settings UI

Editable fields:

- project.name;
- project.workdir, or read-only if you decide this should not be changed in Stage 7;
- runtime.scan_interval_sec;
- runtime.max_parallel_tasks;
- runtime.stuck_task_timeout_sec;
- runtime.request_timeout_sec;
- runtime.file_stability_delay_ms.

Numeric fields must validate client-side before save, but backend remains authoritative.

## 5.3. Stage list UI

For each draft stage show:

- stage id;
- input folder;
- output folder / terminal;
- workflow_url compact;
- retry policy;
- next stage;
- usage summary;
- validation state;
- active/inactive if known from DB.

Selecting a stage opens it in the form.

## 5.4. Stage form UI

Fields:

- `id`
  - editable for new stage;
  - read-only for existing saved stage;
- `input_folder`;
- `output_folder`;
- `workflow_url`;
- `max_attempts`;
- `retry_delay_sec`;
- `next_stage`.

`next_stage` should be a select with:

- `End / terminal`;
- all other stages.

When `next_stage` is terminal:

- output_folder field should be optional;
- label should say “optional for terminal stage”.

When `next_stage` is set:

- output_folder field required.

## 5.5. Add stage

Add button creates a draft stage with safe defaults:

```yaml
id: new_stage
input_folder: stages/new_stage
output_folder: stages/new_stage_out
workflow_url: http://localhost:5678/webhook/new_stage
max_attempts: 3
retry_delay_sec: 10
next_stage: null
```

If `new_stage` exists, generate `new_stage_2`, etc.

## 5.6. Remove stage

Remove from config button:

- removes stage from draft `stages`;
- does not delete DB rows;
- if stage has usage, show confirmation UI or warning panel;
- if any other stage points to it via `next_stage`, either:
  - block removal until links are cleared, or
  - automatically clear those links only with explicit confirmation.

Preferred for Stage 7: block removal while other draft stages reference it, with clear message.

## 5.7. Unsaved changes

Page must track dirty state.

If user has unsaved changes:

- show indicator;
- Save button enabled;
- Discard button enabled.

No need to implement browser navigation guard unless easy.

## 5.8. Validation UX

Validation should run:

- client-side minimally while editing;
- backend validation when user clicks Validate or Save.

Show validation issues grouped by:

- project;
- runtime;
- specific stage;
- graph/links.

Existing `ValidationIssues` component may be reused.

## 5.9. YAML preview

Add optional panel showing generated YAML preview from draft.

It can be read-only.

If backend returns serialized YAML preview, use that. Otherwise frontend can display a simple JSON-like preview, but backend-generated YAML is preferred.

## 5.10. Save UX

On Save:

1. disable buttons while saving;
2. call backend `save_pipeline_config`;
3. if validation errors, show them and do not write;
4. if success:
   - show success message;
   - reload editor state;
   - update bootstrap context if possible by calling reload workdir;
   - keep user on Stage Editor.

---

# 6. Frontend components

Prefer splitting Stage Editor into components.

Suggested layout:

```text
src/components/stage-editor/
  ProjectRuntimeForm.tsx
  StageDraftList.tsx
  StageDraftForm.tsx
  StageValidationPanel.tsx
  StageUsageSummary.tsx
  PipelineYamlPreview.tsx
```

`StageEditorPage.tsx` should orchestrate state and API calls, not contain all rendering logic.

---

# 7. Frontend API/types

Add API wrappers in `runtimeApi.ts`:

```ts
getPipelineEditorState(path)
validatePipelineConfigDraft(path, draftConfig)
savePipelineConfig(path, draftConfig, operatorComment?)
```

Add TypeScript types:

- `PipelineEditorState`
- `StageUsageSummary`
- `PipelineConfigDraft`
- `PipelineConfigValidationResult` if needed
- `SavePipelineConfigResult`
- any command result wrappers.

Keep Rust serialization and TypeScript names aligned.

---

# 8. Backend command safety

## 8.1. Path safety

Saving config must only write:

```text
<selected_workdir>/pipeline.yaml
```

Never allow frontend to supply arbitrary config path.

## 8.2. Folder safety

Stage folder fields must be relative and stay inside workdir.

Reject:

- absolute paths;
- paths containing `..` that escape workdir;
- path equal to workdir root if dangerous;
- path under app directory outside workdir.

Use existing path safety helpers where possible.

## 8.3. No runtime mutation

Validation/get editor state must not:

- scan files;
- run tasks;
- call n8n;
- mutate execution states.

Save is allowed to:

- write YAML;
- sync stages;
- provision directories;
- record app event.

Save must not:

- move JSON files;
- delete JSON files;
- delete DB historical records;
- run n8n.

---

# 9. SQLite behavior

## 9.1. No schema change expected

Stage 7 likely does not need schema migration.

If no persistent columns are added, keep current schema version.

If you add columns, justify and implement migration. Prefer avoiding schema change.

## 9.2. Stage sync

After save, existing `bootstrap_database` / `sync_stages` should:

- insert new stages;
- update existing active stages;
- mark removed stages inactive/archived;
- preserve historical entity/runtime data.

If current sync does not fully support this, fix it carefully and add tests.

## 9.3. App events

On successful save, record:

```text
pipeline_config_saved
```

Context should include:

- added_stage_ids;
- updated_stage_ids;
- removed_stage_ids;
- stage_count;
- operator_comment;
- backup path if created.

On rejected save due validation:

- optionally record warning `pipeline_config_save_rejected`;
- do not spam app_events for every keystroke validation.

---

# 10. Tests

Add Rust tests.

## 10.1. Config/editor backend tests

Cover:

1. `get_pipeline_editor_state` returns current config and stage usage.
2. Valid draft saves to `pipeline.yaml`.
3. Save creates backup or otherwise preserves old file on failure.
4. Invalid draft does not overwrite existing YAML.
5. Duplicate stage id rejected.
6. Missing output_folder for non-terminal rejected.
7. Missing output_folder for terminal accepted.
8. Invalid workflow_url rejected.
9. Invalid next_stage rejected.
10. Self-loop rejected.
11. Cycle rejected or clearly warned according to chosen policy.
12. Removing a stage from YAML marks it inactive/archived in SQLite after sync.
13. Historical entity/stage_state/stage_runs rows are not deleted after stage removal.
14. Adding a new stage syncs it into SQLite.
15. Missing directories for new stage are provisioned after save.
16. Save records `pipeline_config_saved`.

## 10.2. Path safety tests

Cover:

- absolute input folder rejected;
- `../outside` rejected;
- output folder escaping workdir rejected;
- normal `stages/foo` accepted.

## 10.3. Frontend build

No heavy frontend test infra required unless already present.

Required:

```text
npm.cmd run build
```

## 10.4. Regression tests

Existing tests for:

- workdir bootstrap;
- scanner;
- runtime execution;
- Stage 5 dashboard;
- Stage 6 entities/detail;
- JSON editor policy;
- manual actions;

must still pass.

---

# 11. Documentation

Create/update:

```text
docs/codex_stage7_progress.md
docs/codex_stage7_instruction_checklist.md
docs/codex_stage7_delivery_report.md
```

Update README with:

- Stage Editor can now edit `pipeline.yaml`;
- save is atomic;
- removed stages become inactive/archived, not deleted from DB history;
- stage ID rename policy;
- terminal output_folder rule;
- no n8n REST management.

## 11.1. Delivery report must include

- implemented backend commands;
- YAML save behavior;
- validation rules;
- stage removal/archival behavior;
- directory provisioning behavior;
- frontend components;
- tests run;
- known limitations;
- whether Stage 7 is ready for review.

## 11.2. Checklist must include

At least:

- [ ] Editor state command implemented.
- [ ] Draft validation command implemented.
- [ ] Save pipeline config command implemented.
- [ ] Atomic YAML write implemented.
- [ ] Backup or failure-safe strategy implemented.
- [ ] Stage add/edit/remove UI implemented.
- [ ] Project/runtime settings UI implemented.
- [ ] Stage ID immutability enforced.
- [ ] Terminal output rule enforced.
- [ ] Path safety enforced.
- [ ] Removed stages preserve DB history.
- [ ] SQLite stage sync after save works.
- [ ] Directories provisioned after save.
- [ ] Validation errors shown in UI.
- [ ] Save/discard/dirty state works.
- [ ] README updated.
- [ ] Rust tests added.
- [ ] `cargo fmt` passed.
- [ ] Rust tests passed.
- [ ] `npm.cmd run build` passed.
- [ ] No real n8n endpoint called.
- [ ] No manual UI walkthrough claimed unless actually performed.

---

# 12. Verification commands

Run and report:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
```

```powershell
cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'
```

```powershell
npm.cmd run build
```

If exact environment differs, document actual commands.

---

# 13. Acceptance criteria

Stage 7 can be accepted only if:

1. User can load current pipeline config into Stage Editor.
2. User can edit project/runtime settings.
3. User can add a new stage.
4. User can edit stage fields.
5. Saved existing stage IDs are immutable.
6. User can remove a stage from active YAML config.
7. Removed stage historical DB data remains.
8. Removed stage becomes inactive/archived in SQLite after sync.
9. `next_stage` selection works.
10. Terminal stages can omit output folder.
11. Non-terminal stages require output folder.
12. Invalid configs are rejected before save.
13. Validation issues are shown clearly in UI.
14. Save writes `pipeline.yaml` atomically.
15. Failed save does not corrupt existing `pipeline.yaml`.
16. Save syncs SQLite stages.
17. Save provisions new stage directories.
18. Save records app event.
19. Dashboard/Entities continue to work after save.
20. Stage Editor does not call n8n or run tasks.
21. Tests cover backend validation/save/sync.
22. `cargo fmt` passes.
23. Rust tests pass.
24. `npm.cmd run build` passes.
25. README/docs updated honestly.

---

# 14. Expected Codex final response

```md
# Stage 7 Delivery Summary

## Implemented
...

## Backend commands
...

## YAML save / validation
...

## Stage removal and history preservation
...

## Frontend UI
...

## Files changed
...

## Tests
...

## Verification
...

## Known limitations
...

## Acceptance status
...
```

---

# 15. Final instruction

Stage 7 is about giving the operator safe control over `pipeline.yaml` from the desktop app.

Prioritize correctness and safety:

- do not corrupt config;
- do not delete runtime history;
- do not make hidden runtime changes;
- validate before saving;
- make consequences visible.

A smaller safe Stage Editor is better than a large fragile visual builder.
