# beehive — Stage 4 Codex Task

# Этап 4. Интеграция с n8n и retry-механика

## 0. Статус проекта перед началом этапа

Проект: **beehive**

Текущий статус:
- Stage 1 закрыт: есть Tauri + React + TypeScript foundation, workdir bootstrap, YAML config loading, SQLite bootstrap.
- Stage 2 закрыт: есть runtime foundation, schema v2, discovery JSON-файлов, registration entities, active/inactive stages, strengthened workdir validation.
- Stage 3 закрыт: есть schema v3, `entity_files`, reconciliation scanner, file instance model, missing/restored tracking, managed copy to next stage, checksum consistency fix.

Stage 4 должен строиться поверх уже существующей архитектуры. Не переписывай фундамент без необходимости.

---

## 1. Новое подтверждённое внешнее условие

Для интеграции с n8n используется заранее поднятый n8n instance:

```text
https://n8n-dev.steos.io/
```

Пользователь уже собрал простой n8n workflow и проверил production webhook вручную:

```text
https://n8n-dev.steos.io/webhook/b0c81347-5f51-4142-b1d9-18451d8c4ecf
```

Ручной запрос к этому endpoint уже возвращает корректный ответ.

Важно:

- **Не хардкодь этот URL в коде приложения.**
- `workflow_url` должен приходить из `pipeline.yaml`.
- Этот URL можно использовать только как пример в README/docs/test fixture description, но не как обязательную runtime-константу.
- Автоматические тесты не должны ходить в реальный `https://n8n-dev.steos.io/`; используй mock HTTP server.

---

## 2. Правило тестирования для этого этапа

Codex не должен тратить лимиты на ручной mouse-driven UI walkthrough: проверять нужно техническое ядро обычными automated tests, а UI — только smoke-level запуском/сборкой.

---

## 3. Главная цель Stage 4

Реализовать первый полноценный runtime execution layer:

```text
eligible entity stage state
    -> queued
    -> in_progress
    -> POST to n8n webhook
    -> stage_runs audit record
    -> success: done + managed next-stage copy from n8n response
    -> failure: retry_wait or failed
```

Stage 4 должен добавить:

1. HTTP-интеграцию с n8n webhook.
2. Runtime executor для due/pending задач.
3. Retry-механику.
4. Запись `stage_runs`.
5. Обновление `entity_stage_states`.
6. Создание next-stage copy на основе ответа n8n.
7. Минимальное UI-представление runtime execution state.
8. Тесты на success/failure/retry/contract handling.

---

## 4. Что Stage 4 НЕ должен делать

Не реализовывать:

- полноценный background daemon;
- сложный scheduler с постоянным автозапуском;
- visual workflow builder;
- n8n workflow management через n8n REST API;
- создание/редактирование n8n workflows из beehive;
- credential manager;
- authentication UI;
- complex secret storage;
- advanced UI/UX polish;
- массовую параллельную обработку с нестабильной concurrency;
- pixel-perfect UI;
- ручное UI-тестирование через скриншоты;
- полную observability систему.

Stage 4 — это **execution foundation**, не финальный production runner.

---

## 5. Утверждённый подход к n8n

### 5.1. Trigger

Для beehive используется **n8n Webhook Trigger**.

Каждый executable stage в `pipeline.yaml` должен иметь `workflow_url`.

Пример:

```yaml
stages:
  - id: transform_title
    input_folder: stages/incoming
    output_folder: stages/transformed
    workflow_url: https://n8n-dev.steos.io/webhook/b0c81347-5f51-4142-b1d9-18451d8c4ecf
    max_attempts: 3
    retry_delay_sec: 10
    next_stage: review
```

### 5.2. Test URL vs Production URL

Для beehive runtime следует использовать production URL:

```text
/webhook/...
```

а не test URL:

```text
/webhook-test/...
```

Test URL можно использовать только для ручной отладки в n8n UI.

---

## 6. Request contract: beehive -> n8n

Для Stage 4 зафиксировать следующий HTTP request contract.

Method:

```text
POST
```

Headers:

```text
Content-Type: application/json
Accept: application/json
```

Body:

```json
{
  "entity_id": "entity-demo-001",
  "stage_id": "transform_title",
  "entity_file_id": 123,
  "source_file_path": "C:/workdir/stages/incoming/entity-demo-001.json",
  "attempt": 1,
  "run_id": "uuid-or-stable-run-id",
  "payload": {
    "title": "hello beehive"
  },
  "meta": {
    "source": "manual",
    "beehive": {
      "app": "beehive",
      "stage_id": "transform_title",
      "entity_file_id": 123,
      "attempt": 1,
      "run_id": "uuid-or-stable-run-id"
    }
  }
}
```

Rules:

- `entity_id` comes from the logical entity.
- `stage_id` is the current stage being executed.
- `entity_file_id` identifies the physical source file instance.
- `source_file_path` is the absolute path of the physical source file.
- `attempt` is the current attempt number.
- `run_id` uniquely identifies this stage run attempt.
- `payload` is copied from the source JSON's `payload`.
- `meta` is copied from the source JSON's `meta`, with a `meta.beehive` block added/updated.

Do not send internal DB-only structures unless explicitly needed.

---

## 7. Response contract: n8n -> beehive

For Stage 4, n8n response should be a JSON object:

```json
{
  "success": true,
  "entity_id": "entity-demo-001",
  "stage_id": "transform_title",
  "payload": {
    "title": "hello beehive",
    "title_processed": "HELLO BEEHIVE"
  },
  "meta": {
    "n8n": {
      "workflow": "beehive-demo-transform"
    }
  }
}
```

### 7.1. Stage 4 success rule

Treat a run as successful only if:

1. HTTP status is `2xx`;
2. response body is valid JSON;
3. response JSON is an object;
4. `success` is not `false`;
5. `payload` exists and is a JSON object if the current stage has a `next_stage`.

If current stage has no `next_stage`, the response may omit `payload`, but it is still recommended to return one.

### 7.2. Failure cases

Treat as failed attempt if:

- network error;
- DNS/connect error;
- TLS error;
- timeout;
- HTTP non-2xx;
- invalid JSON response;
- JSON response is not an object;
- response has `"success": false`;
- current stage requires next-stage copy but response has no object `payload`;
- next-stage copy fails;
- DB state transition fails.

---

## 8. Runtime state machine

Use the approved status model:

```text
pending
queued
in_progress
retry_wait
done
failed
blocked
skipped
```

### 8.1. Normal execution path

```text
pending -> queued -> in_progress -> done
```

### 8.2. Retry path

```text
pending -> queued -> in_progress -> retry_wait
retry_wait -> queued -> in_progress
retry_wait -> failed
```

### 8.3. Blocked path

Use `blocked` when execution cannot proceed because of a structural/configuration problem, for example:

- stage references missing/inactive next stage;
- stage has no `workflow_url` but is selected for execution;
- source file instance is missing and cannot be restored;
- required source JSON cannot be read.

Do not use `blocked` as normal terminal success. If a stage has no `next_stage` and execution succeeds, mark it `done`.

### 8.4. Manual reset

Stage 4 may implement a minimal reset command if already easy:

```text
failed -> pending
blocked -> pending
retry_wait -> pending
```

But this is optional. Do not build a full operations UI for it in Stage 4.

---

## 9. Execution selection rules

A task is eligible for execution if:

- the stage is active;
- the entity stage state status is `pending`; or
- status is `retry_wait` and `next_retry_at <= now`;
- the source `entity_file` is not missing;
- the source stage has a non-empty `workflow_url`;
- attempts are less than `max_attempts`.

Do not execute:

- `done`;
- `failed`;
- `blocked`;
- `skipped`;
- `in_progress`, unless it is reconciled as stuck first;
- `retry_wait` with future `next_retry_at`.

---

## 10. Runner model for Stage 4

Do **not** implement a permanent background daemon yet.

Implement deterministic commands/services:

### 10.1. `run_due_tasks`

Runs a bounded batch of currently eligible tasks.

Behavior:

1. Reconcile stuck `in_progress` tasks.
2. Promote eligible `pending` / due `retry_wait` states to `queued`.
3. Claim up to configured limit.
4. Execute claimed tasks.
5. Return a summary.

Suggested return:

