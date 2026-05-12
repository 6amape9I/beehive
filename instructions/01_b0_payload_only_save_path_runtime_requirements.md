# B0. Payload-only n8n execution и save_path routing

## 0. Роль этапа

B0 — первый технический этап интеграции Beehive с реальными n8n workflow.

Цель B0 — убрать несовпадение контрактов между программой и workflow.

После B0 Beehive должен:

1. Отправлять в n8n только `payload` исходного файла.
2. Принимать response forms от n8n, включая массив business objects.
3. Сохранять output items по `save_path`.
4. Сохранять runtime metadata внутри программы, не передавая её n8n.
5. Иметь tests, которые подтверждают работу на path-модели, безопасной для Windows и Ubuntu.

## 1. Что нужно прочитать перед началом

Обязательно прочитать:

```text
README.md
src-tauri/src/executor/mod.rs
src-tauri/src/file_ops/mod.rs
src-tauri/src/discovery/mod.rs
src-tauri/src/config/mod.rs
src-tauri/src/domain/mod.rs
src-tauri/src/database/mod.rs
demo/workdir/pipeline.yaml
```

Также прочитать эти инструкции:

```text
instructions/00_beehive_n8n_global_vision.md
instructions/01_b0_payload_only_save_path_runtime_requirements.md
instructions/02_b0_agent_bootstrap.md
```

Если имена файлов отличаются после копирования, найти их по смыслу.

## 2. Обязательный план перед кодом

До изменения кода создать:

```text
docs/beehive_n8n_b0_plan.md
```

План должен содержать:

```text
1. Что понял из задачи.
2. Какие текущие места кода отвечают за HTTP request.
3. Где сейчас строится Beehive metadata wrapper.
4. Где сейчас response превращается в next-stage files.
5. Как будет реализован payload-only request.
6. Как будет реализован save_path routing.
7. Как будет обеспечена filesystem safety.
8. Как будет обеспечена Windows/Ubuntu совместимость.
9. Какие tests будут добавлены/изменены.
10. Какие команды проверки будут запускаться.
11. Что точно не входит в B0.
12. Риски и спорные места.
13. Чеклист выполнения.
```

Не начинать писать код до создания плана.

## 3. Перечитывание ТЗ

Во время работы обязательно перечитывать эти требования.

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

В feedback добавить строку:

```text
ТЗ перечитано на этапах: after_plan, after_request_contract_change, after_response_contract_change, after_save_path_routing, after_filesystem_safety, after_tests, before_feedback
```

## 4. Изменение request contract

Сейчас runtime строит request object с техническими полями Beehive.

Нужно изменить контракт:

```text
POST body = parsed source_file.payload_json
```

Требования:

- n8n получает ровно полезную нагрузку из `payload`;
- Beehive metadata не отправляется;
- `Content-Type: application/json` и `Accept: application/json` остаются;
- `stage_runs.request_json` должен хранить фактически отправленный JSON, то есть payload-only body;
- runtime metadata продолжает храниться через существующие DB columns: `run_id`, `entity_id`, `stage_id`, `entity_file_id`, `attempt_no`, timestamps, status fields;
- source JSON на диске не мутируется при execution.

Ожидаемый пример:

source file:

```json
{
  "id": "raw-001",
  "current_stage": "raw",
  "status": "pending",
  "payload": {
    "raw_text": "Туристы подошли к воротам...",
    "source_name": "book_fantasy_01"
  },
  "meta": {
    "operator_note": "manual test"
  }
}
```

HTTP body to n8n:

```json
{
  "raw_text": "Туристы подошли к воротам...",
  "source_name": "book_fantasy_01"
}
```

Forbidden in HTTP body:

```text
entity_id
stage_id
entity_file_id
attempt
run_id
meta.beehive
```

## 5. Response contract

Beehive должен принимать следующие response forms.

### 5.1 Preferred: array of business objects

```json
[
  {
    "entity_name": "замок",
    "save_path": "main_dir/processed/raw_entities"
  },
  {
    "target_entity_name": "мобила",
    "save_path": "main_dir/processed/raw_representations"
  }
]
```

### 5.2 Existing wrapper object

```json
{
  "success": true,
  "payload": [
    {
      "entity_name": "замок",
      "save_path": "main_dir/processed/raw_entities"
    }
  ],
  "meta": {}
}
```

Existing wrapper support must not be removed.

### 5.3 Single direct business object

