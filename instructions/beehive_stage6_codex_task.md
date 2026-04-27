# beehive — Stage 6 Codex Task

## Entities Table and Entity Detail

Ты работаешь в проекте **beehive**.

Этот документ является единственным источником истины для реализации **Stage 6**. Перед началом работы обязательно перечитай:

- `README.md`;
- `instructions/beehive_stage1_codex_task.md`;
- `instructions/beehive_stage2_codex_task.md`;
- `instructions/beehive_stage3_codex_task.md`;
- `instructions/beehive_stage4_codex_task.md`;
- `instructions/beehive_stage5_codex_task.md`;
- `instructions/beehive_stage5_5_codex_task.md`;
- follow-up task про orphan `stage_runs`, если он уже принят;
- delivery reports и checklists в `docs/`;
- текущий backend/frontend код.

Не полагайся на память. Не переписывай фундамент без необходимости.

---

# 1. Контекст текущего состояния

К началу Stage 6 проект должен иметь:

- Tauri + React + TypeScript desktop foundation;
- Rust backend через Tauri commands;
- SQLite runtime storage;
- YAML `pipeline.yaml`;
- workdir model;
- `entities`;
- `entity_files`;
- `entity_stage_states`;
- `stage_runs`;
- `app_events`;
- scanner/reconciliation;
- managed next-stage copy;
- n8n execution foundation;
- retry mechanics;
- formal state machine;
- atomic task claiming;
- file stability guard;
- terminal-stage handling;
- Stage 5 Dashboard read model.

Stage 6 строится поверх этого фундамента и не должен ломать Stage 1–5.5.

---

# 2. Главная цель Stage 6

Дать оператору удобные инструменты для детального анализа и ручного контроля JSON-сущностей.

К концу Stage 6 пользователь должен уметь:

1. Смотреть список сущностей в полноценной таблице.
2. Фильтровать, искать, сортировать и постранично просматривать сущности.
3. Открывать карточку сущности.
4. Видеть все физические file instances сущности по stage-папкам.
5. Видеть stage timeline/status history.
6. Видеть историю запусков `stage_runs`.
7. Читать JSON содержимое.
8. Безопасно редактировать бизнес-части JSON.
9. Выполнять ручные действия через backend:
   - Retry now;
   - Reset to pending;
   - Skip;
   - Open file;
   - Open folder.
10. Видеть ошибки и технический контекст без ручного чтения БД.

---

# 3. Что НЕ входит в Stage 6

Не реализовывать:

- Stage 7 полноценный Stage Editor / YAML CRUD;
- Stage 8 полноценный workspace tree redesign;
- React Flow graph editor;
- background daemon / scheduler;
- parallel worker pool;
- n8n REST API;
- credential manager;
- authentication UI;
- distributed multi-machine locking;
- complex branch routing;
- массовые операции над сотнями сущностей;
- advanced audit explorer за пределами карточки сущности;
- полное Git-подобное версионирование JSON;
- полноценный low-code editor;
- destructive file overwrite без backend guard.

Если для Stage 6 нужно добавить маленькую backend-команду или frontend-компонент, делай это точечно.

---

# 4. Product principles

1. SQLite остаётся source of truth для runtime state.
2. JSON-файлы остаются source of truth для бизнес-данных.
3. UI не меняет файлы или БД напрямую.
4. Все ручные действия проходят через backend commands.
5. State transitions проходят через state machine.
6. Source JSON не мутируется автоматически после n8n execution.
7. JSON editor не должен позволять пользователю незаметно сломать runtime state.
8. UI должен быть операторским: понятные статусы, явные ошибки, минимум скрытой магии.
9. При росте числа файлов таблица не должна загружать всё в память без необходимости.

---

# 5. Backend: Entities Table read model

## 5.1. Current issue

Текущая Entities page уже существует, но она недостаточна для Stage 6:

- таблица простая;
- фильтры ограничены;
- сортировка неполная;
- нет нормальной серверной пагинации;
- не хватает attempts / last_error / last HTTP / next retry;
- backend не должен заставлять frontend загружать все строки при больших объёмах.

## 5.2. Required command

Добавить или обновить backend command:

```rust
list_entities(...)
```

Можно сохранить имя существующей команды, если frontend совместим. Если проще и безопаснее, добавить `list_entities_v2`, но не оставляй два конкурирующих API без причины.

## 5.3. Query DTO

Добавить typed query DTO примерно такого вида:

