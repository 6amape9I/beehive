# Глобальное видение Beehive S3/n8n control-plane

## 0. Контекст

Beehive начинался как локальный desktop-оркестратор JSON stage pipelines.

Ранняя модель была такой:

```text
локальный workdir → stage folders → JSON files → Beehive отправляет payload в n8n → Beehive сохраняет output files
```

После B0 runtime contract уже сдвинулся в правильную сторону:

```text
Beehive не отправляет свои runtime metadata в n8n
n8n получает только business payload
n8n может возвращать multi-output items с save_path
Beehive валидирует save_path и управляет stage state
```

Теперь стратегическое решение меняется сильнее.

Мы переходим от локального хранения к S3 artifact storage.

Новая целевая модель:

```text
S3 хранит business artifacts
n8n читает business artifacts из S3 и пишет business artifacts в S3
Beehive управляет задачами, статусами, routing, retry, lineage и аудитом
```

Beehive больше не является приложением, которое передаёт JSON-сущности в n8n. Beehive становится control-plane узлом над n8n/S3 pipeline.

## 1. Главная цель проекта

Цель Beehive — превратить набор n8n workflow в серьёзное operator-grade pipeline решение.

Пользователь, который умеет работать только с n8n-блоками, должен видеть:

```text
какие сущности есть;
на каком stage находится каждая сущность;
какой workflow её обрабатывал;
какие artifacts появились после обработки;
куда n8n разветвил outputs;
что упало в retry;
что blocked;
что failed;
что можно безопасно перезапустить;
что не потерялось;
какая история lineage у каждой сущности.
```

n8n остаётся удобным low-code data-plane инструментом. Beehive делает то, что n8n сам по себе обычно не гарантирует: строгую видимость, safety, idempotency, routing governance и runtime state.

## 2. Новое разделение обязанностей

### 2.1 Beehive делает

Beehive отвечает за control plane:

```text
pipeline registry;
stage registry;
allowed save_path / route registry;
S3 artifact pointers;
artifact lifecycle;
stage state machine;
claim задач;
retry policy;
stuck task reconciliation;
blocked/failed accounting;
run audit;
lineage;
operator dashboard;
manual retry/reset/skip;
manifest validation;
S3 reconciliation;
безопасность маршрутизации;
```

Beehive должен знать, что существует artifact, где он лежит, какой stage его ожидает, кто его породил и что с ним произошло.

Beehive не должен знать внутреннюю бизнес-структуру каждого JSON, чтобы запустить stage. Он может показывать preview только как отдельное read-only операторское действие, но execution path не должен зависеть от чтения business JSON.

### 2.2 n8n делает

n8n отвечает за data plane:

```text
скачать конкретный artifact из S3;
распарсить business JSON;
выполнить LLM/API/Postgres/HTTP transformation;
сформировать output business JSON;
положить output artifacts в S3;
указать save_path для осознанного branching;
создать technical manifest результата;
вернуть или сохранить manifest для Beehive.
```

n8n не должен самостоятельно решать, какой artifact взять в production path. Поисковые S3 nodes в n8n допустимы только для ручной отладки и demo.

Production-вход n8n должен быть:

```text
Beehive selected artifact → Beehive claimed task → Beehive triggers n8n with artifact pointer → n8n processes exactly that artifact
```

### 2.3 S3 делает

S3 — единое shared artifact storage:

```text
source business JSON artifacts;
intermediate business JSON artifacts;
final business JSON artifacts;
run manifests;
error manifests;
optional dead-letter artifacts;
```

S3 не является источником runtime truth. Runtime truth остаётся в Beehive control-plane database.

## 3. Принцип: приложение не отправляет business JSON

Beehive не отправляет business JSON сущности в n8n.

Для запуска n8n stage Beehive передаёт только technical pointer/control data:

```text
workspace_id
run_id
stage_id
source artifact bucket
source artifact key
source artifact version/etag, если доступно
manifest output prefix
```

Предпочтительно передавать эти данные через HTTP headers или query parameters, чтобы не смешивать control data с business JSON body.

Если на конкретном этапе временно используется JSON control envelope, он должен быть строго техническим и не содержать business payload. Но целевой UX/contract: n8n получает pointer, а не JSON-сущность.

## 4. Beehive как узел pipeline

Beehive — не файловый менеджер и не low-code workflow builder.

Beehive — control node, который:

```text
выбирает eligible artifact;
claim'ит его атомарно;
создаёт run_id;
запускает n8n webhook;
передаёт pointer на S3 object;
ждёт technical manifest или reconcile'ит manifest позже;
валидирует output routes;
регистрирует child artifacts;
переводит states;
создаёт историю lineage.
```

Это даёт n8n-пайплайнам управляемость, которой нет в ручном режиме.

## 5. Почему n8n не должен сам искать входы

Если n8n сам делает `Search bucket` и берёт первые N файлов, появляются проблемы:

```text
нет атомарного claim;
нет гарантии, что два workflow не взяли один artifact;
нет backpressure;
нет понятного retry;
нет строгого соответствия run_id → source artifact;
сложно объяснить оператору, почему artifact обработан или пропущен;
Beehive становится наблюдателем, а не оркестратором.
```