```json
{
  "entity_name": "замок",
  "save_path": "main_dir/processed/raw_entities"
}
```

Это нужно поддержать, чтобы simple workflow мог вернуть один объект без wrapper.

### 5.4 Error response

Если wrapper содержит:

```json
{ "success": false }
```

это execution failure/contract failure, как сейчас.

### 5.5 No silent loss

Если response содержит output objects, но Beehive не может определить route для этих objects, нельзя молча завершать stage как success without output.

Правило B0:

```text
output item has save_path → route by save_path
output item has no save_path and source stage has next_stage → fallback to next_stage
output item has no save_path and no next_stage → blocked/contract error
terminal stage returns no output → success without target files
```

## 6. save_path routing

Реализовать routing для каждого output item.

Минимальное правило:

```text
save_path должен совпасть с input_folder активного stage после нормализации.
```

Примеры:

```text
save_path: main_dir/processed/raw_entities
stage.input_folder: main_dir/processed/raw_entities
→ ok

save_path: /main_dir/processed/raw_entities
stage.input_folder: main_dir/processed/raw_entities
→ ok, legacy logical path

save_path: stages/raw_entities
stage.input_folder: stages/raw_entities
→ ok

save_path: ../outside
→ reject

save_path: /etc/passwd
→ reject

save_path: C:\Users\me\Desktop
→ reject
```

Если route найден:

```text
target_stage_id = matched active stage.id
target folder = workdir / target_stage.input_folder
target artifact is registered for target_stage_id
target entity_stage_state status = pending
```

Если route не найден:

```text
do not write target file
finish stage run as failure or blocked with clear error_type
source stage state becomes blocked
app_events records route problem
```

## 7. Filesystem safety

Создать или выделить небольшую функцию/module для безопасной нормализации `save_path`.

Рекомендованное имя:

```text
src-tauri/src/save_path.rs
```

или другое имя, если лучше ложится в архитектуру.

Требования к функции:

```text
input: raw save_path string, workdir path, active stages
output: matched StageRecord or safe route error
```

Функция должна:

- trim whitespace;
- normalize separators for comparison;
- allow relative paths only;
- allow legacy `/main_dir/...` by converting to `main_dir/...`;
- reject other leading `/`;
- reject `..` components;
- reject Windows drive prefixes;
- reject UNC paths;
- reject empty paths;
- match only active stage input_folder;
- never canonicalize a non-existing final file path in a way that breaks before directory creation;
- ensure final target directory is inside workdir.

Path comparison should be deterministic across Windows and Ubuntu. Do not rely on case-insensitive matching.

## 8. Target artifact format

Even though n8n sees payload-only data, files written by Beehive must remain Beehive artifacts:

```json
{
  "id": "generated-child-id",
  "current_stage": "target_stage_id",
  "next_stage": "target_stage.next_stage or null",
  "status": "pending",
  "payload": {
    "...": "n8n output object, including save_path if present"
  },
  "meta": {
    "beehive": {
      "copy_source_stage": "source_stage_id",
      "copy_target_stage": "target_stage_id",
      "copy_created_at": "...",
      "source_entity_id": "...",
      "stage_run_id": "..."
    }
  }
}
```

`meta.beehive` may stay inside Beehive artifacts because it is local program metadata. It must not be sent to n8n in later HTTP requests.

## 9. Multi-output and branching

B0 должен поддержать массив output objects, где разные items указывают разные `save_path`.

Example:

```json
[
  {
    "entity_name": "замок",
    "save_path": "main_dir/processed/raw_entities"
  },
  {
    "target_entity_name": "мобила",
    "save_path": "main_dir/processed/raw_representations"
  }
]
```

Acceptance:

```text
one source stage run creates two target files
target file A registered under stage raw_entities
target file B registered under stage raw_representations
source state becomes done
both target states become pending
stage_run records one success
app_event includes created_child_paths
```

If one output item has invalid route, B0 may choose all-or-nothing behavior. Preferred: all-or-nothing. Do not write partial outputs if any item is unsafe or unroutable.

## 10. Backward compatibility

Do not remove existing next_stage behavior.

If an n8n response returns objects without `save_path`, old behavior should still work through `next_stage`.

Existing tests around multi-output next-stage copy should be updated only where request body changed from wrapper to payload-only.

## 11. Tests

Add or update Rust tests. Use local mock HTTP servers only. Do not call real n8n.

Required tests:

### 11.1 Payload-only request