```ts
type EntityListQuery = {
  search?: string | null;
  stage_id?: string | null;
  status?: string | null;
  validation_status?: "valid" | "warning" | "invalid" | null;

  sort_by?: "entity_id" | "current_stage" | "status" | "updated_at" | "last_seen_at" | "attempts" | "last_error" | null;
  sort_direction?: "asc" | "desc" | null;

  page?: number;
  page_size?: number;
};
```

Допустимо адаптировать имена под текущий style.

Defaults:

- `page = 1`;
- `page_size = 50`;
- maximum `page_size = 200`;
- default sort: `updated_at desc`.

## 5.4. Response DTO

Ответ должен содержать:

```ts
type EntityListResult = {
  entities: EntityTableRow[];
  total: number;
  page: number;
  page_size: number;
  available_stages: string[];
  available_statuses: string[];
  errors: CommandErrorInfo[];
};
```

`EntityTableRow` должен включать минимум:

```ts
type EntityTableRow = {
  entity_id: string;
  current_stage_id: string | null;
  current_status: string;
  latest_file_path: string | null;
  latest_file_id: number | null;
  file_count: number;

  attempts: number | null;
  max_attempts: number | null;
  last_error: string | null;
  last_http_status: number | null;
  next_retry_at: string | null;
  last_started_at: string | null;
  last_finished_at: string | null;

  validation_status: "valid" | "warning" | "invalid";
  updated_at: string;
  last_seen_at: string;
};
```

Если текущий `EntityRecord` можно расширить без путаницы — расширяй его. Если нет — создай отдельный DTO для table row.

## 5.5. Query performance

Backend должен выполнять фильтрацию, поиск, сортировку и пагинацию в SQLite, не в frontend.

Требования:

- использовать SQL `WHERE`, `ORDER BY`, `LIMIT`, `OFFSET`;
- не читать payload JSON для таблицы;
- не сканировать файловую систему;
- не вызывать n8n;
- не менять runtime state;
- добавить lightweight indexes только если реально нужны и не требуют тяжёлой миграции.

Если индексы добавляются через `CREATE INDEX IF NOT EXISTS`, schema version можно не поднимать, если это соответствует текущему стилю проекта. Если меняешь schema — добавь migration tests.

---

# 6. Frontend: Entities Table

## 6.1. UI library

Для Stage 6 рекомендуется использовать **TanStack Table** для таблицы.

Если добавляешь dependency:

```text
@tanstack/react-table
```

не добавляй тяжёлые UI frameworks.

Таблица может использовать TanStack только для state/rendering, но данные должны быть server-side paginated/sorted.

## 6.2. Required table functionality

Entities page должна поддерживать:

- search by entity id / file path;
- filter by stage;
- filter by status;
- filter by validation status;
- sort by:
  - entity id;
  - current stage;
  - status;
  - attempts;
  - updated at;
  - last seen;
- pagination;
- refresh;
- row click to entity detail;
- clear filters;
- loading state;
- empty state;
- error state.

## 6.3. Required visible columns

Минимум:

- Entity ID;
- Current stage;
- Status;
- Attempts;
- Last error;
- Last HTTP;
- Next retry;
- File count;
- Latest file path;
- Validation;
- Updated at;
- Last seen.

Last error должен быть truncated в таблице, но полный текст должен быть доступен в detail.

## 6.4. URL/query state

Желательно хранить фильтры/сортировку/страницу в URL query params, чтобы оператор мог вернуться к состоянию таблицы.

Если это слишком много, можно хранить в React state, но не усложняй.

---

# 7. Backend: Entity Detail read model

## 7.1. Current state

Текущий `get_entity` уже возвращает entity, files, stage states и JSON preview. Для Stage 6 нужно сделать карточку полноценной операторской страницей.

## 7.2. Required detail DTO

Карточка должна получать один согласованный payload:

```ts
type EntityDetailPayload = {
  entity: EntityRecord;
  files: EntityFileRecord[];
  stage_states: EntityStageStateRecord[];
  stage_runs: StageRunRecord[];
  timeline: EntityTimelineItem[];
  latest_json_preview: string;
  selected_file_json?: string | null;
  allowed_actions: EntityStageAllowedActions[];
};
```

Можно сохранить отдельный `list_stage_runs`, но page должна загружать всё согласованно и показывать ошибки понятно.

## 7.3. Timeline DTO

Добавить timeline model:

```ts
type EntityTimelineItem = {
  stage_id: string;
  status: string;
  attempts: number;
  max_attempts: number;
  file_path: string | null;
  file_exists: boolean;
  last_error: string | null;
  last_http_status: number | null;
  next_retry_at: string | null;
  last_started_at: string | null;
  last_finished_at: string | null;
  created_child_path: string | null;
  updated_at: string;
};
```

