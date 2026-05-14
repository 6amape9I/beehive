# B6. Web Operator MVP Server, Workspace Flow, and Stage Creation UX — инструкция для Codex-агента

## 0. Роль и контекст

Ты Codex-агент, работающий над Beehive после принятого B5.

B5 сделал переходную архитектурную основу: backend service layer, HTTP-shaped router, frontend API client boundary, server-side workspace registry, stage creation service и multi-output lineage read model. Но B5 ещё не является настоящим web-приложением: нет standalone web server binary, нет полноценного browser flow, часть HTTP adapter методов возвращает `unsupported`, а нормальный пользовательский сценарий всё ещё частично завязан на Tauri/workdir-open semantics.

B6 должен превратить подготовку B5 в минимально рабочий web-MVP для оператора.

Главное правило B6:

```text
Оператор не пользуется терминалом для рабочих действий.
Оператор открывает web UI, выбирает workspace, создаёт stage, запускает reconcile/run, смотрит статусы и lineage.
```

Командная строка допустима только для администратора/разработчика, чтобы запустить сервер и прогнать проверки.

## 1. Что сохранить из предыдущих этапов

Не переписывай ядро.

Сохрани и переиспользуй:

```text
B4 run_pipeline_waves
B3/B4 run_due_tasks_limited
S3 reconciliation
manual S3 source registration
S3 JSON control envelope
manifest validation
save_path routing
transactional S3 output pointer registration
state machine/retry/block behavior
B5 service layer
B5 workspace registry
B5 API client boundary
B5 stage creation service
B5 multi-output lineage read model
```

B6 — это productization/web-MVP поверх уже работающего ядра, а не новая runtime-архитектура.

## 2. Текущие проблемы, которые B6 обязан закрыть

### 2.1 Нет runnable web server

B5 добавил HTTP-shaped router, но не добавил standalone server binary. В B6 нужно добавить настоящий server entrypoint, который можно запустить и открыть в браузере.

### 2.2 Browser flow всё ещё смешан с Tauri open-workdir

В HTTP/browser mode выбор workspace не должен вызывать `openRegisteredWorkspace()` как desktop-действие. Browser должен хранить `workspace_id` в route/state и все обычные operator pages должны работать через workspace-aware API.

### 2.3 Frontend HTTP adapter неполный

Для нормального web-MVP должны работать хотя бы эти browser actions:

```text
list workspaces
select workspace
workspace explorer
create S3 stage
reconcile S3
register S3 source artifact
run small batch
run pipeline waves
list stage runs or at least stage-run output expansion
list stage-run outputs / multi-output lineage
```

Старые admin-only desktop actions могут оставаться unsupported в HTTP mode, но normal operator path не должен натыкаться на unsupported methods.

### 2.4 Stage creation есть, но нужен операторский UX

Оператор создаёт stage на основании готового n8n workflow.

Форма должна быть простой:

```text
stage_id
production webhook URL
optional next_stage
max_attempts / retry_delay_sec / allow_empty_outputs с дефолтами
```

Оператор не вводит S3 папки. Backend генерирует `input_uri` и `save_path_aliases`.

### 2.5 Pipeline linking должен быть удобнее top-down

B5 stage creation отклоняет `next_stage`, если target stage ещё не существует. Это безопасно, но для оператора неудобно. B6 должен добавить лёгкий способ соединять stage после создания, без ручного YAML.

Минимально допустимо:

```text
Create stage without next_stage
Connect stages action: source_stage_id -> target_stage_id or clear next_stage
```

Не надо строить полноценный graph editor. Достаточно простой UI/API для установки `next_stage` между существующими stage.

### 2.6 Multi-output должен быть видимым

Один вход может породить несколько outputs на один или несколько target stages. B6 должен сделать это видимым в web UI:

```text
stage_run -> outputs[]
output artifact -> target stage
output artifact -> runtime status
output artifact -> S3 URI
relation_to_source
producer_run_id
```

Нельзя показывать только один `created_child_path`.

### 2.7 Live n8n preflight — лёгкий, не сверхтестирование

Не превращай n8n проверку в огромный validator. Нужна только практическая preflight-проверка перед pilot:

```text
webhook POST + responseNode
source_bucket/source_key читаются из JSON body, не из headers
нет старой ссылки на Read Beehive headers
нет production Search/List Bucket как source selection
нет /main_dir/pocessed typo
manifest возвращается синхронно
save_path route-compatible
```

Полные production workflows не коммитить в репозиторий. Они живут в n8n. В repo можно хранить только маленькие contract examples.

## 3. Обязательный план до кода

Перед кодом создай:

