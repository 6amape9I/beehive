# beehive — Stage 8 Codex Task

# Workspace Explorer / File Tree and Artifact Connectivity

Ты работаешь в проекте **beehive**.

Этот документ является **единственным источником истины** для реализации **Stage 8**. Цель этапа — дать оператору физическую картину `workdir`, связать stage-папки и JSON-файлы с логическими сущностями в SQLite, а также показать путь/копии артефакта по пайплайну.

Перед началом работы обязательно перечитай:

- `README.md`;
- `instructions/beehive_stage1_codex_task.md`;
- `instructions/beehive_stage2_codex_task.md`;
- `instructions/beehive_stage3_codex_task.md`;
- `instructions/beehive_stage4_codex_task.md`;
- `instructions/beehive_stage5_codex_task.md`;
- `instructions/beehive_stage5_5_codex_task.md`;
- `instructions/beehive_stage6_codex_task.md`;
- `instructions/beehive_stage6_polish_codex_task.md`;
- `instructions/beehive_stage7_codex_task.md`;
- delivery reports/checklists по Stage 1–7 в `docs/`;
- текущий код:
  - `src-tauri/src/database`;
  - `src-tauri/src/discovery`;
  - `src-tauri/src/file_open`;
  - `src-tauri/src/file_ops`;
  - `src-tauri/src/commands`;
  - `src/pages/WorkspaceExplorerPage.tsx`;
  - `src/pages/EntityDetailPage.tsx`;
  - `src/types/domain.ts`.

Не полагайся на память и не переписывай архитектуру без необходимости.

---

## 0. Накопительный polish backlog, но НЕ scope Stage 8

После Stage 7 архитектурное ревью зафиксировало несколько некритичных polish-пунктов:

1. Сделать backup filename для `pipeline.yaml.bak...` устойчивым к двум save в одну секунду.
2. Позже добавить config repair mode, чтобы Stage Editor мог помочь чинить invalid `pipeline.yaml`.
3. В command wrapper для draft validation возвращать validation issue при backend error, а не `is_valid = true` с отдельным `errors`.
4. Позже разделить `pipeline_editor/mod.rs` на несколько подмодулей, если он продолжит расти.

**Не реализуй эти пункты в Stage 8**, если не столкнёшься с ними как с прямым блокером. Они остаются накопительным polish backlog.

Stage 8 должен быть сфокусирован на Workspace Explorer / File Tree.

---

## 1. Контекст текущего состояния

К текущему моменту проект уже имеет:

- Tauri + React + TypeScript desktop foundation.
- Workdir model.
- `pipeline.yaml`.
- SQLite runtime DB.
- YAML config loading/validation.
- Stage Editor with draft validation and YAML save.
- Discovery scanner.
- Logical `entities`.
- Physical `entity_files`.
- `entity_stage_states`.
- `stage_runs`.
- `app_events`.
- n8n execution foundation.
- Retry/reconciliation.
- Dashboard.
- Entities Table.
- Entity Detail with timeline, stage runs, manual actions, JSON viewer/editor, open file/folder.
- Existing Workspace Explorer page/read model from earlier stages, but not yet a full operator-oriented file tree and artifact-connectivity view.

Stage 8 должен развить Workspace Explorer из простого списка/группировки в полноценный read-only навигатор по физическим файлам и их логическим связям.

---

## 2. Главная цель Stage 8

Реализовать экран **Workspace Explorer / File Tree**, который позволяет оператору видеть:

1. Физическую структуру `workdir`.
2. Stage-папки и их состояние.
3. JSON-файлы внутри stage-папок.
4. Связь каждого зарегистрированного файла с `entity_id`, `entity_file_id`, `stage_id`, runtime status и validation status.
5. Missing files, invalid files, managed copies, inactive-stage artifacts.
6. Копии одной сущности по разным stages.
7. Путь движения артефакта по stage-пайплайну.
8. Быстрый переход из файла в Entity Detail.
9. Быстрое открытие файла или папки через backend command.
10. Последние scan/reconciliation diagnostics.

Stage 8 — это **read-only explorer**. Он не должен изменять JSON, двигать файлы, запускать n8n, редактировать YAML или менять runtime state.

---

## 3. Что НЕ входит в Stage 8

Не реализовывать:

- background watcher;
- automatic filesystem scan on page load;
- automatic n8n execution;
- drag-and-drop moving files between stages;
- deleting files;
- renaming files;
- editing JSON from Workspace Explorer;
- editing pipeline config from Workspace Explorer;
- React Flow graph builder;
- n8n REST workflow management;
- credential manager;
- distributed filesystem sync;
- full audit explorer;
- OS-level file operations beyond already approved open file/open folder;
- recursive indexing of the whole workdir if it risks performance/regression.