Timeline должен сортироваться по pipeline stage order, а не случайно по id. Если stage уже inactive/archived, всё равно показывать historical state.

## 7.4. Stage runs

На карточке сущности показывать историю runs:

- run id;
- stage;
- attempt;
- success/failure;
- HTTP status;
- error type;
- error message;
- started/finished;
- duration;
- expandable request JSON;
- expandable response JSON.

Не показывать огромные JSON snapshots полностью в таблице; использовать details/accordion/pre blocks.

---

# 8. Manual actions

Stage 6 должен добавить ручные действия, но строго через backend и state machine.

## 8.1. Allowed actions model

Backend должен отдавать для каждого stage state список allowed actions:

```ts
type EntityStageAllowedActions = {
  stage_id: string;
  can_retry_now: boolean;
  can_reset_to_pending: boolean;
  can_skip: boolean;
  can_run_this_stage: boolean;
  reasons: string[];
};
```

Frontend не должен угадывать правила только по status. UI должен опираться на backend allowed actions.

## 8.2. Retry now

Добавить backend command:

```rust
retry_entity_stage_now(path, entity_id, stage_id, operator_comment?)
```

Семантика:

- для `retry_wait`: запускает stage сейчас, даже если `next_retry_at` в будущем, используя existing safe manual/debug execution path;
- для `failed` / `blocked`: допускается как combined action только если backend сначала безопасно reset-ит state в `pending`, затем запускает один раз;
- для `pending`: может вести себя как `run_entity_stage`;
- для `done`, `queued`, `in_progress`, `skipped`: запрещено;
- всегда применяет atomic claim and source pre-flight checks;
- не обходит file stability guard;
- пишет `app_events` с операторским действием.

Если combined retry for `failed` / `blocked` кажется слишком рискованным, реализуй только `retry_wait` + `pending`, а для `failed` / `blocked` UI должен предлагать `Reset to pending`. Но это решение должно быть явно отражено в docs и allowed actions.

## 8.3. Reset to pending

Добавить backend command:

```rust
reset_entity_stage_to_pending(path, entity_id, stage_id, operator_comment?)
```

Семантика:

- allowed for `failed`, `blocked`, `skipped`, `retry_wait`;
- not allowed for `done`, `queued`, `in_progress`;
- for `pending` it may no-op;
- reset should:
  - set `status = pending`;
  - set `attempts = 0`;
  - clear `next_retry_at`;
  - clear `last_error`;
  - clear `last_http_status`;
  - clear `created_child_path` only if this is safe and intended;
  - keep `stage_runs` history unchanged;
  - write `app_events`.

State machine may need new manual transition support:

```text
blocked -> pending
retry_wait -> pending
failed -> pending
skipped -> pending
```

Only add these transitions for `ManualReset` reason. Add tests.

## 8.4. Skip

Добавить backend command:

```rust
skip_entity_stage(path, entity_id, stage_id, operator_comment?)
```

Conservative Stage 6 behavior:

- `skip` marks current stage state as `skipped`;
- it does **not** create next-stage copy;
- it does **not** advance entity to next stage;
- it does **not** call n8n;
- it writes `app_events`.

Allowed statuses:

- minimum: `pending`;
- optionally: `retry_wait`, `failed`, `blocked` if state machine and product semantics are clear.

Do not allow skip for:

- `done`;
- `queued`;
- `in_progress`.

If you extend allowed transitions beyond `pending -> skipped`, document and test them.

## 8.5. Run this stage

Existing `run_entity_stage` can stay, but UI should expose it only when allowed by backend action model.

Do not run active `queued` / `in_progress`.

## 8.6. Action feedback

Frontend must show:

- loading while action runs;
- success message;
- errors;
- refreshed entity detail after action;
- refreshed stage runs after action.

---

# 9. Open file / folder

Add backend commands for OS integration:

```rust
open_entity_file(path, entity_file_id)
open_entity_folder(path, entity_file_id)
```

or equivalent.

Requirements:

- resolve file from DB;
- verify path is within current workdir or a known registered file path;
- avoid path traversal;
- handle missing file gracefully;
- use cross-platform safe opening method;
- no hardcoded Windows-only command;
- return structured error if OS open fails.

Implementation options:

- use a small Rust crate such as `opener`;
- or a Tauri v2 plugin if already appropriate for the project.

Do not expose arbitrary path open without validation.

Tests may cover path resolution and error cases. OS-level opening can be smoke-tested lightly or documented if not feasible in automated tests.

