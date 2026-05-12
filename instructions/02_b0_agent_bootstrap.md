# Инициализационная инструкция для Codex-агента: Beehive n8n Runtime Engineer B0

## 0. Кто ты

Ты Codex-агент в роли Beehive n8n Runtime Engineer.

Ты работаешь над проектом:

```text
6amape9I/beehive
```

Твоя задача — выполнить этап:

```text
B0. Payload-only n8n execution и save_path routing
```

Ты не архитектор проекта. Архитектурное решение уже принято:

```text
Beehive отправляет в n8n только полезную нагрузку.
Beehive metadata остаётся внутри программы.
n8n output может ветвиться через save_path.
```

Твоя задача — аккуратно реализовать это решение в коде, tests и документации.

## 1. Что уже есть

В Beehive уже есть:

```text
Tauri v2 + React UI
Rust backend
SQLite runtime state
pipeline.yaml
stage discovery/reconciliation
entity_files
entity_stage_states
stage_runs
run_due_tasks
run_entity_stage
retry/stuck handling
next_stage copy
multi-output support через next_stage
```

Ты должен использовать существующее ядро, а не переписывать проект.

## 2. Что нужно прочитать перед началом

Сначала прочитай:

```text
README.md
instructions/00_beehive_n8n_global_vision.md
instructions/01_b0_payload_only_save_path_runtime_requirements.md
instructions/02_b0_agent_bootstrap.md
```

Затем прочитай код:

```text
src-tauri/src/executor/mod.rs
src-tauri/src/file_ops/mod.rs
src-tauri/src/discovery/mod.rs
src-tauri/src/config/mod.rs
src-tauri/src/domain/mod.rs
src-tauri/src/database/mod.rs
src-tauri/src/lib.rs
```

Также посмотри:

```text
demo/workdir/pipeline.yaml
docs/
```

Если какого-то файла нет, запиши это в план и продолжи, если задача всё ещё выполнима.

## 3. Как ты должен работать

Ты обязан начать с плана.

Создай:

```text
docs/beehive_n8n_b0_plan.md
```

Не пиши код до создания плана.

В плане укажи:

```text
1. Что понял.
2. Какие места кода отвечают за текущий request/response contract.
3. Какие изменения нужны для payload-only.
4. Какие изменения нужны для save_path routing.
5. Как будет устроена safety-проверка путей.
6. Какие tests добавишь.
7. Как проверишь Windows/Ubuntu совместимость.
8. Какие команды запустишь.
9. Что не входит в B0.
10. Риски.
11. Чеклист.
```

## 4. Перечитывание требований

Ты обязан перечитывать ТЗ во время работы.

Минимальные checkpoints:

```text
after_plan
after_request_contract_change
after_response_contract_change
after_save_path_routing
after_filesystem_safety
after_tests
before_feedback
```

В feedback дословно укажи:

```text
ТЗ перечитано на этапах: after_plan, after_request_contract_change, after_response_contract_change, after_save_path_routing, after_filesystem_safety, after_tests, before_feedback
```

Если контекст был очищен или ты не уверен, не продолжай с середины. Сначала перечитай инструкции, план и текущий код.

## 5. Главная задача

Реализуй B0 так, чтобы Beehive мог выполнить такой сценарий:

```text
1. operator places Beehive-wrapped JSON in source stage folder;
2. Scan workspace registers it as pending;
3. Run due tasks sends only payload to n8n webhook;
4. n8n returns one or more business objects;
5. each output object is saved by save_path into matched active stage input folder;
6. source stage state becomes done;
7. target stage states become pending;
8. stage_runs/app_events contain audit information;
9. source JSON file is not mutated.
```

## 6. Что запрещено делать

На B0 запрещено:

```text
делать UI redesign;
добавлять background daemon;
добавлять n8n REST API workflow manager;
добавлять credential manager;
вызывать реальные n8n URL в tests;
вызывать web search;
переписывать database schema без необходимости;
удалять существующие runtime features;
ломать next_stage fallback;
делать filesystem writes outside workdir;
делать Windows-only или Ubuntu-only path logic;
завязывать acceptance на Tauri GUI launch.
```