```json
{
  "claimed": 2,
  "succeeded": 1,
  "retry_scheduled": 1,
  "failed": 0,
  "blocked": 0,
  "skipped": 0,
  "errors": []
}
```

### 10.2. `run_entity_stage`

Optional but useful. Runs one specific entity/stage or entity_file/stage for debugging.

Do not overbuild.

### 10.3. Sequential vs parallel

For Stage 4, safe sequential execution is acceptable and preferred over risky concurrency.

If using `runtime.max_parallel_tasks`, it may mean:

```text
maximum tasks processed per run_due_tasks call
```

rather than true parallel threads.

True parallel execution can be added later only after task claiming is robust.

---

## 11. Retry mechanics

Use stage-level config:

```yaml
max_attempts: 3
retry_delay_sec: 10
```

If `max_attempts` is missing, use a safe default from config validation or existing model.

### 11.1. Attempt numbering

Before sending request:

- `attempt_no = current_attempts + 1`
- increment attempts or store attempt number consistently
- create a `stage_runs` row for this attempt

### 11.2. On failure

If:

```text
attempt_no < max_attempts
```

then:

```text
status = retry_wait
next_retry_at = now + retry_delay_sec
last_error = ...
last_http_status = ...
```

If:

```text
attempt_no >= max_attempts
```

then:

```text
status = failed
next_retry_at = null
last_error = ...
last_http_status = ...
```

### 11.3. On success

```text
status = done
next_retry_at = null
last_error = null
last_http_status = http status
last_finished_at = now
```

Then create target next-stage file/state if `next_stage` exists.

---

## 12. Stuck task reconciliation

Use existing runtime config:

```yaml
runtime:
  stuck_task_timeout_sec: 900
```

Before running a batch, detect `in_progress` states where:

```text
last_started_at < now - stuck_task_timeout_sec
```

Then:

- if attempts < max_attempts: move to `retry_wait`;
- else: move to `failed`.

Record `app_events` for reconciliation.

This protects the app after crashes.

---

## 13. SQLite schema changes

Inspect existing schema v3 first. Do not duplicate existing columns.

Stage 4 likely requires schema v4.

### 13.1. Required state fields

Ensure `entity_stage_states` can store:

- `status`
- `attempts`
- `max_attempts`
- `last_error`
- `last_http_status`
- `next_retry_at`
- `last_started_at`
- `last_finished_at`
- `updated_at`
- source file reference if needed

Some may already exist. Add only what is missing.

### 13.2. Required `stage_runs` fields

Ensure `stage_runs` can store:

- `id`
- `run_id`
- `entity_id`
- `entity_file_id`
- `stage_id`
- `attempt_no`
- `workflow_url`
- `request_json`
- `response_json`
- `http_status`
- `success`
- `error_type`
- `error_message`
- `started_at`
- `finished_at`
- `duration_ms`

If existing columns have different names, prefer compatibility over needless churn. Add migration helpers carefully.

### 13.3. App events

Record useful events in `app_events`, for example:

- `task_queued`
- `task_started`
- `task_succeeded`
- `task_retry_scheduled`
- `task_failed`
- `task_blocked`
- `stuck_task_reconciled`
- `n8n_http_error`
- `n8n_contract_error`

Do not spam app_events with excessive noise. Store enough context for debugging.

### 13.4. Migration

Implement safe migration:

```text
v3 -> v4
```

Must support:

- fresh DB creation at v4;
- migration from v3 to v4;
- already-v4 DB no-op.

Do not require deleting `app.db`.

---

## 14. File behavior on success

### 14.1. Source file

Do **not** mutate the source file during Stage 4 execution.

Rationale:

- Stage 3 established non-destructive source behavior.
- DB is the runtime source of truth for execution state.
- Mutating source JSON during execution creates checksum churn and reconciliation ambiguity.

### 14.2. Target file

If current stage has `next_stage`, create/update only through existing managed copy/file operation architecture.

But Stage 4 must create the target file from the **n8n response**, not merely copy the original source payload unchanged.

Target JSON should be normalized:

```json
{
  "id": "entity-demo-001",
  "current_stage": "next_stage_id",
  "next_stage": "stage_after_next_or_null",
  "status": "pending",
  "payload": {
    "title": "hello beehive",
    "title_processed": "HELLO BEEHIVE"
  },
  "meta": {
    "source": "manual",
    "n8n": {
      "workflow": "beehive-demo-transform"
    },
    "beehive": {
      "created_by": "stage4_n8n_execution",
      "source_stage_id": "transform_title",
      "target_stage_id": "review",
      "source_entity_file_id": 123,
      "stage_run_id": "..."
    }
  }
}
```

Rules:

- `id` should remain the same logical entity id.
- `current_stage` must be the target stage.
- `next_stage` should come from target stage config, if available.
- `status` should be `pending` by default.
- `payload` should come from `response.payload`.
- `meta` should merge original source meta and response meta, with `meta.beehive` provenance.
- If target stage has no next stage, `next_stage` may be `null`.

### 14.3. Collision behavior

Reuse Stage 3 managed file collision policy.

Do not overwrite existing target file destructively unless the existing implementation already allows a safe compatible path.

If target file exists and is compatible, register actual on-disk bytes/checksum, not newly generated bytes that were not written.

This Stage 3 consistency fix must not regress.

---

## 15. HTTP client requirements

Use a Rust HTTP client appropriate for Tauri backend.

Requirements:

- async-friendly if commands are async;
- JSON body support;
- timeout support;
- capture status code;
- capture response body as text/bytes before parsing;
- distinguish network/timeout/HTTP/contract errors.

Suggested behavior:

```text
timeout_sec = stage.request_timeout_sec or runtime.request_timeout_sec or default 30
```

If adding config field:

```yaml
runtime:
  request_timeout_sec: 30
```

or per stage:

```yaml
stages:
  - id: transform_title
    request_timeout_sec: 30
```

Keep it simple. Do not build a full HTTP client configuration UI.

### 15.1. URL validation

Allow:

- `https://...`
- `http://localhost...`
- `http://127.0.0.1...`
- mock HTTP server URLs in tests

Warn or reject clearly if URL is empty/malformed.

Do not prevent tests from using local `http://` mock endpoints.

---

## 16. Pipeline config validation updates

Extend config validation if needed:

- executable stage must have `workflow_url`;
- `workflow_url` must be valid URL;
- `max_attempts >= 1`;
- `retry_delay_sec >= 0`;
- optional `request_timeout_sec > 0`;
- `next_stage`, if present, should reference an existing stage;
- stage with `next_stage` should have usable output folder behavior.

Do not make validation too strict for future graph branching, but catch obvious config errors.

---

## 17. Tauri commands to add/update

Add backend commands similar to:

```text
run_due_tasks(workdir_path)
run_entity_stage(workdir_path, entity_id, stage_id)
list_stage_runs(workdir_path, entity_id?)
reconcile_stuck_tasks(workdir_path)
```

Names can differ if consistent with existing conventions.

Commands must:

- validate/open workdir;
- bootstrap DB schema if needed;
- use existing config loading;
- return typed responses;
- not panic on normal failures.

---

## 18. Frontend changes

Keep UI minimal.

### 18.1. Dashboard

Add:

- `Run due tasks` button;
- summary of runtime execution:
  - pending;
  - queued;
  - in_progress;
  - retry_wait;
  - done;
  - failed;
  - blocked;
- last run result summary;
- latest execution errors.

### 18.2. Entity Detail

Add:

- stage state attempts;
- last error;
- next retry time;
- last HTTP status;
- stage run history;
- optional `Run this stage` button if easy.

### 18.3. Stage Editor / Diagnostics

Add read-only visibility where useful:

- workflow URL;
- max attempts;
- retry delay;
- timeout;
- active/inactive stage status.

Do not implement polished UX.

---

## 19. Test requirements

Automated tests are required. Use mock HTTP endpoints, not real n8n.

### 19.1. Required Rust tests

Add tests for:

1. successful n8n execution:
   - pending state becomes done;
   - stage_run success row is written;
   - next-stage file is created from response payload;
   - next-stage state is pending;
   - source file is not mutated.

2. HTTP non-2xx:
   - attempt recorded;
   - status becomes retry_wait if attempts remain;
   - status becomes failed after max attempts.