Create a source file with payload and meta. Run one mocked stage. Assert mock server request body equals payload only.

Assert request body does not contain:

```text
entity_id
stage_id
entity_file_id
attempt
run_id
beehive
```

### 11.2 Array response routed by save_path

Mock n8n returns array of two business objects with two different save_path values. Assert two target files are created in matching stage input folders.

### 11.3 Direct object response

Mock n8n returns one direct business object with save_path. Assert one target file is created.

### 11.4 Legacy `/main_dir/...` logical route

Stage input_folder:

```text
main_dir/processed/raw_entities
```

Mock output:

```text
/main_dir/processed/raw_entities
```

Assert route succeeds on the current OS.

### 11.5 Unsafe save_path rejected

Test at least:

```text
../outside
/etc/passwd
C:\Users\bad\file
\\server\share
empty string
```

Assert no file is written outside workdir and task becomes blocked or failed with clear error_type.

### 11.6 Fallback next_stage still works

Response without save_path but source stage has next_stage. Assert old behavior still creates target artifact.

### 11.7 No silent output loss

Response has output object without save_path and source stage has no next_stage. Assert blocked/contract failure, not success with zero target files.

## 12. Suggested files to modify

Likely files:

```text
src-tauri/src/executor/mod.rs
src-tauri/src/file_ops/mod.rs
src-tauri/src/domain/mod.rs
src-tauri/src/config/mod.rs only if truly needed
src-tauri/src/lib.rs if adding a new module
docs/n8n_contract.md
README.md if contract docs need a short update
```

Do not make broad UI changes in B0.

## 13. Ubuntu-specific constraints

The project must remain usable on Ubuntu.

Do not make verification depend on:

```text
npm.cmd
cmd.exe
PowerShell
Visual Studio Developer Prompt
Windows absolute paths
```

For Ubuntu, expected useful commands are:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
```

`tauri dev` / `tauri build` may need system GTK/WebKit dependencies on Ubuntu, so do not make GUI launch a hard acceptance blocker for B0.

Tests must be backend-oriented and runnable with Cargo.

## 14. Documentation

Add/update:

```text
docs/n8n_contract.md
```

It must explain:

```text
1. Beehive input artifact format.
2. Payload-only HTTP request.
3. Supported n8n response forms.
4. save_path routing.
5. Safe path rules.
6. Legacy /main_dir compatibility.
7. next_stage fallback.
8. What happens on blocked routes.
9. Manual smoke-test steps with a local/mock or real n8n URL.
```

## 15. Commands to run

At minimum:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
```

If `npm run build` cannot run because dependencies are missing, write the exact reason in feedback. Do not hide it.

Do not run real n8n in automated tests.

## 16. Acceptance criteria

B0 is accepted only if:

- `docs/beehive_n8n_b0_plan.md` exists before code work;
- Beehive sends payload-only HTTP body;
- Beehive no longer sends runtime metadata to n8n;
- `stage_runs.request_json` stores payload-only body;
- array response of business objects is supported;
- direct single business object response is supported;
- wrapper response remains supported;
- output item with `save_path` routes to active stage input_folder;
- multi-output items can route to different active stages;
- unsafe save_path values are rejected;
- legacy `/main_dir/...` logical route is handled safely or explicitly rejected with tests/docs;
- no output objects are silently lost;
- old next_stage fallback still works;
- source JSON file is not mutated during execution;
- target files are Beehive-wrapped artifacts;
- target stage states become pending;
- Rust tests pass;
- `docs/n8n_contract.md` exists;
- `docs/beehive_n8n_b0_feedback.md` exists and is complete.

## 17. Feedback после B0

Создать:

```text
docs/beehive_n8n_b0_feedback.md
```

Feedback должен содержать:

```text
1. Что сделано.
2. Какие файлы изменены.
3. Какие требования выполнены.
4. Как изменился n8n request contract.
5. Как работает save_path routing.
6. Как обрабатываются unsafe paths.
7. Как сохраняется backward compatibility через next_stage.
8. Какие tests добавлены/изменены.
9. Какие команды запускались.
10. Результаты cargo fmt.
11. Результаты cargo test.
12. Результаты npm run build.
13. Что не сделано.
14. Риски.
15. Что нужно передать следующему этапу.
16. ТЗ перечитано на этапах: ...
```

Обязательно отдельно указать:

```text
Главный output следующего этапа:
working Beehive runtime that can send payload-only data to n8n and save outputs by save_path.
```
