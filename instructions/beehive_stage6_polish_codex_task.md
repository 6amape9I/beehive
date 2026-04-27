# beehive — Stage 6 Polish Codex Task

## Короткая стабилизационная задача после Stage 6

Ты работаешь в проекте **beehive**.

Этот документ является единственным источником истины для небольшого polish-этапа после Stage 6. Цель — не добавлять новый продуктовый функционал, а закрыть два технических риска, обнаруженных при архитектурном ревью Stage 6:

1. Запретить или явно ограничить редактирование business JSON для файлов, чей stage сейчас находится в активном или завершённом runtime-состоянии.
2. Начать аккуратную декомпозицию слишком большого `src-tauri/src/database/mod.rs`, не ломая существующий API и тесты.

Stage 7 не начинать в рамках этой задачи.

---

## 0. Контекст

К текущему моменту проект уже имеет:

- Stage 1–5 foundation.
- Stage 5.5 runtime stabilization.
- Stage 6 entities table и entity detail.
- Backend commands для `retry/reset/skip/run`, `open file/folder`, `save_entity_file_business_json`.
- JSON editor, который редактирует только `payload` и `meta`.
- SQLite как runtime source of truth.
- Source JSON как business-data artifact.

Ревью Stage 6 приняло работу, но выявило два follow-up пункта:

- `save_entity_file_business_json` проверяет file snapshot, но не проверяет runtime status связанного stage перед сохранением.
- `database/mod.rs` разросся и стал содержать слишком много разных зон ответственности.

---

# 1. Scope

## 1.1. Входит в задачу

Нужно сделать:

1. Backend enforcement для JSON edit policy.
2. UI-disable / UX message для JSON editor, основанные на backend-provided policy.
3. App event для отказа в JSON edit из-за runtime status.
4. Небольшую безопасную декомпозицию database layer.
5. Тесты на новую policy.
6. Обновление README/docs.

## 1.2. Не входит в задачу

Не реализовывать:

- Stage 7 editor.
- YAML save.
- Stage CRUD.
- background scheduler.
- полноценный audit explorer.
- новый design system.
- массовый refactor всего backend.
- schema migration без необходимости.
- полноценную role/permission model.

---

# 2. JSON editing policy

## 2.1. Основное правило

Редактирование `payload`/`meta` выбранного `entity_file` разрешено только если связанный runtime state не является активным или завершённым.

Связанный runtime state определяется по:

```text
entity_file.entity_id + entity_file.stage_id
```

Использовать `entity_stage_states` для этого stage.

## 2.2. Разрешённые статусы для save

Разрешить редактирование business JSON только для stage state:

```text
pending
retry_wait
failed
blocked
skipped
```

При этом сохраняются уже существующие проверки:

- файл существует;
- файл stable;
- checksum/size/mtime совпадают с DB snapshot;
- `id` в JSON не изменился;
- `payload` не `null`;
- `meta` — JSON object.

## 2.3. Запрещённые статусы для save

Запретить редактирование business JSON для:

```text
queued
in_progress
done
```

Причины:

- `queued` и `in_progress` — активное runtime-состояние, редактирование может запутать operator mental model.
- `done` — завершённый audit state; редактирование source artifact после завершения может нарушить смысл истории обработки. Если нужно изменить данные после `done`, operator должен работать с next-stage copy или выполнить явное reset/manual workflow в будущем.

## 2.4. Если stage state отсутствует

Если для `entity_file.entity_id + entity_file.stage_id` нет `entity_stage_state`, save должен быть запрещён с понятной ошибкой:

```text
No runtime stage state exists for this file. Run Scan workspace before editing.
```

## 2.5. Backend must enforce

Запрет должен быть на backend-уровне, не только в UI.

`save_entity_file_business_json` должен:

1. Загрузить `EntityFileRecord`.
2. Найти связанный `entity_stage_state`.
3. Проверить edit policy.
4. Если запрещено:
   - не писать файл;
   - не менять DB;
   - не создавать fake successful event;
   - вернуть structured error message;
   - записать `app_events` warning с code `entity_file_json_edit_rejected`.
5. Если разрешено:
   - выполнить текущую stable snapshot проверку;
   - atomically save JSON;
   - update DB snapshot;
   - написать `entity_file_json_saved`.

---

# 3. Backend DTO / allowed actions

## 3.1. Добавить edit policy в Entity Detail DTO

Добавить в backend DTO и TypeScript types структуру, например:

```rust
EntityFileAllowedActions {
  entity_file_id: i64,
  can_edit_business_json: bool,
  can_open_file: bool,
  can_open_folder: bool,
  reasons: Vec<String>,
}
```

И добавить в `EntityDetailPayload`:

```rust
file_allowed_actions: Vec<EntityFileAllowedActions>
```

Названия могут отличаться, но frontend должен получать policy от backend.

Минимально нужно уметь объяснить:

- почему файл нельзя редактировать;
- можно ли открыть файл;
- можно ли открыть папку.

## 3.2. UI behavior

