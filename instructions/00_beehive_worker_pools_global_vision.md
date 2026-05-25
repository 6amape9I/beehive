# 00. Глобальное видение Beehive Worker Pools

## 0. Контекст

Beehive уже доказал, что может быть control-plane над S3+n8n pipeline:

```text
операторский web UI
→ Beehive control-plane
→ S3 artifacts
→ n8n workflow
→ S3 outputs
→ Beehive manifest/state/lineage
```

Текущая сильная сторона проекта: пользователь может создавать workspaces, stages, загружать сущности, запускать выбранные artifacts и видеть результаты. n8n остаётся data-plane: читает S3, вызывает LLM/API/Postgres, пишет outputs в S3 и возвращает manifest.

Следующая большая задача — перейти от ручных синхронных запусков к управляемой параллельной обработке большого объёма данных. Целевая нагрузка — десятки тысяч документов и последующее разбиение части входов на ещё большее число artifacts.

## 1. Почему нужен worker layer

Синхронные selected waves полезны для ручной проверки и пилотов, но недостаточны для production-like обработки 22 000 документов.

Без worker layer появляются проблемы:

```text
оператор должен вручную запускать волны;
нет общего контроля in-flight задач;
дорогие локальные LLM могут получить больше запросов, чем выдерживают;
retry/block recovery слишком ручной;
сложно видеть backlog и throughput;
сложно безопасно восстанавливать задачи после падения процесса;
сложно ограничивать разные типы нагрузки разными лимитами.
```

Beehive должен стать владельцем параллелизма. n8n исполняет workflow, но не выбирает source artifacts и не управляет глобальной очередью.

## 2. Основное архитектурное решение

На первом этапе не используем Kafka и не используем RabbitMQ.

Делаем встроенные DB-backed worker pools внутри Beehive:

```text
Beehive DB/state machine
+ resource_class у stage
+ worker pool config
+ claim/lease/heartbeat
+ worker loops внутри beehive-server
```

Это даёт контроль параллелизма без немедленного усложнения инфраструктуры.

Future path:

```text
B11-B14: internal DB-backed worker pools
после реального load test: решение, нужен ли RabbitMQ
Kafka: не использовать для этой задачи сейчас
```

## 3. Resource class вместо двух жёстких типов воркеров

В UI можно начать с простого чекбокса:

```text
[ ] Использует локальную LLM
```

Но в backend хранить не boolean, а resource class:

```text
default
local_llm
```

Так мы не запираем архитектуру в два типа воркеров. Позже можно добавить:

```text
gemini_proxy
graph_write
heavy_cpu
low_priority
```

UI mapping на B11:

```text
unchecked “Использует локальную LLM” → resource_class = default
checked “Использует локальную LLM” → resource_class = local_llm
```

## 4. Worker pools

Параллелизм контролируется не n8n, а Beehive.

Пример целевой конфигурации:

```yaml
runtime:
  worker_pools:
    default:
      concurrency: 10
    local_llm:
      concurrency: 1
```

Это означает:

```text
одновременно можно выполнять до 10 обычных задач;
одновременно можно выполнять только 1 задачу, которая использует локальную LLM.
```

Так мы защищаем локальную модель от перегруза. Если `local_llm.concurrency = 1`, Beehive не отправит в n8n больше одной задачи такого класса одновременно.

## 5. Ограничение гарантии

Beehive может гарантировать только количество workflow executions, которые он сам запускает.

Если внутри одного n8n workflow есть несколько параллельных вызовов локальной модели, Beehive видит это как одну задачу, но локальный LLM-server может получить несколько запросов. Поэтому правило workflow authoring:

```text
Stage с resource_class=local_llm не должен параллелить локальные LLM calls внутри одного n8n execution.
```

На текущих пайплайнах такого нет и не планируется, поэтому это принимается как рабочее ограничение.

## 6. Что делаем с artifact_id

Пользователь и n8n-автор не должны проектировать сложную idempotency-схему.

Практическая модель для n8n:

```text
entity_id + save_path = logical output route
```

Для пользователя важны:

```text
entity_id
entity_name
save_path
S3 key
```