---

# 10. JSON viewer/editor

## 10.1. Viewer

Entity Detail must show JSON content of selected file instance.

Requirements:

- choose file instance from list;
- show formatted JSON;
- handle invalid/missing file gracefully;
- show checksum/mtime/size;
- show whether selected file is latest/current.

## 10.2. Editor scope

Stage 6 should implement safe JSON editing for business content.

Preferred Stage 6 scope:

- allow editing `payload`;
- optionally allow editing `meta`;
- do **not** allow editing `id`;
- do **not** allow editing runtime state through JSON editor;
- do **not** use JSON `status` to overwrite SQLite execution state.

If implementing full JSON editor, backend must enforce invariants:

- root must be JSON object;
- `id` must remain unchanged;
- `payload` must exist and be non-null;
- file must still match DB snapshot before save;
- source file must be stable before save;
- write must be atomic;
- DB snapshot must be refreshed after save;
- stage state execution status must not be overwritten by JSON `status` if SQLite has a runtime status such as `done`, `failed`, `retry_wait`, `in_progress`.

## 10.3. Backend command

Add command such as:

```rust
save_entity_file_json(path, entity_file_id, edited_json, operator_comment?)
```

or separate payload/meta save commands.

Required behavior:

1. Load file record from DB.
2. Verify file exists.
3. Verify file is inside workdir or registered canonical path.
4. Verify disk file still matches DB checksum/size/mtime before save.
5. Parse edited JSON.
6. Validate invariants.
7. Atomic write via tmp file + rename.
8. Recompute checksum/mtime/size.
9. Update `entity_files` snapshot.
10. Update logical entity summary if needed.
11. Write app event `entity_file_json_saved`.
12. Return updated file/entity detail.

If stale disk mismatch is detected:

- do not overwrite;
- return structured error telling user to refresh/scan.

## 10.4. UI editor

UI can be simple:

- read-only by default;
- `Edit payload/meta` button;
- textarea or JSON editor component;
- validate JSON before save;
- show parse errors;
- `Save` / `Cancel`;
- show backend errors.

Do not add heavy editor dependency unless justified. A `<textarea>` with formatting is acceptable for Stage 6.

---

# 11. Entity Detail UI requirements

The page should have clear sections:

1. Entity header:
   - entity id;
   - current stage;
   - current status;
   - validation;
   - latest file path;
   - updated/last seen.

2. Quick actions:
   - Retry now;
   - Reset to pending;
   - Skip;
   - Run this stage where appropriate;
   - Open file;
   - Open folder;
   - Refresh.

3. Timeline:
   - stage by stage;
   - status;
   - attempts;
   - last error;
   - timestamps.

4. File instances:
   - path;
   - stage;
   - status;
   - file exists/missing;
   - checksum;
   - size;
   - mtime;
   - managed copy marker;
   - select file for JSON viewer.

5. JSON viewer/editor:
   - selected file;
   - formatted content;
   - edit/save if allowed.

6. Stage run history:
   - compact table;
   - expandable request/response/error context.

7. Diagnostics:
   - validation issues;
   - app event references if easy;
   - missing/stale file warnings.

---

# 12. Backend safety and state machine requirements

Manual action commands must not directly SQL-update status without state machine validation.

Add state machine reasons as needed, for example:

- `ManualRetryNow`
- `ManualReset`
- `ManualSkip`

Add transitions only with tests.

All manual actions must write `app_events` with:

- action name;
- entity_id;
- stage_id;
- operator_comment if provided;
- previous status;
- new status;
- timestamp.

If user action is rejected, return structured error. Do not panic.

---

# 13. Testing requirements

Add automated tests.

## 13.1. Entities table tests

- search by entity id;
- search by latest file path;
- filter by stage;
- filter by status;
- filter by validation status;
- sort by updated_at desc;
- sort by attempts;
- pagination returns correct total and page rows;
- table query does not load payload JSON.

## 13.2. Entity detail tests

- detail returns entity, files, stage states, runs;
- timeline follows stage order;
- inactive/historical stages still display;
- missing file appears clearly;
- latest JSON preview works;
- selected file JSON loads.

## 13.3. Manual actions tests

- retry now on `retry_wait` executes through safe claim and creates run;
- retry now on future `retry_wait` is allowed only for manual path;
- retry now rejected for `done`, `queued`, `in_progress`;
- reset `failed -> pending` resets attempts and clears retry/error fields;
- reset `blocked -> pending` works only if explicitly added to state machine;
- skip pending -> skipped;
- skip rejected for in_progress;
- manual actions write app_events;
- history in `stage_runs` is preserved after reset.