`EntityJsonPanel` должен:

- disable кнопку `Edit payload/meta`, если backend говорит `can_edit_business_json = false`;
- показать короткую причину;
- не пытаться save, если действие запрещено;
- всё равно позволять read-only JSON preview.

`EntityFileInstances` может показывать edit capability рядом с файлом, если это удобно, но не обязательно.

---

# 4. Database layer decomposition

## 4.1. Цель

Снизить размер и смешение ответственности в `src-tauri/src/database/mod.rs` без большого rewrite.

## 4.2. Минимально требуемое

Вынести хотя бы Stage 6-oriented код в один или несколько подмодулей:

Возможный вариант:

```text
src-tauri/src/database/
  mod.rs
  entity_table.rs
  entity_detail.rs
  entity_actions.rs
```

Или:

```text
src-tauri/src/database/entities.rs
```

Допустим один файл, если он реально разгружает `mod.rs`.

## 4.3. Что можно вынести

Предпочтительно вынести:

- `list_entity_table_page`
- entity table helper functions
- `get_entity_detail_with_selection`
- timeline/allowed-actions builders
- manual actions:
  - `reset_entity_stage_to_pending`
  - `skip_entity_stage`
  - `record_manual_retry_event`
- JSON edit policy helper

## 4.4. Что не трогать без необходимости

Не делать большой refactor:

- schema creation/migration;
- runtime claim;
- executor-facing DB functions;
- stage run insert/finish core;
- app events core;
- discovery persistence.

## 4.5. Public API compatibility

Существующие callers должны продолжать использовать тот же `crate::database::...` API, если это проще.

Можно оставить re-export/wrapper functions в `database/mod.rs`.

---

# 5. Tests

Добавить Rust tests.

## 5.1. JSON edit policy tests

Покрыть:

1. `pending` file can save business JSON.
2. `retry_wait` file can save business JSON.
3. `failed` file can save business JSON.
4. `blocked` file can save business JSON.
5. `skipped` file can save business JSON.
6. `queued` file cannot save business JSON.
7. `in_progress` file cannot save business JSON.
8. `done` file cannot save business JSON.
9. Missing stage state cannot save business JSON.
10. Rejected edit does not mutate file bytes.
11. Rejected edit does not update DB checksum/mtime.
12. Rejected edit records `entity_file_json_edit_rejected`.

## 5.2. Entity Detail policy tests

Проверить, что `get_entity_detail_with_selection` возвращает allowed action / reason для selected file или file actions list.

## 5.3. Regression tests

Убедиться, что не сломаны:

- entity table query;
- entity detail loading;
- manual reset/skip;
- run entity stage;
- JSON save allowed path;
- frontend build.

---

# 6. Frontend requirements

## 6.1. Types

Обновить `src/types/domain.ts` под новый DTO.

## 6.2. UI

Обновить:

- `EntityDetailPage`
- `EntityJsonPanel`
- при необходимости `EntityFileInstances`

Требования:

- read-only preview работает всегда, если JSON доступен;
- edit button disabled with reason;
- save невозможен при forbidden policy;
- если backend всё равно вернул error, отобразить его через existing error panel.

Не добавлять новых тяжёлых frontend dependencies.

---

# 7. Documentation

Создать/обновить:

```text
docs/codex_stage6_polish_progress.md
docs/codex_stage6_polish_instruction_checklist.md
docs/codex_stage6_polish_delivery_report.md
```

README обновить коротко:

- JSON editor edits only business `payload`/`meta`.
- JSON editor is disabled for `queued`, `in_progress`, `done`.
- SQLite remains runtime source of truth.
- Completed artifacts should not be edited silently.

---

# 8. Verification commands

Запустить и указать точный результат:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
```

```powershell
cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'
```

```powershell
npm.cmd run build
```

Если команда отличается в окружении, указать фактическую команду.

---

# 9. Acceptance criteria

Polish принимается, если:

1. Backend запрещает JSON save для `queued`, `in_progress`, `done`.
2. Backend разрешает JSON save для `pending`, `retry_wait`, `failed`, `blocked`, `skipped`.
3. Missing stage state запрещает save.
4. Rejected save не меняет файл.
5. Rejected save не меняет DB snapshot.
6. Rejected save логирует `entity_file_json_edit_rejected`.
7. Entity Detail DTO содержит edit policy/reasons.
8. UI disabled edit button and shows reason.
9. `database/mod.rs` разгружен через хотя бы один новый database submodule.
10. Existing Stage 6 functionality не регрессирует.
11. Rust tests добавлены и проходят.
12. `npm.cmd run build` проходит.
13. Docs updated honestly.

---

# 10. Expected Codex final response

```md
# Stage 6 Polish Delivery Summary

## Implemented
...

## JSON edit policy
...

## Database decomposition
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

# Final instruction

Сделай маленькую, безопасную стабилизацию. Не начинай Stage 7. Не меняй schema без необходимости. Главное — backend-enforced edit policy и первый шаг к более поддерживаемому database layer.
