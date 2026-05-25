# 01. Правила работы Codex-агента над Beehive

## 0. Роль агента

Ты Codex-агент, работающий над проектом Beehive.

Beehive — web/control-plane приложение для операторов, которые не программируют. Beehive управляет S3 artifacts, stages, n8n workflow executions, retries, blocked states, lineage и видимостью результата.

n8n — data-plane. Он читает S3, выполняет workflow, пишет outputs в S3 и возвращает manifest.

Не превращай Beehive в ещё один n8n и не тащи production workflow в репозиторий.

## 1. Как работать с инструкциями

Перед началом любого этапа прочитай актуальную инструкцию этапа полностью.

На B11 обязательно прочитать:

```text
instructions/00_beehive_worker_pools_global_vision.md
instructions/01_codex_agent_working_rules.md
instructions/02_b11_resource_classes_worker_pool_config_requirements.md
```

Если в репозитории есть предыдущие feedback-файлы, прочитай их тоже:

```text
docs/beehive_s3_b9_entities_upload_simplified_crud_feedback.md
docs/beehive_s3_b10_runtime_contract_hardening_feedback.md
```

Если каких-то файлов нет, честно напиши это в плане и продолжай по доступному коду.

## 2. План до кода

До изменения кода создай plan-файл:

```text
docs/beehive_s3_b11_resource_classes_worker_pools_plan.md
```

В плане укажи:

```text
1. Что понял из задачи.
2. Какие файлы прочитал.
3. Где сейчас описан StageDefinition / pipeline config.
4. Где сейчас создаются/редактируются stages.
5. Где сейчас запускается selected run / waves.
6. Как добавишь resource_class.
7. Как добавишь worker_pools config.
8. Как сохранишь backward compatibility.
9. Как UI покажет “Использует локальную LLM”.
10. Какие tests добавишь.
11. Какие команды запустишь.
12. Что не будешь делать в B11.
13. Риски.
```

Не пиши runtime/feature code до создания плана.

## 3. Перечитывание ТЗ

Ты обязан периодически перечитывать инструкцию, по которой составил план.

Для B11 checkpoints:

```text
after_plan
after_domain_config_design
after_backend_changes
after_ui_changes
after_tests
before_feedback
```

В feedback добавь строку:

```text
ТЗ перечитано на этапах: after_plan, after_domain_config_design, after_backend_changes, after_ui_changes, after_tests, before_feedback
```

## 4. Не ломать доказанное ядро

Beehive уже умеет:

```text
workspace/stage/entity CRUD;
S3 upload/import;
selected pipeline waves;
JSON-body n8n control envelope;
manifest validation;
save_path routing;
partial output handling/cardinality, если B10 уже реализован;
web server/browser flow.
```

Не ломай эти возможности ради B11.

B11 — это подготовка worker-track, а не переписывание runtime.

## 5. Малые изменения лучше больших

Если задачу можно решить небольшим добавлением enum/config/UI checkbox — сделай так.

Не начинай B11 с:

```text
RabbitMQ;
Kafka;
Postgres migration;
нового scheduler;
реальных background workers;
огромного рефакторинга frontend;
переписывания executor;
удаления legacy local mode.
```

Эти вещи не входят в B11.

## 6. Backward compatibility

Старые pipeline.yaml должны продолжать загружаться.

Если поле `resource_class` отсутствует, stage считается:

```text
resource_class = default
```

Если `runtime.worker_pools` отсутствует, использовать default config:

```yaml
worker_pools:
  default:
    concurrency: 1
  local_llm:
    concurrency: 1
```

Не требуй от старых workspaces ручного редактирования YAML.

## 7. UI для оператора

Оператору не нужен термин “resource class”.

В Stage Editor показывать простое поле:

```text
[ ] Использует локальную LLM
```

Подсказка:

```text
Если включено, stage будет выполняться пулом local_llm с отдельным лимитом параллелизма.
```

Не добавляй перегруженную настройку worker pools в основной UI B11. Можно показать read-only текущий resource class в Advanced/Diagnostics.

## 8. Документация и feedback

После работы создай:

```text
docs/beehive_s3_b11_resource_classes_worker_pools_feedback.md
```

Feedback должен содержать:

```text
1. Что сделано.
2. Какие файлы изменены.
3. Как выглядит stage.resource_class.
4. Как выглядит runtime.worker_pools.
5. Как старые configs мигрируют/читаются.
6. Что изменилось в UI.
7. Какие tests добавлены.
8. Какие команды запускались и результаты.
9. Что не реализовано в B11.
10. Риски для B12.
11. Checkpoints перечитывания ТЗ.
```

## 9. Тестовая дисциплина

Запусти и честно зафиксируй:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
rg "@tauri-apps/api/core|invoke\(" src -n
git diff --check
```

Если команда не запускается — не делай вид, что прошла. Напиши причину.

## 10. Стиль работы

- Не скрывай проблемы.
- Не делай production claims без smoke/test evidence.
- Не добавляй большие зависимости без причины.
- Не коммить секреты.
- Не коммить production n8n workflow.
- Не меняй бизнес-смысл существующих n8n stages.
- Если видишь старый мусорный код, не удаляй его случайно. Запиши cleanup candidate в feedback.

## 11. Что считать хорошим результатом B11

Хороший B11:

```text
маленький;
безопасный;
не ломает текущий web/operator flow;
добавляет resource_class;
добавляет worker_pools config;
готовит B12 leases/workers;
имеет tests и feedback.
```

Плохой B11:

```text
переписывает executor;
тащит RabbitMQ/Kafka;
ломает stage CRUD/upload;
добавляет background workers без lease модели;
засоряет UI новыми сложными настройками.
```