```text
docs/beehive_s3_b6_web_mvp_plan.md
```

План должен включать:

```text
1. Что понял из задачи.
2. Какие B5 pieces будут переиспользованы.
3. Какой server binary будет добавлен.
4. Как server будет bind-иться и защищаться.
5. Какие HTTP endpoints станут реально доступными.
6. Как frontend перейдёт на workspace_id routes.
7. Как будет работать workspace selector в HTTP mode.
8. Как будет работать create stage.
9. Как будет работать connect stages / pipeline links.
10. Как будет показываться multi-output lineage.
11. Какие manual QA / smoke checks будут выполнены.
12. Что не будет реализовано в B6.
13. Риски и rollback.
14. Checkpoints перечитывания ТЗ.
```

Не начинай код до плана.

## 4. Перечитывание ТЗ

Минимальные checkpoints:

```text
after_plan
after_server_binary
after_workspace_routes
after_frontend_http_flow
after_stage_creation_and_links
after_multi_output_ui
after_tests
after_http_smoke
before_feedback
```

В feedback добавь строку:

```text
ТЗ перечитано на этапах: after_plan, after_server_binary, after_workspace_routes, after_frontend_http_flow, after_stage_creation_and_links, after_multi_output_ui, after_tests, after_http_smoke, before_feedback
```

## 5. Backend требования

### 5.1 Добавить runnable server binary

Добавить standalone binary:

```text
src-tauri/src/bin/beehive-server.rs
```

И соответствующий `[[bin]]` в `src-tauri/Cargo.toml`, если требуется.

Server должен:

```text
bind по умолчанию к 127.0.0.1
порт по умолчанию 8787 или другой явный dev порт
читать BEEHIVE_SERVER_HOST
читать BEEHIVE_SERVER_PORT
читать BEEHIVE_WORKSPACES_CONFIG
использовать существующий http_api/router/service layer
отдавать JSON API
иметь GET /api/health
логировать адрес запуска и registry path
```

Рекомендуемый подход: использовать минимальный Rust HTTP framework. Если добавляешь зависимости, добавь ровно необходимые и объясни в feedback. Предпочтительно `axum` + `tower-http`, но допустим иной простой вариант, если он лучше вписывается в проект.

### 5.2 Security baseline

По умолчанию server должен быть local-only:

```text
host = 127.0.0.1
```

Если оператор/админ пытается bind не на localhost, server должен требовать явный opt-in:

```text
BEEHIVE_SERVER_ALLOW_NON_LOCAL=1
```

Если bind не localhost, обязательно требовать token:

```text
BEEHIVE_OPERATOR_TOKEN
```

Минимальный токен-механизм:

```text
Authorization: Bearer <token>
```

Для localhost token можно не требовать, но если `BEEHIVE_OPERATOR_TOKEN` задан, то проверяй его и на localhost.

Не логируй S3 keys/secrets/tokens.

### 5.3 HTTP endpoints B6 MVP

Должны реально работать через server:

```text
GET  /api/health
GET  /api/workspaces
GET  /api/workspaces/{workspace_id}
GET  /api/workspaces/{workspace_id}/workspace-explorer
POST /api/workspaces/{workspace_id}/reconcile-s3
POST /api/workspaces/{workspace_id}/register-s3-source
POST /api/workspaces/{workspace_id}/run-small-batch
POST /api/workspaces/{workspace_id}/run-pipeline-waves
POST /api/workspaces/{workspace_id}/stages
GET  /api/workspaces/{workspace_id}/stage-runs/{run_id}/outputs
```

Добавить для stage linking:

```text
POST /api/workspaces/{workspace_id}/stages/{stage_id}/next-stage
```

Request:

```json
{
  "next_stage": "target_stage_id"
}
```

Для clear:

```json
{
  "next_stage": null
}
```

Backend должен:

```text
validate source stage exists
validate target stage exists if not null
reject self-link
update pipeline.yaml atomically
sync SQLite stages
return updated stage or route/pipeline summary
```

### 5.4 Workspace ID boundary

Browser-originated API никогда не должен принимать arbitrary filesystem path.

Правильно:

```text
workspace_id -> backend registry -> workdir/pipeline/database paths
```

Неправильно:

```text
browser sends /tmp/path/to/app.db
browser sends arbitrary workdir path
```

Path-based Tauri/admin commands можно оставить, но web endpoints должны работать только через workspace_id.

### 5.5 Static frontend serving

B6 должен дать хотя бы один понятный способ открыть web UI.

Допустимые варианты:

Вариант A, предпочтительный:

```text
beehive-server serves API and built frontend from dist/
```