## 7. Техническое ядро B0

### 7.1 Payload-only request

Измени место, где создаётся request JSON.

Теперь Beehive должен отправлять:

```text
serde_json::from_str(source_file.payload_json)
```

а не wrapper с Beehive metadata.

Runtime metadata должна остаться в DB и stage_run records.

### 7.2 Response handling

Поддержи:

```text
array of objects
wrapper object with success/payload/meta
single direct business object
```

Не теряй outputs молча.

### 7.3 save_path route

Если output item содержит `save_path`, он должен определить target stage.

`save_path` должен совпасть с active stage input_folder после безопасной нормализации.

Если route invalid, unsafe или unknown — не писать файл.

### 7.4 Target file creation

Target file должен быть Beehive artifact.

Payload target file = n8n output object.

Metadata target file can include Beehive-local trace. Но это metadata не должна отправляться в n8n при следующем execution, потому что request теперь payload-only.

## 8. Tests обязательны

Добавь/обнови Rust tests.

Минимум:

```text
payload_only_request_body
save_path_routes_array_outputs_to_multiple_stages
save_path_routes_direct_object_response
legacy_main_dir_save_path_is_logical_not_os_absolute
unsafe_save_path_is_rejected
next_stage_fallback_still_works
output_without_route_is_not_silently_lost
```

Названия tests могут отличаться, но смысл должен покрываться.

Tests должны использовать tempdir и local mock HTTP server.

Tests не должны зависеть от n8n-dev.steos.io.

## 9. Cross-platform требования

При реализации path logic помни:

```text
на Ubuntu строка /main_dir/... выглядит как абсолютный путь;
на Windows строка C:\... выглядит как абсолютный путь;
в n8n save_path может содержать slash даже если Beehive запущен на Windows;
stage.input_folder в YAML должен быть относительным логическим путём;
сравнение должно быть нормализованным, но не небезопасным.
```

Do not use platform-specific shell commands in tests.

Для проверки:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
```

На Windows оператор может использовать `npm.cmd`, но в коде и feedback не делай его единственным вариантом.

## 10. Documentation

Создай или обнови:

```text
docs/n8n_contract.md
```

Документ должен быть понятен оператору и следующему разработчику.

Он должен показать:

```text
Beehive input artifact
payload-only request body
n8n response examples
save_path route examples
unsafe save_path examples
next_stage fallback
manual smoke test flow
```

## 11. Feedback после работы

Создай:

```text
docs/beehive_n8n_b0_feedback.md
```

Feedback должен быть честным.

Обязательно включи:

```text
1. Что сделано.
2. Какие файлы изменены.
3. Какие tests добавлены.
4. Какие команды запускались.
5. Результаты команд.
6. Что не удалось проверить.
7. Какие риски остались.
8. Какие решения были приняты в реализации.
9. Что передать следующему этапу.
10. ТЗ перечитано на этапах: ...
```

Если какая-то команда не прошла, не скрывай. Укажи ошибку и причину.

## 12. Acceptance self-check

Перед feedback проверь:

```text
[ ] request body payload-only
[ ] no Beehive metadata sent to n8n
[ ] stage_runs still record audit
[ ] array response works
[ ] direct object response works
[ ] wrapper response works
[ ] save_path route works
[ ] multi-route output works
[ ] unsafe save_path rejected
[ ] next_stage fallback works
[ ] source JSON not mutated
[ ] target JSON Beehive-wrapped
[ ] cargo fmt done
[ ] cargo test done
[ ] npm run build attempted
[ ] docs/n8n_contract.md created
[ ] feedback created
```

## 13. Главное напоминание

Цель B0 — не сделать весь Beehive идеальным.

Цель B0 — сделать возможным реальный сквозной n8n pipeline:

```text
payload-only request → n8n business workflow → save_path-routed outputs → next Beehive stages
```

Не расширяй задачу без необходимости.
