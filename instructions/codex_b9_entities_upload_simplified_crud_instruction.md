# B9. Entities CRUD, Folder Upload, Simplified Workspace Creation, and save_path-only Stages

## 0. Роль агента

Ты Codex-агент, работающий над Beehive.

Твоя задача — исправить продуктовый UX после B8. B8 добавил Workspace CRUD и Stage CRUD, но интерфейс всё ещё слишком технический, а оператору всё ещё не хватает главного: загрузки сущностей и простого управления ими.

B9 — это не этап про новые n8n-пайплайны, не этап про high-load, не этап про Postgres/RBAC. Это этап про то, чтобы оператор мог нормально работать в web-приложении.

## 1. Главная цель B9

После B9 пользователь должен уметь без YAML, терминала и ручного S3-конфига:

1. Создать workspace, указав только имя.
2. Открыть workspace.
3. Создать stage из `stage_id` и production n8n webhook URL.
4. Загрузить локальную папку с JSON-файлами сущностей.
5. Увидеть загруженные entities/artifacts в workspace.
6. Архивировать/восстановить entity.
7. Выбрать entities.
8. Запустить `Run selected pipeline waves`.
9. Увидеть outputs и статусы.

Обычный пользователь не должен видеть или вводить:

```text
workspace id
bucket
region
endpoint
workspace_prefix
server workdir path
pipeline path
database path
next_stage
Connect Stages
manual S3 input_uri
manual save_path_aliases editing
2. Оценка B8, которую нужно учитывать

B8 не надо переписывать с нуля. Он уже добавил Workspace CRUD, Stage CRUD, HTTP CRUD smoke и UI simplification. Это полезная база.

Но B8 оставил три проблемы:

Workspace creation всё ещё слишком технический.
Stage Editor всё ещё тащит next_stage и Connect Stages.
Entity CRUD и загрузка локальных JSON-файлов отсутствуют.

B9 должен исправить именно это.

3. Workspace creation: только name
3.1 Новое продуктовое правило

Пользователь при создании workspace вводит только:

name

Backend сам задаёт:

bucket = steos-s3-data
region = ru-1
endpoint = https://s3.ru-1.storage.selcloud.ru
workspace_prefix = name
workspace_id = generated
3.2 Важное правило по workspace_prefix

workspace_prefix должен быть равен имени workspace.

Можно делать безопасный trim, можно запрещать опасные символы, но нельзя превращать prefix в отдельное поле, которое пользователь вводит руками.

Допустимо:

name = Медицинские сущности тест
workspace_prefix = Медицинские сущности тест

Запрещать:

пустое имя
/ или \\
..
control characters
строку только из пробелов
3.3 workspace_id

workspace_id генерируется backend’ом. Пользователь его не вводит.

Если из имени можно сделать safe ASCII slug — сделать slug. Если имя кириллическое и slug пустой, сгенерировать:

workspace-20260514-abcdef

ID можно показать после создания, но не просить у пользователя.

3.4 UI

Форма Create Workspace должна быть такой:

Workspace name: [________________]
[Create workspace]

Под формой можно показать мелким текстом:

S3: steos-s3-data / ru-1 / https://s3.ru-1.storage.selcloud.ru
Prefix will use the workspace name.

Не показывать обычному оператору bucket/region/endpoint/prefix/id.

4. Stage model: только save_path, next_stage deprecated
4.1 Стратегическое решение

Мы больше не используем next_stage как операторскую модель.

Routing делает n8n через save_path, а Beehive только валидирует, что save_path соответствует разрешённому stage route.

Это уже соответствует текущим n8n-пайплайнам: workflow возвращают business output с save_path, например stage 3 пишет /main_dir/processed/weight_entity, а stage 1.1 ветвит разные outputs через save_path.

4.2 Удалить из UI

Полностью убрать из обычного web UI:

Connect Stages
Next stage
source/target stage dropdowns
next_stage wording
stage linking panel
4.3 Backend behavior

Для новых S3 stages:

next_stage = null

Stage create/update normal HTTP/UI path не должен принимать next_stage.

Если поле next_stage остаётся в Rust structs из-за совместимости — окей, но UI/API оператора не должны его менять.

Endpoint:

POST /api/workspaces/{workspace_id}/stages/{stage_id}/next-stage

должен быть удалён из normal API или возвращать deprecated error:

{
  "errors": [
    {
      "code": "next_stage_deprecated",
      "message": "next_stage is deprecated. Route outputs through n8n save_path."
    }
  ]
}
4.4 isTerminal

Оставить только галочку:

Terminal stage

Она маппится на существующую runtime-логику:

Terminal stage = allow_empty_outputs

UI-текст:

Terminal stage means this workflow may finish successfully without output artifacts. Non-terminal stages must return at least one manifest output with save_path.

Не показывать пользователю термин allow_empty_outputs, только Terminal stage.

4.5 Stage create/edit form

Обычная форма stage должна содержать только:

Stage ID
Production n8n webhook URL
Max attempts
Retry delay
Terminal stage checkbox
Generated save_path aliases, read-only/copyable

Пользователь не редактирует:

input_uri
input_folder
output_folder
save_path_aliases
next_stage
5. Entity CRUD
5.1 Что считать entity

В B9 entity — это логическая сущность в workspace, представленная одной или несколькими S3 artifacts в entity_files.

CRUD должен быть достаточно простым:

list
detail
update operator note/display label
archive/delete
restore
upload/import JSON files
5.2 API

Добавить:

GET    /api/workspaces/{workspace_id}/entities
GET    /api/workspaces/{workspace_id}/entities/{entity_id}
PATCH  /api/workspaces/{workspace_id}/entities/{entity_id}
DELETE /api/workspaces/{workspace_id}/entities/{entity_id}
POST   /api/workspaces/{workspace_id}/entities/{entity_id}/restore
POST   /api/workspaces/{workspace_id}/entities/import-json-batch

List query params:

stage_id
status
search
include_archived
limit
offset
5.3 Archive, not S3 delete

Удаление entity не удаляет S3 objects.

DELETE должен архивировать entity:

is_archived = true
archived_at = now

Restore снимает archive.

Если в текущей SQLite schema нет таких полей, добавить additive migration:

entities.is_archived BOOLEAN DEFAULT 0
entities.archived_at TEXT NULL
entities.updated_at TEXT NULL
entities.operator_note TEXT NULL

Или эквивалентную metadata table, если это безопаснее.

5.4 Update entity

На B9 не делать сложный editor сущности.

Разрешить только:

operator_note
display_name / label, если удобно

Business JSON не редактировать в B9.

6. Upload entities from local folder
6.1 Product flow

В workspace должен быть заметный button:

Upload entities

Пользовательский сценарий:

1. Нажать Upload entities.
2. Выбрать локальную папку с JSON-файлами.
3. Выбрать target stage.
4. Приложение валидирует JSON.
5. Приложение загружает валидные JSON в S3.
6. Приложение регистрирует artifacts в workspace DB.
7. Пользователь видит summary.
6.2 Browser implementation

В browser mode backend не может сам открыть локальную папку пользователя. Frontend должен использовать browser File API:

<input type="file" webkitdirectory multiple accept="application/json,.json" />

Frontend читает файлы, парсит JSON, отправляет на backend батчами.

Не отправлять 22 000 файлов одним request.

Batch size:

default 25 or 50 files

Если request превышает BEEHIVE_SERVER_MAX_BODY_BYTES, уменьшать batch size или показывать понятную ошибку.

6.3 Import batch request

Endpoint:

POST /api/workspaces/{workspace_id}/entities/import-json-batch

Request:

{
  "stage_id": "raw_entities",
  "files": [
    {
      "relative_path": "folder/example.json",
      "file_name": "example.json",
      "content": {
        "any": "valid JSON object"
      }
    }
  ],
  "options": {
    "overwrite_existing": false
  }
}

Только JSON object. Если файл — массив, строка, число, null — считать invalid для B9 и вернуть ошибку по этому файлу.

6.4 S3 destination

Загружать в stage prefix:

s3://steos-s3-data/{workspace_prefix}/stages/{stage_id}/{file_name}

Если имя небезопасно, sanitize. Разрешить кириллицу. Запретить:

/
\\
..
control characters
empty filename

Если collision и overwrite_existing=false, использовать:

{name_without_ext}__{short_hash}.json
6.5 Identity derivation

Для каждого файла определить:

entity_id
artifact_id

Правила:

entity_id:
  1. content.entity_id, если это непустая строка
  2. content.id, если это непустая строка
  3. безопасный file stem + short hash

artifact_id:
  1. content.artifact_id, если это непустая строка
  2. entity_id + "__source"
  3. source__{short_hash}

Не использовать полный S3 key как entity_id.

6.6 S3 metadata

При upload обязательно ставить metadata:

beehive-entity-id
beehive-artifact-id
beehive-stage-id

Если текущий S3 client умеет только list/head, расширить его:

put_json_object(bucket, key, bytes, metadata)

Использовать существующие S3 credentials/env. Не передавать secrets в браузер.

6.7 DB registration

После upload зарегистрировать artifact как pending source artifact на выбранном stage.

Результат должен сразу появляться в workspace/entity UI без ручного “register source”.

Можно дополнительно запускать внутреннюю reconciliation, но пользователь не должен знать, что это нужно.

6.8 Partial failure

Один плохой файл не должен ломать весь batch.

Response:

{
  "uploaded_count": 0,
  "registered_count": 0,
  "skipped_count": 0,
  "invalid_count": 0,
  "failed_count": 0,
  "files": [
    {
      "relative_path": "example.json",
      "status": "uploaded|registered|skipped|invalid|failed",
      "entity_id": "entity_...",
      "artifact_id": "artifact_...",
      "bucket": "steos-s3-data",
      "key": "...",
      "error": null
    }
  ]
}
7. UI requirements
7.1 Workspace page

Main workspace page should focus on:

Upload entities
Reconcile S3
Run selected pipeline waves
Entities summary
Stages summary
Errors requiring attention

Diagnostics hidden by default.

7.2 Entities page/section

Add an Entities page or clear section:

Upload entities
Search
Filter by stage/status
Table
Archive
Restore
Open output/run details

Default columns:

checkbox
entity/display name
stage
status
S3 key short preview
updated/last run
actions

Hide by default:

checksum
etag
full metadata
internal counters
server paths
7.3 Stage Editor

Remove Connect Stages.

Remove Next stage.

Use only:

Create stage
Edit stage
Archive/Delete stage
Restore stage
Copy save_path aliases
Terminal stage checkbox
8. Required backend modules

Add or update:

src-tauri/src/services/entities.rs
src-tauri/src/services/workspaces.rs
src-tauri/src/services/pipeline.rs
src-tauri/src/s3_client.rs
src-tauri/src/http_api/mod.rs
src-tauri/src/domain/mod.rs

Add API client methods:

src/lib/apiClient/types.ts
src/lib/apiClient/httpClient.ts
src/lib/apiClient/tauriClient.ts
src/lib/runtimeApi.ts

Update UI:

src/pages/WorkspaceSelectorPage.tsx
src/pages/WorkspaceExplorerPage.tsx
src/pages/StageEditorPage.tsx
optional: src/pages/EntitiesPage.tsx
9. Tests

Workspace tests:

create workspace requires only name
create workspace generates id
create workspace uses steos-s3-data
create workspace uses ru-1
create workspace uses https://s3.ru-1.storage.selcloud.ru
workspace_prefix equals name after safe trim
unsafe workspace name rejected
archive/restore still works
old registry entries still load

Stage tests:

new S3 stage has next_stage = None
stage create request does not expose next_stage
stage update cannot change next_stage
next-stage HTTP endpoint is deprecated or absent
Terminal stage maps to allow_empty_outputs
non-terminal empty success still blocked by existing manifest validation
terminal empty success allowed

Entity/upload tests:

list entities works
archive entity hides it from default list
restore entity shows it again
import JSON batch rejects non-object JSON
import JSON batch derives entity_id/artifact_id
import JSON batch preserves Cyrillic filenames when safe
import JSON batch writes S3 metadata
import JSON batch registers pending stage state
import JSON batch handles partial failures

HTTP tests:

POST /api/workspaces accepts name-only request
POST /api/workspaces/{id}/entities/import-json-batch parses batch
DELETE/restore entity routes parse
selected-run B7 route still works
stage CRUD routes work without next_stage

Frontend/build:

cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
rg "@tauri-apps/api/core|invoke\\(" src -n
git diff --check
10. Smoke

Create:

scripts/web_operator_entities_smoke.mjs

Use temporary registry/root. Do not touch production workspace.

Smoke scenario:

1. Create workspace with name only.
2. Create one non-terminal stage.
3. Upload/import 2-3 JSON object files.
4. Confirm entities appear.
5. Archive one entity.
6. Confirm hidden by default.
7. Restore entity.
8. Confirm selected-run route still responds.
9. Archive/delete stage.
10. Archive/delete workspace.

If real S3 upload is used, use a dedicated test prefix and report exact keys. If using mock S3 trait, say so clearly.

11. Docs and feedback

Create:

docs/beehive_s3_b9_entities_upload_simplified_crud_plan.md
docs/beehive_s3_b9_entities_upload_simplified_crud_feedback.md
docs/operator_entities_upload_runbook.md

Feedback must include:

what changed
files changed
workspace create simplification
stage save_path-only changes
entity CRUD behavior
folder upload behavior
S3 key and metadata behavior
UI simplification
commands run
test results
smoke results
known risks
what remains for B10

Required checkpoint line:

ТЗ перечитано на этапах: after_plan, after_workspace_simplification, after_stage_save_path_only, after_entity_crud_design, after_upload_implementation, after_ui_update, after_tests, after_smoke, before_feedback
12. Acceptance criteria

B9 is accepted only if:

1. Workspace can be created from UI with name only.
2. Workspace creation does not ask for id/bucket/region/endpoint/prefix.
3. New workspace uses steos-s3-data, ru-1, https://s3.ru-1.storage.selcloud.ru.
4. workspace_prefix equals workspace name after safe trim/validation.
5. Stage UI has no Connect Stages.
6. Stage UI has no Next stage.
7. New stages always have next_stage = null.
8. UI uses Terminal stage wording.
9. Entity list exists.
10. Entity archive/restore exists.
11. Upload entities button exists in normal UI.
12. User can choose local folder of JSON files in browser mode.
13. Valid JSON object files upload to S3 under selected stage prefix.
14. Uploaded files are registered as pending artifacts.
15. Invalid files are reported without killing the whole batch.
16. Selected pipeline waves still work after upload.
17. Diagnostics remain available but are not default UI.
13. Non-goals

Do not implement:

full RBAC
Postgres migration
background worker
large 22k production run
n8n REST workflow editor
production workflow storage in repo
complex business JSON editor
visual graph editor
full README rewrite
14. Product principle

Do not expose configuration just because it exists internally.

For now Beehive is standardized on:

bucket: steos-s3-data
region: ru-1
endpoint: https://s3.ru-1.storage.selcloud.ru

Optimize for the current operator workflow, not for future migrations.


Короткое решение по направлению: **B9 должен принять B8 как базу, но убрать из интерфейса лишнюю