Stage 8 may add small UI helpers, but it must remain an explorer, not a file manager.

---

## 4. UX target

The user should be able to answer these questions from Workspace Explorer:

- What stage folders exist in this workdir?
- Which stage folders are active/inactive?
- Which JSON files are registered in SQLite?
- Which files are missing on disk but still tracked?
- Which files are invalid/unregistered from the last scan?
- Which files are managed copies?
- For this entity, where are all its physical copies?
- What is the movement path of this entity through stages?
- Which file came from which previous file, if known?
- What is the runtime state of this file’s stage?
- Can I open the file/folder?
- Can I jump to the Entity Detail page?

---

## 5. Backend requirements

### 5.1. Keep read-only semantics

Workspace Explorer backend read commands must not:

- scan filesystem automatically;
- call n8n;
- claim tasks;
- reconcile stuck tasks;
- modify `entity_stage_states`;
- modify `stage_runs`;
- modify `entity_files`;
- write files;
- update `pipeline.yaml`.

It is acceptable to read selected filesystem metadata for display, as long as it does not mutate DB state.

Manual `Scan workspace` button may continue using existing scanner command. That is a separate explicit operator action.

---

### 5.2. Existing command review

Inspect existing command:

```text
get_workspace_explorer(path)
```

and existing structs around:

```text
WorkspaceExplorerResult
WorkspaceStageGroup
WorkspaceFileRecord
InvalidDiscoveryRecord
```

Decide whether to:

1. extend existing `get_workspace_explorer`; or
2. add a new command such as `get_workspace_tree`.

Preferred: evolve existing command if it does not become confusing. Add new DTO names if the old shape is too limited.

Keep frontend and backend naming clear.

---

### 5.3. Required backend read model

The explorer read model should include at minimum:

```rust
WorkspaceExplorerResult {
  generated_at: String,
  workdir_path: String,
  last_scan_at: Option<String>,
  stages: Vec<WorkspaceStageTree>,
  entity_trails: Vec<WorkspaceEntityTrail>,      // may be limited / optional
  totals: WorkspaceExplorerTotals,
  errors: Vec<CommandErrorInfo>,
}
```

Names can differ, but the concepts must exist.

#### 5.3.1. Stage tree node

For each stage known from SQLite `stages`, include:

```rust
WorkspaceStageTree {
  stage_id: String,
  input_folder: String,
  output_folder: Option<String>,
  workflow_url: Option<String>,
  next_stage: Option<String>,
  is_active: bool,
  archived_at: Option<String>,
  folder_path: String,
  folder_exists: bool,
  files: Vec<WorkspaceFileNode>,
  invalid_files: Vec<InvalidDiscoveryRecord>,
  counters: WorkspaceStageTreeCounters,
}
```

`output_folder` may be `None` for terminal stages.

#### 5.3.2. File node

For each registered `entity_files` row relevant to a stage, include:

```rust
WorkspaceFileNode {
  entity_file_id: i64,
  entity_id: String,
  stage_id: String,
  file_name: String,
  file_path: String,
  file_exists: bool,
  missing_since: Option<String>,
  is_managed_copy: bool,
  copy_source_file_id: Option<i64>,
  copy_source_entity_id: Option<String>,
  copy_source_stage_id: Option<String>,
  runtime_status: Option<String>,
  file_status: String,
  validation_status: EntityValidationStatus,
  validation_errors: Vec<ConfigValidationIssue>,
  current_stage: Option<String>,
  next_stage: Option<String>,
  checksum: String,
  file_size: u64,
  file_mtime: String,
  updated_at: String,
  can_open_file: bool,
  can_open_folder: bool,
}
```

Runtime status should come from `entity_stage_states`, not from JSON `status`, because SQLite is runtime source of truth.

#### 5.3.3. Stage counters

For every stage, include counts:

```rust
WorkspaceStageTreeCounters {
  registered_files: u64,
  present_files: u64,
  missing_files: u64,
  invalid_files: u64,
  managed_copies: u64,
  pending: u64,
  queued: u64,
  in_progress: u64,
  retry_wait: u64,
  done: u64,
  failed: u64,
  blocked: u64,
  skipped: u64,
}
```

These counters should be derived from SQLite read model and last scan invalid records.

#### 5.3.4. Entity trail

