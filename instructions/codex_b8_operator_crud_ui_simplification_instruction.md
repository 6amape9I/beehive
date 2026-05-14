# B8. Operator CRUD and UI Simplification

## Главная цель

Превратить текущий web MVP из инженерной консоли в нормальное операторское приложение.

После B8 пользователь без командной строки и без YAML должен уметь:

1. Создать workspace.
2. Выбрать workspace.
3. Изменить workspace.
4. Удалить/архивировать workspace.
5. Создать stage из stage_id + production n8n webhook URL.
6. Изменить stage.
7. Удалить/архивировать stage.
8. Соединить stages в pipeline.
9. Запустить S3 reconcile.
10. Выбрать конкретные source-файлы.
11. Запустить Run selected pipeline waves.

Не надо делать новый scheduler, RBAC, Postgres, n8n REST editor или production-run на 22 000 файлов. Это этап про CRUD и нормальный UI.

## Что сейчас не так

Интерфейс перегружен. По умолчанию оператор видит слишком много статистики, технических карточек, внутренних id, checksum, диагностических панелей и сообщений в стиле “всё работает”. Это должно уйти в Diagnostics / Advanced.

Главный экран workspace должен отвечать на простые вопросы:

```text
Где я?
Какие stages есть?
Сколько pending / failed / done?
Что можно безопасно запустить?
Что сломалось?
Что делать дальше?
```

Всё остальное — вторично.

## Workspace CRUD

Добавить backend CRUD для workspace registry.

Новые API:

```text
GET    /api/workspaces?include_archived=true|false
POST   /api/workspaces
PATCH  /api/workspaces/{workspace_id}
DELETE /api/workspaces/{workspace_id}
POST   /api/workspaces/{workspace_id}/restore
```

### Create workspace

Пользователь вводит только:

```text
name
bucket
workspace_prefix
region
endpoint
optional id
```

Пользователь НЕ вводит:

```text
workdir_path
pipeline_path
database_path
```

Backend сам генерирует server-side пути:

```text
workdir_path  = {BEEHIVE_WORKSPACES_ROOT}/{workspace_id}
pipeline_path = {workdir_path}/pipeline.yaml
database_path = {workdir_path}/app.db
```

Добавить env:

```text
BEEHIVE_WORKSPACES_ROOT
```

Default:

```text
/tmp/beehive-web-workspaces
```

При создании workspace backend должен:

```text
создать директорию workspace;
создать пустой pipeline.yaml с S3 storage;
создать/инициализировать app.db;
атомарно обновить workspaces.yaml;
вернуть workspace descriptor;
не трогать S3 objects;
не хранить S3 secrets.
```

### Workspace registry schema

Расширить workspaces.yaml:

```yaml
workspaces:
  - id: smoke
    name: Smoke
    provider: s3
    bucket: steos-s3-data
    workspace_prefix: beehive-smoke/test_workflow
    region: ru-1
    endpoint: https://s3.ru-1.storage.selcloud.ru
    workdir_path: /tmp/beehive-web-workspaces/smoke
    pipeline_path: /tmp/beehive-web-workspaces/smoke/pipeline.yaml
    database_path: /tmp/beehive-web-workspaces/smoke/app.db
    is_archived: false
    created_at: "..."
    updated_at: "..."
    archived_at: null
```

Старые записи без новых полей должны продолжать читаться.

### Edit workspace

Разрешить менять:

```text
name
endpoint
region
```

bucket и workspace_prefix менять только если workspace пустой: нет stages, нет entity_files, нет stage_runs. Если история уже есть — отклонить с понятным сообщением:

```text
Нельзя изменить bucket/prefix: в workspace уже есть зарегистрированные artifacts или история запусков.
```

### Delete workspace

S3 objects не удалять никогда.

Если workspace пустой — можно hard delete из registry.

Если есть история — делать archive:

```text
is_archived = true
archived_at = now
```

По умолчанию archived workspaces скрывать. Добавить toggle Show archived.

## Stage CRUD

Сейчас есть create и link. Нужно сделать нормальный CRUD.

Новые API:

```text
POST   /api/workspaces/{workspace_id}/stages
PATCH  /api/workspaces/{workspace_id}/stages/{stage_id}
DELETE /api/workspaces/{workspace_id}/stages/{stage_id}
POST   /api/workspaces/{workspace_id}/stages/{stage_id}/restore
POST   /api/workspaces/{workspace_id}/stages/{stage_id}/next-stage
```

### Create stage

Оставить простым:

```text
stage_id
production n8n webhook URL
optional next_stage
max_attempts
retry_delay_sec
allow_empty_outputs
```

Пользователь НЕ вводит S3 route руками.

Backend сам генерирует:

```text
input_uri = s3://{bucket}/{workspace_prefix}/stages/{stage_id}

save_path_aliases:
  {workspace_prefix}/stages/{stage_id}
  /{workspace_prefix}/stages/{stage_id}
  s3://{bucket}/{workspace_prefix}/stages/{stage_id}
```

### Edit stage

Разрешить менять:

```text
workflow_url
max_attempts
retry_delay_sec
allow_empty_outputs
next_stage
```

Не давать обычному пользователю редактировать:

```text
input_uri
input_folder
output_folder
save_path_aliases
```

Это системные поля.

stage_id сделать read-only. Rename отложить или разрешить только если stage полностью пустой и без истории. Если есть история — запретить:

```text
Stage id используется в истории запусков. Создайте новый stage.
```

### Delete/archive stage

Если stage пустой и на него никто не ссылается — можно удалить из pipeline.yaml.

Если stage имеет историю — архивировать/деактивировать, но не уничтожать историю.

Если другой stage указывает на него через next_stage, удаление блокировать:

```text
Нельзя удалить stage semantic_rich: stage raw_entities ссылается на него как next_stage.
```

Не удалять S3 objects.

## UI simplification

### /workspaces

Сделать нормальную страницу управления workspace:

```text
Create workspace
Edit
Archive/Delete
Restore
Show archived
Select workspace
```

Карточка workspace должна показывать только:

```text
name
id
bucket
workspace_prefix
stages count
status active/archived
```

Не показывать server paths обычному пользователю.

### /workspaces/{id}/workspace

Сделать операторский экран, а не диагностическую свалку.

Сверху:

```text
Workspace name
S3 bucket / prefix
Stages count
Pending / Failed / Done
Primary actions:
  Reconcile S3
  Run selected pipeline waves
  Add stage
```

Технические счётчики, checksum, raw metadata, detailed reconciliation counters — в Diagnostics.

### /workspaces/{id}/stages

Сделать простой CRUD stages:

```text
Add stage
Edit stage
Connect stages
Archive/Delete stage
Restore stage
Copy save_path aliases
```

YAML preview и raw validation — только в Advanced.

### Workspace Explorer

Оставить видимым:

```text
checkbox
stage
entity/artifact id
status
S3 key
copy S3 URI
error indicator
producer_run_id / outputs только по раскрытию
```

Скрыть по умолчанию:

```text
checksum
etag
all internal ids
огромные summary cards
технические counters
file_exists если оно не нужно для действия
```

## Обязательные backend-функции

В `services/workspaces.rs` или новом `workspace_registry.rs`:

```text
create_workspace(...)
update_workspace(...)
archive_or_delete_workspace(...)
restore_workspace(...)
list_workspace_descriptors(include_archived)
```

В `services/pipeline.rs`:

```text
update_s3_stage_for_workspace(...)
archive_or_delete_stage_for_workspace(...)
restore_stage_for_workspace(...)
```

Все записи в `workspaces.yaml` и `pipeline.yaml` делать атомарно:

```text
write temp
backup old file
rename temp
```

## Обязательные проверки

Workspace:

```text
create workspace writes registry and initializes files;
create rejects duplicate id;
update rejects dangerous bucket/prefix change when history exists;
delete archives workspace with history;
restore workspace works;
old registry entries without created_at still load.
```

Stage:

```text
update changes workflow_url/retry settings;
delete blocks when another stage links to it;
delete archives stage with history;
hard delete works for empty unlinked stage;
restore stage works if no active duplicate exists.
```

HTTP:

```text
routes parse CRUD requests;
old B7 selected-run route still works.
```

Frontend:

```bash
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
```

Existing checks:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
rg "@tauri-apps/api/core|invoke\(" src -n
git diff --check
```

## Smoke script

Расширить `scripts/web_operator_smoke.mjs` или создать новый smoke, который работает с временным registry/workspace root и проверяет:

```text
POST workspace
PATCH workspace
POST stage
PATCH stage
POST next-stage
DELETE stage
DELETE workspace
```

Не использовать реальный production workspace для CRUD smoke.

## Документы после работы

Агент обязан создать:

```text
docs/beehive_s3_b8_crud_ui_plan.md
docs/beehive_s3_b8_crud_ui_feedback.md
docs/operator_crud_runbook.md
```

Feedback должен содержать:

```text
что сделано;
какие файлы изменены;
как работает workspace CRUD;
как работает stage CRUD;
что упрощено в UI;
что спрятано в Advanced/Diagnostics;
какие команды запускались;
результаты тестов;
результаты smoke;
риски;
что делать в B9.
```

Контрольная строка:

```text
ТЗ перечитано на этапах: after_plan, after_workspace_crud_design, after_stage_crud_design, after_backend_crud, after_ui_simplification, after_tests, after_smoke, before_feedback
```

## Acceptance criteria

B8 принимается только если:

1. Workspace можно создать из UI.
2. Workspace можно изменить из UI.
3. Workspace можно удалить/архивировать из UI.
4. Stage можно создать из UI.
5. Stage можно изменить из UI.
6. Stage можно удалить/архивировать из UI.
7. Stage можно соединить с другим stage из UI.
8. Пользователь не редактирует server paths и S3 route руками.
9. Default UI стал проще.
10. Диагностика не исчезла, но спрятана.
11. Старый selected pipeline run не сломан.

Главный принцип для агента: не показывать всё подряд. Показывать только то, что помогает оператору сделать следующий шаг.