Поэтому S3 search/list в n8n допустимы только для demo и ручного эксперимента. В production n8n должен получать конкретный artifact pointer от Beehive.

## 6. Новый pipeline contract

Целевой production flow:

```text
1. Beehive registers S3 artifact.
2. Artifact state is pending on stage A.
3. Operator or runner calls Run due tasks.
4. Beehive atomically claims artifact: pending → queued → in_progress.
5. Beehive calls n8n stage webhook with technical pointer only.
6. n8n downloads exact source artifact from S3.
7. n8n transforms data.
8. n8n uploads output business JSON artifacts to S3.
9. n8n produces run manifest.
10. Beehive validates manifest.
11. Beehive registers output artifacts and routes them to target stages.
12. Source state becomes done, child states become pending.
```

Failure flow:

```text
network/timeout → retry_wait or failed
n8n business error manifest → retry_wait/failed depending policy
invalid save_path → blocked
unknown target stage → blocked
manifest missing outputs when outputs required → blocked or failed by policy
source artifact missing before run → skipped/released, no attempt consumed
```

## 7. Manifest-first thinking

Beehive should not read every output business JSON to know what happened.

n8n must produce a technical manifest:

```json
{
  "schema": "beehive.s3_artifact_manifest.v1",
  "workspace_id": "semantic-dev",
  "run_id": "run_...",
  "source": {
    "bucket": "steos-s3-data",
    "key": "main_dir/raw/input_001.json",
    "version_id": null,
    "etag": null
  },
  "status": "success",
  "outputs": [
    {
      "artifact_id": "art_...",
      "bucket": "steos-s3-data",
      "key": "main_dir/processed/raw_entities/art_....json",
      "save_path": "main_dir/processed/raw_entities",
      "content_type": "application/json",
      "checksum_sha256": null,
      "size": 12345
    }
  ],
  "created_at": "2026-05-12T00:00:00Z"
}
```

Error manifest:

```json
{
  "schema": "beehive.s3_artifact_manifest.v1",
  "workspace_id": "semantic-dev",
  "run_id": "run_...",
  "source": {
    "bucket": "steos-s3-data",
    "key": "main_dir/raw/input_001.json"
  },
  "status": "error",
  "error_type": "llm_invalid_json",
  "error_message": "Model returned invalid JSON",
  "outputs": [],
  "created_at": "2026-05-12T00:00:00Z"
}
```

Beehive validates manifests and updates state. Business JSON remains in S3.

## 8. save_path становится logical route, не local path

`save_path` сохраняется как язык осознанного branching для n8n-оператора.

Но теперь `save_path` — это logical route / S3 prefix alias, а не локальная папка.

Пример:

```text
save_path: main_dir/processed/raw_entities
```

maps to:

```text
stage_id: raw_entities
input_uri: s3://steos-s3-data/main_dir/processed/raw_entities
```

Legacy values with leading `/main_dir/...` may be normalized for compatibility, but Beehive must never treat them as OS absolute paths.

Invalid routes must become `blocked`, not silent output loss.

## 9. Database direction

Current SQLite can remain for MVP and single-operator development.

But the model must not hard-code local files forever. Tables and structs should move conceptually from:

```text
entity_files.file_path
```

to:

```text
entity_artifacts.storage_provider
entity_artifacts.bucket
entity_artifacts.key
entity_artifacts.version_id
entity_artifacts.etag
entity_artifacts.size
entity_artifacts.checksum
entity_artifacts.stage_id
entity_artifacts.source_artifact_id
```

The existing state machine remains valuable and should be preserved:

```text
entity_stage_states
stage_runs
app_events
```

For multi-user production, the future direction is central control-plane storage such as Postgres or an API service. Do not implement this in the first S3 stage, but avoid new design choices that make it impossible.

## 10. Cross-platform requirement

The app must work on Windows and Ubuntu.

Do not encode Windows-only assumptions:

```text
npm.cmd only
cmd.exe only
PowerShell-only paths
backslash-only paths
MSVC-only verification
local absolute /main_dir paths
```

Path/routing logic should use logical slash-separated routes for S3. OS-specific filesystem paths must not leak into S3 routing.

## 11. What is intentionally deferred

Deferred for later stages:

```text
real high-load scheduler;
advanced backpressure;
parallel worker pool;
S3 credential manager UI;
multi-user central database;
n8n workflow management through n8n REST API;
visual graph builder;
full migration of old local DB data;
preview/editor for S3 business JSON;
```

The immediate priority is not scale. The immediate priority is a correct, safe, visible S3/n8n control-plane contract.

## 12. First S3 milestone

The next implementation stage is:

```text
B1. S3 Artifact Control Plane Foundation
```

B1 should prove that Beehive can:

```text
understand S3 artifacts;
configure S3 stages;
claim a specific artifact;
trigger n8n without sending business JSON;
accept/parse a technical manifest;
register output artifact pointers;
route outputs by save_path;
keep runtime state and lineage;
remain compatible with local mode for old tests/demo.
```

B1 does not need to call real S3. It can use mock manifests and mock HTTP servers. Real S3 reconciliation can be the next stage.

## 13. Главный вывод

Beehive должен стать не “локальным JSON прогонщиком”, а операторским control-plane слоем для S3+n8n pipelines.

n8n делает transformation. S3 хранит data. Beehive делает system safety.