Implement either:

- global limited `entity_trails` for visible files; or
- a command/field for selected entity trail; or
- include trail data in each file node enough for frontend to construct it.

Preferred backend DTO:

```rust
WorkspaceEntityTrail {
  entity_id: String,
  file_count: u64,
  stages: Vec<WorkspaceEntityTrailNode>,
  edges: Vec<WorkspaceEntityTrailEdge>,
}
```

Trail node:

```rust
WorkspaceEntityTrailNode {
  entity_file_id: i64,
  stage_id: String,
  file_name: String,
  file_path: String,
  file_exists: bool,
  runtime_status: Option<String>,
  is_managed_copy: bool,
}
```

Trail edge:

```rust
WorkspaceEntityTrailEdge {
  from_entity_file_id: i64,
  to_entity_file_id: i64,
  relation: String, // e.g. "managed_copy", "n8n_response_copy", "same_entity_next_stage"
}
```

Use existing `copy_source_file_id`, `created_child_path`, `is_managed_copy`, and same `entity_id` ordering by stage config where possible. If exact edge cannot be inferred, show conservative relation such as `same_entity_stage_sequence`.

Do not invent false certainty. If a relation is inferred, mark it as inferred in DTO or relation string.

---

### 5.4. Invalid and unregistered files

The current scanner records invalid files in `app_events` for the last scan. Stage 8 should surface these in the tree.

Minimum:

- show invalid files from last scan grouped under their stage;
- include code/message/path/file_name/created_at.

If easy and safe, also show JSON files currently present in stage folders but not registered in DB. However, this must be read-only and must not replace `Scan workspace`.

If implementing unregistered disk file display:

- scan only configured active/inactive stage input folders, non-recursively;
- do not parse full JSON unless necessary;
- never mutate DB;
- label them clearly as `unregistered_on_disk`;
- explain that operator should run `Scan workspace`.

If this is too risky for Stage 8, skip unregistered disk display and document it as deferred. Do not overbuild.

---

### 5.5. Inactive stages

Inactive stages must remain visible if they exist in SQLite.

Workspace Explorer should show:

- active stage folders;
- inactive/archived stages;
- historical files/states/runs connected to inactive stages;
- warning/label that inactive stages are not scanned for new files.

Do not delete inactive stage history.

---

### 5.6. Path safety

All open file/folder actions must continue using backend-managed commands. Do not expose arbitrary path open from frontend.

If Stage 8 needs new open behavior, reuse:

```text
open_entity_file(path, entity_file_id)
open_entity_folder(path, entity_file_id)
```

Do not add `open_path(raw_path)`.

---

## 6. Frontend requirements

### 6.1. Workspace Explorer page

Update `WorkspaceExplorerPage` into a real tree/connected explorer.

Recommended layout:

```text
Header
  - title
  - Refresh
  - Scan workspace
  - Last scan timestamp
  - totals

Filters/search
  - search entity id / file name / path
  - stage filter
  - status filter
  - validation filter
  - checkboxes/toggles: show missing, show invalid, show inactive stages, show managed copies

Main area
  - left/primary: stage tree
  - right/secondary: selected file/entity details or trail panel
```

If layout complexity is too high, implement as stacked panels but keep the logical grouping clear.

---

### 6.2. Tree behavior

The UI should display:

- workdir root;
- stage groups;
- per-stage counters;
- file nodes under stage;
- invalid files under stage;
- missing files clearly marked;
- managed copies clearly marked;
- inactive stage clearly marked.

Use accessible HTML where possible:

- `<details>` / `<summary>` is acceptable;
- nested lists are acceptable;
- tables inside stage panels are acceptable.

Do not add heavy tree libraries unless necessary.

---

### 6.3. File node actions

For registered file nodes, provide:

- `Open file`;
- `Open folder`;
- `Go to Entity`;
- optionally `Select / Show Trail`.

`Go to Entity` should navigate to Entity Detail. Prefer including selected file id in query string:

```text
/entities/:entityId?file_id=:entityFileId
```

If current Entity Detail does not read `file_id`, update it to do so.

Do not run tasks or edit JSON from Workspace Explorer.

---

### 6.4. Entity trail panel

When an operator selects a file/entity, show a trail/lineage panel with:

- entity id;
- all file instances for that entity;
- stage id per file;
- file path;
- present/missing;
- runtime status;
- managed copy flag;
- source/target copy relationship if known;
- created child path if known;
- links/buttons to open file/folder or go to entity detail.