`artifact_id` остаётся техническим internal identifier. Его не надо удалять из базы прямо сейчас, но его нужно демотировать из обязательного смыслового поля n8n-контракта.

Целевая логика:

```text
если n8n прислал artifact_id → Beehive использует его;
если n8n не прислал artifact_id → Beehive сам выводит artifact_id из bucket/key или stable hash.
```

Это снижает сложность для n8n-автора и не ломает существующую БД.

## 7. Lease и heartbeat

Worker не должен просто “забрать задачу”. Он должен получить lease.

Целевая модель:

```text
claim task → lease_until
heartbeat продлевает lease
success/failure release lease
если worker умер → lease истёк → задача восстанавливается
```

Это защищает от ситуации, когда задача навсегда зависла в running, потому что процесс умер.

## 8. Retry policy

Retry должен зависеть от типа ошибки.

Пример:

```text
timeout/network/HTTP 5xx → retry with backoff
local_llm overload/429/ECONNRESET → retry with backoff
manifest_invalid/schema mismatch → blocked
save_path unknown/unsafe → blocked
output cardinality violation → blocked
artifact registration conflict → warning/blocked, не retry loop
operator manual retry → allowed from failed/blocked
```

Нельзя бездумно retry’ить дорогие LLM-задачи, если ошибка детерминированная и retry ничего не исправит.

## 9. Manual recovery

Оператор должен уметь руками вернуть failed/blocked задачу в retry/pending без пересборки и перезаливки.

Минимальные действия:

```text
Retry this failed/blocked item
Retry selected failed/blocked
Release stuck lease
```

С подтверждением и понятным сообщением.

## 10. Backpressure UI

Оператору нужно видеть очередь и нагрузку.

Минимально:

```text
Default pool:
  pending
  running
  retry_wait
  failed
  blocked
  active workers / limit

Local LLM pool:
  pending
  running
  retry_wait
  failed
  blocked
  active workers / limit
```

Потом можно добавить:

```text
avg duration
oldest pending age
throughput per hour
last error
stuck leases
```

## 11. Workspace isolation вместо приоритетов

Пока приоритеты не нужны.

Тесты и production должны жить в разных workspaces. Worker pools могут быть включены/выключены на workspace.

Будущая настройка:

```yaml
workspace_runtime:
  workers_enabled: true
  max_in_flight: 20
```

Priority queues вернём позже, если появится реальная потребность смешивать urgent и background jobs в одном workspace.

## 12. RabbitMQ и Kafka

### RabbitMQ

RabbitMQ хорошо подходит под будущую внешнюю work queue:

```text
ack/nack/requeue;
prefetch;
dead-letter queues;
separate queues by resource_class;
work queue semantics.
```

Если DB-backed queue станет узким местом, RabbitMQ — первый кандидат.

### Kafka

Kafka сейчас не нужна.

Kafka хороша как event log и streaming platform, но для текущей задачи “одна задача → один worker → ack/retry/dead-letter” она сложнее и менее естественна.

## 13. Roadmap worker-track

### B11. Resource Classes and Worker Pool Configuration

```text
stage.resource_class
UI checkbox “Использует локальную LLM”
runtime.worker_pools config
API/types/docs/tests
no actual background worker yet
```

### B12. DB-backed leases and internal worker pools

```text
lease table or lease fields
claim/release/heartbeat
worker loops inside beehive-server
no double-claim tests
```

### B13. Queue UI and manual retry controls

```text
Workers/Queue page
backpressure metrics
pause/resume pools
manual retry failed/blocked
release stuck lease
```

### B14. Retry policy and medium batch pilot

```text
error classification
backoff
blocked vs retry
pilot 100–500 docs
throughput report
```

### B15. Broker decision point

```text
evaluate DB queue under real load
decide: keep DB queue, migrate to Postgres, or add RabbitMQ
```

## 14. Current priority

Следующий этап — B11.

B11 должен быть маленьким и безопасным. Он не должен добавлять real background workers. Он должен добавить правильную модель resource_class и worker_pools, чтобы B12 мог строить leases/workers поверх стабильного контракта.