Вариант B, допустимый для MVP:

```text
beehive-server serves API only
Vite dev server serves frontend
frontend uses VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787
```

Если выбран B, feedback должен честно сказать, что static serving будет B7/B6.1. Но acceptance B6 требует browser smoke через Vite + HTTP API.

## 6. Frontend требования

### 6.1 Workspace routes

Перевести normal operator flow на routes с workspace_id.

Минимальные routes:

```text
/workspaces
/workspaces/:workspaceId/workspace
/workspaces/:workspaceId/stages
/workspaces/:workspaceId/entities/:entityId?
```

Если часть старых routes остаётся, они не должны ломать Tauri mode.

### 6.2 Workspace selector in HTTP mode

В HTTP mode выбор workspace не должен вызывать `openRegisteredWorkspace()`.

Он должен:

```text
load GET /api/workspaces
store selected_workspace_id in app state
navigate to /workspaces/{workspace_id}/workspace
use workspace_id for subsequent API calls
```

В Tauri mode можно оставить openRegisteredWorkspace как admin/dev behavior.

### 6.3 API client

`src/lib/apiClient/httpClient.ts` должен поддерживать весь normal B6 operator path.

Normal web operator path не должен вызывать unsupported methods.

Проверь и поправь components/pages, чтобы они использовали workspace-aware methods в HTTP mode:

```text
getWorkspaceExplorerById
reconcileS3WorkspaceById
registerS3SourceArtifactById
runDueTasksLimitedById
runPipelineWavesById
createS3Stage
listStageRunOutputs
connect/update next_stage
```

### 6.4 Stage creation UI

В web mode оператор должен уметь создать S3 stage:

```text
stage_id
workflow_url
optional next_stage existing only
max_attempts
retry_delay_sec
allow_empty_outputs
```

После создания UI должен показать:

```text
input_uri
save_path_aliases
copy buttons для save_path aliases
подсказку: n8n manifest outputs must use one of these save_path values
```

### 6.5 Pipeline links / connect stages UI

Добавить простой UI для соединения stage:

```text
source stage dropdown
target stage dropdown / terminal
save button
```

Цель: оператор может создать stage A и stage B в любом порядке, затем соединить A -> B.

Не надо делать drag-and-drop graph editor.

### 6.6 Workspace overview / explorer

Browser operator должен видеть:

```text
stages
input_uri / save_path aliases where useful
counts by runtime status
S3 pointer rows
producer_run_id
bucket/key/S3 URI
missing/present state
buttons: Reconcile S3, Register S3 source, Run small batch, Run pipeline waves
```

### 6.7 Multi-output lineage UI

В Entity Detail или Stage Runs panel должен быть web-compatible expansion:

```text
stage run row -> expand outputs
```

Показать:

```text
output_count
entity_id
artifact_id
target_stage_id
relation_to_source
runtime_status
s3_uri
bucket/key
size/checksum if available
```

Это должно работать через HTTP endpoint:

```text
GET /api/workspaces/{workspace_id}/stage-runs/{run_id}/outputs
```

## 7. n8n workflow handling в B6

### 7.1 Не коммитить production workflows

Не добавляй реальные production n8n pipeline JSON в repository. Они являются внешними артефактами.

Можно хранить только маленький пример контракта, если он уже есть.

### 7.2 Лёгкий preflight

Добавить или обновить doc:

```text
docs/n8n_web_mvp_preflight_b6.md
```

В нём зафиксировать checklist:

```text
POST + responseNode
JSON body control envelope
source_bucket/source_key из body
не использовать source_key headers
не использовать Search/List Bucket для production source selection
нет ссылок на старые node names типа Read Beehive headers
нет /main_dir/pocessed typo
manifest schema beehive.s3_artifact_manifest.v1
outputs contain artifact_id/entity_id/relation_to_source/bucket/key/save_path
```

Не надо писать “сверхвалидатор всего n8n”. Достаточно практического checklist и, если уже есть lightweight linter, не расширяй его чрезмерно.

## 8. Manual QA / smoke сценарий B6

B6 должен доказать, что операторский browser path работает.

Минимальный smoke:

### 8.1 Server/API smoke