The trail can be textual/table-based for Stage 8. No graph library required.

---

### 6.5. Integration with Entity Detail

Update Entity Detail to support selected file from URL query param if not already supported:

```text
/entities/:entityId?file_id=123
```

Expected behavior:

- Entity Detail loads entity.
- If `file_id` belongs to the entity, it becomes selected file.
- If not, fallback to latest file and show normal detail.

This is important so Workspace Explorer can deep-link into the exact artifact.

---

### 6.6. Empty/loading/error states

Handle:

- no workdir selected;
- invalid config;
- empty workdir;
- no stages;
- no files;
- stage folder missing;
- files missing on disk;
- invalid files from last scan;
- backend errors.

No white screen.

---

## 7. Backend tests

Add Rust tests for the read model.

### 7.1. Required tests

1. Fresh workdir with stages returns stage tree and zero counters.
2. Registered present JSON appears under the correct stage with entity id and file id.
3. Missing registered file appears as `file_exists = false` and is counted as missing.
4. Managed copy appears with `is_managed_copy = true` and source file relationship if available.
5. Same entity in multiple stages produces an entity trail with multiple nodes.
6. Invalid last-scan file appears under stage invalid files.
7. Inactive stage remains visible with historical data.
8. Terminal stage with no `output_folder` does not break read model.
9. Read model does not mutate SQLite state.
10. Open file/folder commands still reject unknown file id and resolve registered file paths safely.

### 7.2. Optional tests

If unregistered disk files are implemented:

- unregistered disk JSON appears as read-only unregistered node;
- unregistered display does not create DB rows;
- non-JSON files are ignored or shown only if explicitly designed.

---

## 8. Frontend verification

At minimum:

```powershell
npm.cmd run build
```

must pass.

Only add frontend tests if a test pattern already exists or if it is low-cost. Do not spend large effort on UI test framework setup in Stage 8.

---

## 9. Documentation requirements

Create or update:

```text
docs/codex_stage8_progress.md
docs/codex_stage8_instruction_checklist.md
docs/codex_stage8_delivery_report.md
```

Update `README.md` with a concise section describing Workspace Explorer:

- read-only;
- stage tree;
- registered files;
- missing/invalid files;
- managed copies;
- entity trail;
- quick open file/folder;
- deep-link to Entity Detail;
- scan remains manual.

Documentation must not claim manual UI walkthrough unless actually done.

---

## 10. Required verification commands

Run and report exact results:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
```

```powershell
cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'
```

```powershell
npm.cmd run build
```

If command syntax differs in your environment, document exactly what was run.

Do not call real n8n endpoints.

Do not perform mouse-driven UI walkthrough unless explicitly done; if not done, say so.

---

## 11. Acceptance criteria

Stage 8 can be accepted only if:

1. Workspace Explorer page shows stage folders as a tree/grouped explorer.
2. Registered files are linked to `entity_id`, `entity_file_id`, `stage_id`.
3. Runtime status is shown from SQLite `entity_stage_states`, not JSON status.
4. Missing files are visible and clearly marked.
5. Invalid files from last scan are visible and clearly marked.
6. Managed copies are visible and clearly marked.
7. Entity artifact trail/copies across stages are visible.
8. Operator can jump from file node to Entity Detail.
9. Entity Detail can select file from URL query param or equivalent deep link.
10. Operator can open registered file/folder via backend-managed commands.
11. Explorer read path does not mutate DB, files, YAML, or runtime state.
12. Scan remains manual and explicit.
13. Inactive stages remain visible with historical data.
14. Terminal stages do not break the explorer.
15. Backend tests cover tree, file links, missing, invalid, managed copy, trail, inactive/terminal stages.
16. `cargo fmt` passes.
17. Rust tests pass.
18. `npm.cmd run build` passes.
19. Docs are updated honestly.
20. No unrelated Stage 9 polish work is mixed in.

---

## 12. Expected final response from Codex

When finished, respond with:

```md
# Stage 8 Delivery Summary

## Implemented
...

## Backend read model
...

## Workspace Explorer UI
...

## Entity trail / artifact connectivity
...

## Deep linking to Entity Detail
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

Do not overstate. If something is partial, say it explicitly.

---

## 13. Final instruction

Build Stage 8 as an operator-friendly read-only view of the physical workdir and logical artifact connectivity.

Do not mutate runtime state from explorer reads. Do not implement a background watcher. Do not turn this into a file manager. The goal is to help the operator understand where files are, how they are connected to entities/stages, and how artifacts moved through the pipeline.