## 13.4. JSON editor tests

- save valid payload/meta updates file atomically and refreshes DB snapshot;
- stale disk mismatch rejects save;
- changing `id` is rejected;
- missing `payload` is rejected;
- invalid JSON is rejected;
- source runtime state in SQLite is not overwritten by edited JSON `status`;
- app event is written.

## 13.5. Open file/folder tests

- command rejects unknown file id;
- command rejects missing file with structured error;
- path resolution stays inside/registered workdir;
- OS open function can be abstracted/mocked if needed.

## 13.6. Regression tests

Existing tests for:

- workdir bootstrap;
- config validation;
- schema bootstrap;
- scanner/reconciliation;
- atomic claim;
- file stability;
- n8n success/failure/retry;
- terminal stage;
- dashboard read model;

must continue to pass.

---

# 14. Frontend build and quality

Requirements:

- TypeScript strictness should not degrade;
- avoid `any` unless unavoidable;
- keep components split and maintainable;
- no white screen on empty/fresh workdir;
- no white screen on missing entity;
- no white screen on invalid JSON preview;
- show loading and error states;
- `npm.cmd run build` must pass.

Suggested component split:

```text
src/pages/EntitiesPage.tsx
src/pages/EntityDetailPage.tsx
src/components/entities/EntityFilters.tsx
src/components/entities/EntitiesTable.tsx
src/components/entities/PaginationControls.tsx
src/components/entity-detail/EntityHeader.tsx
src/components/entity-detail/EntityTimeline.tsx
src/components/entity-detail/EntityFileInstances.tsx
src/components/entity-detail/EntityJsonPanel.tsx
src/components/entity-detail/StageRunsPanel.tsx
src/components/entity-detail/ManualActionsPanel.tsx
```

Names may differ, but avoid one huge unmaintainable component.

---

# 15. Documentation requirements

Create/update:

```text
docs/codex_stage6_progress.md
docs/codex_stage6_instruction_checklist.md
docs/codex_stage6_delivery_report.md
```

Update README if operator-facing behavior changes.

Delivery report must include:

- table/read model implementation;
- entity detail implementation;
- manual actions semantics;
- JSON editor scope and safety rules;
- open file/folder behavior;
- backend commands added;
- frontend components added;
- tests added;
- verification commands;
- known limitations;
- whether Stage 6 is ready for review.

Checklist must be honest. Do not mark an item done unless code and tests exist.

---

# 16. Verification commands

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

If the command syntax differs in the environment, use the project’s existing verified command and document exactly what was run.

No real n8n endpoint should be called by automated tests.

No full manual UI walkthrough is required, but a minimal UI smoke note is acceptable if actually performed.

---

# 17. Acceptance criteria

Stage 6 is accepted only if all are true:

1. Entities table uses real backend data.
2. Filtering works by search, stage, status, validation.
3. Sorting works for key columns.
4. Pagination works and is backend-driven.
5. Table shows attempts and last error.
6. Entity detail shows entity header, files, stage states, timeline, and runs.
7. JSON viewer works for selected file instance.
8. JSON editing is safe, backend-mediated, atomic, and does not corrupt runtime state.
9. Manual actions exist through backend commands.
10. Manual actions use state machine validation.
11. Retry/reset/skip semantics are documented and tested.
12. Open file/folder actions are backend-mediated and safe.
13. Stage run history is visible and useful.
14. Existing Stage 1–5.5 behavior does not regress.
15. Rust tests cover backend read models and manual actions.
16. Rust tests cover JSON save safety.
17. Frontend build passes.
18. Docs are updated honestly.
19. Known limitations are stated clearly.

---

# 18. Known acceptable limitations after Stage 6

It is acceptable if Stage 6 still does not have:

- full Stage Editor CRUD;
- React Flow graph editor;
- Workspace Explorer redesign;
- background daemon;
- mass/bulk actions;
- advanced JSON diff/version history;
- rich Monaco editor;
- authentication/roles;
- multi-user synchronization.

But entity-level operator work must be useful and safe.

---

# 19. Expected final response from Codex

When finished, respond with:

```md
# Stage 6 Delivery Summary

## Implemented
...

## Backend/read models
...

## Manual actions
...

## JSON viewer/editor
...

## Frontend/UI
...

## Files changed
...

## Tests
...

## Verification commands
...

## Known limitations
...

## Stage 6 acceptance status
...
```

Do not overstate. If a part is partial, say so explicitly.
