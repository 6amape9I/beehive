# Мини-гайд: реальный S3+n8n smoke для Beehive

## Цель

Проверить один минимальный production-like путь:

```text
selected_50 JSON -> S3 raw prefix -> Beehive registers/claims source artifact -> n8n receives only S3 pointer headers -> n8n downloads exactly that object -> n8n uploads output JSON to S3 processed prefix -> n8n returns manifest -> Beehive registers child pointer -> source done, child pending/visible
```

## Что подготовить человеку до запуска Codex-агента

### 1. S3 credentials в окружении

Создать локальный `.env` или экспортировать переменные в shell, где будет запускаться агент/приложение:

```bash
export S3_HOST="s3.ru-1.storage.selcloud.ru"
export S3_REGION="ru-1"
export S3_KEY="***"
export S3_SEC_KEY="***"
export S3_BUCKET_NAME="steos-s3-data"
export BEEHIVE_SMOKE_PREFIX="beehive-smoke/test_workflow"
```

Для AWS SDK/Beehive также можно продублировать:

```bash
export AWS_ACCESS_KEY_ID="$S3_KEY"
export AWS_SECRET_ACCESS_KEY="$S3_SEC_KEY"
export AWS_REGION="$S3_REGION"
export BEEHIVE_S3_ENDPOINT="https://$S3_HOST"
```

Нельзя коммитить секреты.

### 2. n8n workflow

Импортировать файл:

```text
n8n_beehive_s3_pointer_smoke_workflow.json
```

После импорта:

1. Привязать S3 credentials к двум S3 nodes: `Download source object` и `Upload smoke output`.
2. Убедиться, что S3 credentials смотрят на Selectel endpoint `https://s3.ru-1.storage.selcloud.ru`, region `ru-1`.
3. Активировать workflow.
4. Скопировать production webhook URL. Он будет похож на:

```text
https://n8n-dev.steos.io/webhook/beehive-s3-pointer-smoke
```

Если n8n при импорте ругается на `fileKey` у upload node, открыть node `Upload smoke output` и вручную выбрать:

```text
Operation: Upload
Bucket Name: ={{ $json.output_bucket }}
File Name / Key: ={{ $json.output_key }}
Binary Property: data
```

### 3. selected_50_for_n8n.zip

Положить архив рядом с smoke kit или передать агенту путь к нему.

Smoke kit уже содержит скрипт:

```bash
python3 prepare_selected50_s3_smoke.py \
  --zip selected_50_for_n8n.zip \
  --out s3_smoke_dataset \
  --prefix "$BEEHIVE_SMOKE_PREFIX" \
  --limit 50
```

Он создаёт 50 source JSON objects и upload script.

### 4. Загрузка 50 source objects в S3

Из каталога `s3_smoke_dataset` выполнить:

```bash
./upload_selected50_to_s3.sh
```

Проверить:

```bash
ENDPOINT="https://${S3_HOST}"
aws s3 ls --endpoint-url "$ENDPOINT" \
  "s3://${S3_BUCKET_NAME}/${BEEHIVE_SMOKE_PREFIX}/raw/"
```

Должно быть 50 JSON files.

### 5. Beehive workdir и pipeline.yaml

Создать новый workdir, например:

```text
/tmp/beehive_s3_smoke_workdir
```

Скопировать туда `pipeline.s3_smoke.example.yaml` как:

```text
pipeline.yaml
```

В `pipeline.yaml` заменить:

```text
workflow_url: https://n8n-dev.steos.io/webhook/REPLACE_WITH_IMPORTED_SMOKE_WEBHOOK_PATH
```

на production webhook URL импортированного n8n workflow.

## Как проверить вручную

### Вариант A: через Beehive UI

1. Открыть workdir в Beehive.
2. Запустить `Reconcile S3 workspace`, если кнопка/команда доступна.
3. Проверить, что появились source artifacts в stage `smoke_source`.
4. Запустить `Run due tasks` с `max_parallel_tasks: 1`.
5. Проверить:
   - source stage state стал `done`;
   - появился stage_run с `success=true`;
   - `stage_runs.request_json` содержит technical pointer, но не содержит business JSON;
   - появился child artifact в `smoke_processed`;
   - child artifact имеет S3 key под `beehive-smoke/test_workflow/processed/`.

### Вариант B: через SQLite после запуска

```bash
sqlite3 /tmp/beehive_s3_smoke_workdir/app.db \
  "select entity_id, stage_id, status, file_exists from entity_stage_states order by updated_at desc limit 20;"

sqlite3 /tmp/beehive_s3_smoke_workdir/app.db \
  "select run_id, entity_id, stage_id, success, http_status, error_type from stage_runs order by id desc limit 10;"

sqlite3 /tmp/beehive_s3_smoke_workdir/app.db \
  "select entity_id, artifact_id, stage_id, storage_provider, bucket, object_key, producer_run_id from entity_files order by id desc limit 20;"
```

### Проверка S3 output

```bash
aws s3 ls --endpoint-url "https://${S3_HOST}" \
  "s3://${S3_BUCKET_NAME}/${BEEHIVE_SMOKE_PREFIX}/processed/"
```

Должен появиться хотя бы один output JSON.

## Что считается успешным smoke

Минимальный успех:

```text
1 source S3 artifact registered or reconciled
1 Run due tasks executed
n8n received empty body + X-Beehive-* headers
n8n downloaded source object from S3
n8n uploaded output object to processed prefix
n8n returned valid beehive.s3_artifact_manifest.v1
Beehive marked source state done
Beehive registered child S3 pointer in smoke_processed
```

Полный успех для пачки:

```text
50 source artifacts uploaded
50 source artifacts discovered/registered
N runs executed, where N can start with 1 and later become 50
No silent lost entities
Failures are retry_wait/failed/blocked with visible errors
```

## Если что-то падает

### Beehive blocks manifest route

Проверить, что n8n manifest output has:

```text
save_path = beehive-smoke/test_workflow/processed
key starts with beehive-smoke/test_workflow/processed/
bucket = steos-s3-data
```

### n8n cannot download from S3

Проверить S3 credentials в n8n и endpoint/region.

### Beehive cannot list S3

Проверить env vars в shell, откуда запущен Beehive/Codex:

```bash
env | grep -E 'S3_|AWS_|BEEHIVE_S3_'
```

### Objects are unmapped

Проверить, что source objects были загружены с metadata:

```bash
aws s3api head-object --endpoint-url "https://${S3_HOST}" \
  --bucket "$S3_BUCKET_NAME" \
  --key "${BEEHIVE_SMOKE_PREFIX}/raw/smoke_entity_001__porfiriya.json"
```

В metadata должны быть:

```text
beehive-entity-id
beehive-artifact-id
beehive-stage-id
```