3. network/timeout error:
   - retry_wait or failed according to attempts;
   - error_type recorded.

4. invalid JSON response:
   - treated as failed attempt;
   - response/error recorded.

5. response `{ "success": false }`:
   - treated as failed attempt;
   - error message recorded.

6. missing `payload` when next_stage exists:
   - treated as contract error.

7. retry wait not due:
   - not executed before `next_retry_at`.

8. retry wait due:
   - executed after `next_retry_at`.

9. stuck `in_progress` reconciliation:
   - old in_progress becomes retry_wait or failed.

10. already done state:
   - not executed again.

11. blocked structural problem:
   - missing/inactive next_stage leads to blocked or clear failure according to chosen behavior.

12. stage_runs audit:
   - request snapshot exists;
   - response snapshot or error exists;
   - attempt number correct;
   - duration is set.

13. managed copy consistency:
   - created target file checksum matches DB;
   - existing compatible target does not regress Stage 3 checksum fix.

### 19.2. Frontend tests

Only add frontend tests if the project already has a reasonable pattern.

At minimum:

```text
npm run build
```

must pass.

### 19.3. Commands to run

Run and report:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
```

Rust tests via the existing Windows-compatible method, for example through `vcvars64.bat` if required.

```bash
npm run build
```

Do not hit the real n8n instance from automated tests.

---

## 20. Manual/smoke verification

Only smoke-level UI verification is expected:

- app starts;
- main window opens;
- Dashboard does not crash;
- `Run due tasks` button is visible if workdir is loaded.

Do not perform page-by-page visual QA.

Optional technical manual check:

- user can later test the real webhook manually with the known production URL;
- Codex should not rely on this real URL for automated verification.

---

## 21. Documentation requirements

Update or create:

```text
docs/codex_stage4_delivery_report.md
docs/codex_stage4_instruction_checklist.md
docs/codex_stage4_progress.md
```

Update README if new commands/config fields are added.

Documentation must include:

- execution model;
- request contract;
- response contract;
- retry rules;
- success/failure criteria;
- schema v4 changes;
- migration behavior;
- test strategy;
- known limitations;
- what is intentionally left for later.

---

## 22. Known limitations acceptable after Stage 4

It is acceptable if Stage 4 still does not have:

- background auto-run loop;
- start/stop daemon UI;
- credential management;
- webhook authentication;
- complex branch routing;
- n8n execution polling;
- n8n REST API integration;
- visual retry controls;
- full manual UI QA.

But it must have a reliable manually triggered execution core.

---

## 23. Expected deliverables

Return your result in this structure:

### A. What was implemented

Short but specific.

### B. Files changed

Group by backend, frontend, docs, tests.

### C. n8n execution behavior

Explain request, response, success criteria, failure handling.

### D. Retry behavior

Explain attempts, retry_wait, next_retry_at, failed.

### E. Schema/migration changes

Explain schema version, migration path, new/changed columns.

### F. File behavior

Explain source file immutability and target file creation from n8n response.

### G. Tests added/updated

List exact scenarios.

### H. Verification performed

List commands and results.

### I. Known limitations

Be honest.

### J. Whether Stage 4 is ready for review

Say yes/no and why.

---

## 24. Acceptance criteria

Stage 4 can be accepted only if:

- n8n webhook execution is implemented through configurable `workflow_url`;
- real n8n URL is not hardcoded in application code;
- successful HTTP 2xx valid JSON response marks source stage done;
- successful response creates next-stage file from response payload;
- source file is not mutated;
- stage run audit row is written;
- failed attempts schedule retry or mark failed;
- `retry_wait` respects `next_retry_at`;
- stuck `in_progress` states are reconciled;
- schema migration works from v3 to v4;
- automated tests cover success/failure/retry/core contract;
- `cargo fmt`, Rust tests, and `npm run build` pass;
- docs are updated;
- no full manual UI walkthrough is claimed.

---

## 25. Final instruction

Build this as a stable execution foundation.

Do not chase UI polish.
Do not add unrelated features.
Do not call the real n8n endpoint in automated tests.
Do not hide failures behind generic messages.
Prioritize correctness of state transitions, retry behavior, stage_runs audit, and file consistency.