Команды:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin beehive-server
curl -sS http://127.0.0.1:8787/api/health
curl -sS http://127.0.0.1:8787/api/workspaces
```

Если порт другой — указать в feedback.

### 8.2 Frontend HTTP-mode smoke

Запустить frontend с HTTP adapter:

```bash
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
```

Если возможно, также:

```bash
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run dev
```

И вручную/через documented steps проверить:

```text
/workspaces loads registry
select workspace opens workspace-scoped page
workspace explorer loads without Tauri invoke
create S3 stage works or rejects with clear validation
route hints are shown
reconcile S3 runs
run small batch or waves works when test data available
stage-run outputs expansion works when run_id has outputs
```

### 8.3 Optional real S3/n8n smoke

Если existing smoke workspace and n8n endpoint доступны, выполнить один web/API-triggered operation:

```text
reconcile S3
run small batch or waves
inspect output lineage in web UI/API
```

Если real S3/n8n не запускается в B6, это не blocker, если HTTP-mode browser path доказан на registry/mock/local data. Но feedback должен честно указать, что real S3/n8n smoke не запускался.

## 9. Tests

Обязательные проверки:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
git diff --check
```

Добавить/обновить tests:

```text
server/router health route
server/router workspace list route
server rejects non-local bind without opt-in/token, if implemented in testable form
workspace selector/client HTTP adapter calls correct endpoints
stage next-stage linking service tests
stage creation tests still pass
stage-run outputs tests still pass
no direct Tauri invoke outside apiClient/tauriClient.ts
```

Если frontend tests отсутствуют, минимум сохранить `npm run build` и `rg` check:

```bash
rg "@tauri-apps/api/core|invoke\(" src -n
```

Результат должен быть только в `src/lib/apiClient/tauriClient.ts`.

## 10. Документация B6

Создать:

```text
docs/beehive_s3_b6_web_mvp_plan.md
docs/beehive_s3_b6_web_mvp_feedback.md
docs/web_operator_mvp_runbook.md
docs/n8n_web_mvp_preflight_b6.md
```

Обновить при необходимости:

```text
docs/front_back_split.md
docs/workspace_registry.md
docs/stage_creation_s3_ui_contract.md
docs/multi_output_lineage.md
```

Не делать большой финальный README rewrite. README можно минимально дополнить ссылкой на B6 runbook, но полный README продукта будет финальным этапом.

## 11. Не цели B6

B6 не должен делать:

```text
high-load scheduler
background daemon
worker pool
async manifest polling
n8n REST workflow editor
production n8n workflow storage in repo
full auth/RBAC
Postgres migration
multi-user locking model
large 22k-file production run
full README rewrite
full n8n semantic validator
```

## 12. Acceptance criteria

B6 принимается, если:

```text
1. Есть runnable beehive-server или честно документированный API server command.
2. GET /api/health работает через curl.
3. GET /api/workspaces работает через curl.
4. Browser/frontend HTTP mode может загрузить workspace selector.
5. Normal web operator flow использует workspace_id, а не arbitrary workdir path.
6. Оператор может создать S3 stage через UI/API по stage_id + webhook URL.
7. Backend генерирует input_uri/save_path_aliases.
8. Оператор может соединить stages через простой UI/API или clear terminal state.
9. Reconcile S3 / run small batch / run pipeline waves доступны через web/API path.
10. Multi-output lineage outputs[] доступны через HTTP and visible in UI or documented panel.
11. Direct Tauri invoke из React ограничен apiClient/tauriClient.ts.
12. Tests/build проходят.
13. Feedback честно описывает, что работало в Tauri mode, HTTP mode, browser smoke, and what remains unsupported.
```

## 13. Feedback требования

Создать:

```text
docs/beehive_s3_b6_web_mvp_feedback.md
```

Feedback должен содержать:

```text
1. Что было реализовано.
2. Какие B5/B4 pieces сохранены.
3. Как запустить server.
4. Какой host/port используется.
5. Какие endpoints работают.
6. Как включается token/non-local bind protection.
7. Как запустить frontend HTTP mode.
8. Какие browser/operator сценарии проверены.
9. Stage creation evidence.
10. Stage linking evidence.
11. Multi-output lineage evidence.
12. Какие команды запускались и точные результаты.
13. Что не было проверено.
14. Какие HTTP adapter methods всё ещё unsupported.
15. Какие Tauri dependencies остаются.
16. Какие риски остаются перед B7.
17. Что передать следующему этапу.
18. Строка reread checkpoints.
```

## 14. Ожидаемое состояние после B6

После B6 Beehive должен быть уже не просто Tauri desktop tool with web-ready abstractions, а минимальный browser-first operator MVP:

```text
admin запускает server
оператор открывает browser UI
оператор выбирает workspace
оператор создаёт stage из готового n8n webhook
оператор видит route hints/save_path
оператор запускает S3 reconcile/run
оператор видит статусы и one-to-many outputs
```

Это всё ещё не production-grade multi-user system, но это уже правильное направление продукта для коллег, которые не пользуются терминалом.
