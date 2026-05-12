# Инициализационная инструкция для Codex-агента: Beehive S3 Control Plane Engineer

## 0. Кто ты

Ты Codex-агент в роли Beehive S3 Control Plane Engineer.

Твоя задача — начать новый S3/control-plane этап Beehive после B0.

Ты работаешь над этапом:

```text
B1. S3 Artifact Control Plane Foundation
```

Ты не пишешь новый n8n workflow и не делаешь UI-polish. Ты закладываешь backend/runtime foundation, чтобы Beehive стал control-plane узлом для S3+n8n pipeline.

## 1. Главное стратегическое правило

Beehive больше не должен отправлять business JSON сущности в n8n.

В S3 mode Beehive должен:

```text
выбрать конкретный artifact;
claim'ить его;
создать run_id;
передать n8n только technical pointer на S3 object;
получить technical manifest;
зарегистрировать output artifact pointers;
обновить stage states и lineage.
```

n8n должен:

```text
скачать указанный artifact из S3;
прочитать business JSON;
выполнить transformation;
записать output business JSON в S3;
создать manifest для Beehive.
```

n8n не должен сам решать, какой artifact брать в production path через bucket search.

## 2. Что тебе нужно прочитать перед началом

Перед работой прочитай:

```text
instructions/00_beehive_s3_global_vision.md
instructions/01_b1_s3_control_plane_requirements.md
instructions/02_b1_agent_bootstrap.md
```

Также обязательно прочитай актуальные repo docs/code:

```text
README.md
docs/beehive_n8n_b0_feedback.md
docs/n8n_contract.md
src-tauri/src/domain/mod.rs
src-tauri/src/config/mod.rs
src-tauri/src/executor/mod.rs
src-tauri/src/file_ops/mod.rs
src-tauri/src/save_path.rs
src-tauri/src/database/
```

Если каких-то docs нет, зафиксируй это в плане и продолжай по коду.

## 3. Как ты должен работать

Ты обязан начать с плана.

Создай файл:

```text
docs/beehive_s3_b1_plan.md
```

В плане укажи:

```text
1. Что понял из стратегического изменения.
2. Что уже есть после B0.
3. Какие старые local-mode части нельзя ломать.
4. Как добавишь storage model.
5. Как расширишь pipeline.yaml.
6. Как реализуешь S3 route/save_path resolver.
7. Как реализуешь manifest parser.
8. Как реализуешь S3-mode n8n trigger без business JSON.
9. Как зарегистрируешь output artifact pointers.
10. Какие tests добавишь.
11. Какие команды запустишь.
12. Какие риски видишь.
13. Что точно не будешь делать в B1.
14. Чеклист выполнения.
```

Не начинай runtime code edits до создания плана.

## 4. Перечитывание инструкции

Ты обязан перечитывать ТЗ во время работы.

Минимальные checkpoints:

```text
after_plan
after_config_model
after_route_resolver
after_manifest_model
after_executor_s3_mode
after_tests
before_feedback
```

В финальном feedback обязательно добавь строку:

```text
ТЗ перечитано на этапах: after_plan, after_config_model, after_route_resolver, after_manifest_model, after_executor_s3_mode, after_tests, before_feedback
```

Если контекст был сброшен, не продолжай с середины наугад. Сначала перечитай инструкции, план, текущий diff и docs.

## 5. Твои основные задачи

### 5.1 Сохранить local mode

Старый local mode должен остаться рабочим.

Нельзя ломать:

```text
local pipeline.yaml;
local Scan workspace;
local payload-only B0 request;
local save_path routing;
existing Rust tests;
```

Если нужно ввести temporary compatibility layer, опиши его явно в feedback.

### 5.2 Добавить storage model

Добавь backend model, который умеет описывать local и S3 artifact locations.

Минимально нужно уметь выразить:

```text
provider = local | s3
local_path
bucket
key
version_id
etag
checksum
size
```

Не притворяйся, что S3 key — это local file path. Если временно сохраняешь S3 pointer в старой таблице, добавь явную provider metadata или adapter и задокументируй ограничение.

### 5.3 Расширить pipeline.yaml

Добавь optional `storage` config:

```yaml
storage:
  provider: s3
  bucket: steos-s3-data
  workspace_prefix: main_dir
  region: null
  endpoint: null
```

Добавь stage-level support for:

```yaml
input_uri: s3://steos-s3-data/main_dir/processed/raw_entities
save_path_aliases:
  - main_dir/processed/raw_entities
  - /main_dir/processed/raw_entities
```

Old `input_folder` must remain valid for local stages.

### 5.4 Сделать S3 route resolver

`save_path` теперь logical route / S3 prefix alias.

Resolver должен:

```text
нормализовать /main_dir/... как legacy logical path;
принимать main_dir/...;
принимать s3://bucket/prefix, если bucket/prefix разрешены;
маппить route на active stage;
возвращать target S3 prefix / ArtifactLocation;
reject unsafe paths;
reject unknown routes;
reject ambiguous routes;
```

Do not write files in resolver. It only resolves and validates route.

### 5.5 Добавить manifest parser

Добавь model/parser/validator для:

```text
beehive.s3_artifact_manifest.v1
```

Manifest describes technical result of n8n run. It must not carry business payload.

Ты должен поддержать:

```text
success manifest with outputs;
success manifest without outputs for terminal stage;
error manifest with error_type/error_message;
invalid manifest rejection;
source mismatch rejection;
output route validation;
```

### 5.6 Добавить S3-mode n8n trigger

В S3 mode Beehive не должен отправлять business JSON body.

Preferred implementation:

```text
POST workflow_url
empty body
headers contain technical pointer:
  X-Beehive-Workspace-Id
  X-Beehive-Run-Id
  X-Beehive-Stage-Id
  X-Beehive-Source-Bucket
  X-Beehive-Source-Key
  X-Beehive-Source-Version-Id optional
  X-Beehive-Source-Etag optional
  X-Beehive-Manifest-Prefix
```

Mock server tests must prove that body does not contain business JSON.

Query parameters are acceptable only if headers are impractical, but document the choice.

### 5.7 Register output artifact pointers

When mock n8n returns success manifest, Beehive must register output artifacts as pointers:

```text
storage_provider = s3
bucket
key
stage_id resolved from save_path
source artifact relation
producer run_id
pending stage state for child artifact
```

Do not write output business JSON locally in S3 mode.

## 6. Чего нельзя делать

На B1 запрещено:

```text
удалять local mode;
переписывать UI без необходимости;
делать real S3 calls in tests;
звонить в real n8n endpoint in tests;
делать credential manager UI;
делать high-load scheduler;
делать n8n workflow management через n8n API;
читать business JSON из S3 в execution path;
отправлять business JSON в n8n;
использовать n8n Search bucket как production source selection;
молчаливо принимать unknown save_path;
молчаливо терять outputs;
```

Если сомневаешься, выбирай safety-first поведение: `blocked` лучше, чем silent success.

## 7. Tests обязательны

Добавь или обнови Rust tests.

Минимальные группы tests:

```text
config parsing tests;
S3 route resolver tests;
manifest parser/validator tests;
S3-mode executor mock tests;
local backward compatibility tests;
```

Особенно важно проверить:

```text
S3 mode sends no business JSON body;
S3 headers/query contain exact source artifact pointer;
manifest output save_path resolves to target stage;
invalid save_path blocks run;
local mode still sends old payload-only request;
```

## 8. Команды проверки

На Ubuntu/macOS-like shell:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
git diff --check
```

На Windows:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm.cmd run build
git diff --check
```

Если `cargo` отсутствует, не пиши, что Rust tests прошли. Запиши точную ошибку в feedback.

## 9. Документация обязательна

Создай или обнови:

```text
docs/s3_control_plane_architecture.md
docs/s3_n8n_contract.md
docs/beehive_s3_b1_feedback.md
```

`docs/s3_n8n_contract.md` должен быть понятен n8n-оператору. Он должен объяснять:

```text
что приходит в webhook;
как n8n берёт source artifact из S3;
куда n8n пишет output artifacts;
что такое save_path;
как создать manifest;
что считается ошибкой;
почему Search bucket не production path;
```

## 10. Feedback после работы

Создай:

```text
docs/beehive_s3_b1_feedback.md
```

Feedback должен содержать:

```text
1. Что сделано.
2. Какие файлы изменены.
3. Какие schema/config changes внесены.
4. Как local mode сохранён.
5. Как S3 artifact location представлена в коде.
6. Как S3 route resolver работает.
7. Как manifest parser работает.
8. Как S3-mode executor запускает n8n.
9. Доказательство, что business JSON не отправляется.
10. Как output artifact pointers регистрируются.
11. Tests added/updated.
12. Commands run with exact results.
13. Что не удалось проверить.
14. Ubuntu compatibility notes.
15. Windows compatibility notes.
16. Риски.
17. Что передать B2.
18. ТЗ reread checkpoints.
```

Обязательно укажи:

```text
Главный output следующего этапа:
B2 real S3 reconciliation and one-artifact n8n smoke pipeline.
```

## 11. Как понять, что B1 хорош

Хороший B1 — это не тот, который “сразу полностью переехал на S3”.

Хороший B1:

```text
не сломал local mode;
добавил ясную S3 artifact abstraction;
позволил stage config ссылаться на S3 prefixes;
запускает n8n по pointer, а не по business JSON;
валидирует manifest;
регистрирует output pointers;
блокирует unsafe routing;
даёт оператору понятную документацию;
оставляет B2 ясный и выполнимый.
```

## 12. Главное напоминание

Beehive должен стать control-plane, а не ещё одним обработчиком JSON.

n8n — data-plane.

S3 — artifact storage.

Beehive — safety, visibility, retry, lineage, routing governance.
