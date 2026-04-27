# beehive — Stage 5.5 Codex Task

## Закрытие несоответствий этапов 1–5 перед переходом к дальнейшей функциональной разработке

Ты работаешь в проекте **beehive**.

Этот документ является **единственным источником истины** для стабилизационного этапа **Stage 5.5**. Перед началом работы обязательно перечитай:

- актуальный `README.md`;
- `instructions/beehive_stage1_codex_task.md`;
- `instructions/beehive_stage2_codex_task.md`;
- `instructions/beehive_stage3_codex_task.md`;
- `instructions/beehive_stage4_codex_task.md`;
- `instructions/beehive_stage5_codex_task.md`;
- delivery reports и checklists по Stage 1–5 в `docs/`;
- текущий код backend/frontend.

Не полагайся на память. Не переписывай архитектуру без необходимости. Этот этап должен закрыть выявленные расхождения и укрепить уже реализованный фундамент.

---

# 0. Контекст текущего состояния

По итогам архитектурного ревью состояние проекта оценивается так:

| Этап | Оценка | Статус |
|---|---:|---|
| Stage 1. Инициализация проекта и каркас | 90–95% | Практически выполнен |
| Stage 2. Модель данных и runtime-ядро | 75–85% | Выполнен, но нужна доработка state machine / locking |
| Stage 3. Сканер workdir и управление файлами | 80–90% | В основном выполнен |
| Stage 4. n8n и retry | 80–90% | В основном выполнен |
| Stage 5. Dashboard | 85–90% | Выполнен как read-only operator dashboard |

Текущий проект уже имеет:

- Tauri + React + TypeScript desktop foundation;
- Rust backend commands через Tauri;
- SQLite через `rusqlite`;
- YAML config через `serde_yaml`;
- `workdir` model;
- logical `entities`;
- physical `entity_files`;
- `entity_stage_states`;
- `stage_runs`;
- `app_events`;
- reconciliation scanner;
- managed next-stage copy;
- n8n webhook execution foundation;
- retry mechanics;
- stuck `in_progress` reconciliation;
- Stage 5 Dashboard read model.

Главная задача Stage 5.5 — не добавлять новый пользовательский функционал, а закрыть архитектурные риски, которые мешают считать этапы 1–5 полностью принятыми.

---

# 1. Главная цель Stage 5.5

Стабилизировать runtime core и файловую модель так, чтобы первые пять этапов соответствовали ТЗ не только функционально, но и архитектурно.

Необходимо закрыть следующие ключевые несоответствия:

1. Нет единого формального слоя state machine, через который проходят runtime-переходы статусов.
2. Нет достаточно строгого атомарного task claiming / runtime lock в SQLite, защищающего от двойного запуска одной entity/stage задачи.
3. Нет полноценной защиты от partially-written / unstable JSON-файлов.
4. Terminal-stage / optional `output_folder` нужно привести к утверждённой модели.
5. Нужно явно зафиксировать и защитить решение: source JSON не мутируется при runtime execution, SQLite является источником runtime-состояния.
6. Нужно обновить тесты и документацию так, чтобы эти гарантии были проверяемыми.

---

# 2. Что НЕ входит в Stage 5.5

Не реализовывать в этом этапе:

- Stage 6 как полноценную таблицу сущностей с TanStack Table;
- полноценный JSON editor;
- полноценные manual actions `reset / skip / retry now` как пользовательский UX, если это требует отдельной продуктовой проработки;
- Stage 7 stage CRUD editor;
- Stage 8 полноценный Workspace Explorer redesign;
- React Flow graph editor;
- background daemon / scheduler;
- true parallel worker pool;
- n8n REST API integration;
- credential manager;
- authentication UI;
- distributed locking между несколькими машинами;
- масштабную миграцию frontend design system;
- ручной mouse-driven UI walkthrough как обязательную часть.

Можно делать небольшие UI/diagnostics изменения только если они нужны для видимости новых runtime гарантий.

---

# 3. Принципы реализации

1. SQLite остаётся источником runtime-состояния.
2. JSON-файлы остаются источником бизнес-данных.
3. `pipeline.yaml` остаётся источником stage configuration.
4. UI не должен напрямую менять файловую систему или SQLite, минуя backend orchestration layer.
5. Runtime-переходы статусов должны быть проверяемыми и тестируемыми.
6. Любая операция, которая может привести к повторной обработке, должна быть атомарной или идемпотентной.
7. Не нужно делать большой rewrite. Предпочти точечные, хорошо протестированные изменения.
8. Не ходить в реальный n8n из автоматических тестов. Только mock HTTP server.

---

# 4. Обязательные backend workstreams

## 4.1. Формальная state machine

Добавить отдельный backend-модуль для state machine, например:

```text
src-tauri/src/state_machine/mod.rs
```

Название может отличаться, если оно лучше соответствует текущей архитектуре.

### 4.1.1. Обязательная модель статусов

Использовать существующие статусы:

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

Не добавлять новые публичные статусы без необходимости.

### 4.1.2. Базовые допустимые переходы

State machine должна явно поддерживать утверждённые переходы:

```text
pending -> queued
queued -> in_progress
in_progress -> done
in_progress -> retry_wait
retry_wait -> queued
retry_wait -> failed
pending -> skipped
pending -> blocked
done -> blocked       // только если уже есть существующий terminal/copy сценарий, иначе не использовать
failed -> pending     // только manual reset / future manual action
skipped -> pending    // только manual reset / future manual action
```

Недопустимые прямые переходы должны возвращать структурированную ошибку, а не silently succeed.

### 4.1.3. Внутренний recovery-переход для `queued`

Если текущая реализация использует `queued` как atomic claim перед `in_progress`, необходимо обработать crash/restart между claim и start.

Разрешается добавить **внутренний технический переход**:

```text
queued -> pending
```

но только для recovery expired/stale claim, если:

- HTTP-вызов ещё не был отправлен;
- `stage_runs` для этой попытки ещё не создан или попытка не стартовала;
- attempts не увеличиваются;
- событие пишется в `app_events` как `queued_claim_released` или аналогично;
- это явно задокументировано как internal recovery transition, а не пользовательская операция.

Если можно реализовать claim так, чтобы `queued` не зависал, этот переход можно не добавлять. Но зависший `queued` state не должен оставаться без reconciliation.

### 4.1.4. Единая точка переходов

Все runtime-изменения `entity_stage_states.status` должны проходить через один слой:

- либо через state machine service;
- либо через database wrapper, который вызывает state machine validation.

Прямые SQL `UPDATE entity_stage_states SET status = ...` допустимы только:

- при первичном создании state row;
- при миграциях;
- в очень узких технических случаях, явно обоснованных в комментарии и тестах.

Нужно обновить существующие функции вроде:

- `update_stage_state_for_run_start`;
- `update_stage_state_success`;
- `update_stage_state_failure`;
- `block_stage_state`;
- stuck reconciliation;
- task queue/claim flow;

так, чтобы они не обходили state machine незаметно.

### 4.1.5. Transition metadata

Для transition logic добавить понятие причины перехода, например enum:

```rust
RuntimeTransitionReason
```

Минимальные причины:

- `RuntimeClaim`
- `RuntimeStart`
- `RuntimeSuccess`
- `RuntimeRetryScheduled`
- `RuntimeFailed`
- `RuntimeBlocked`
- `StuckReconciliation`
- `ManualResetReserved`
- `ManualSkipReserved`
- `ClaimRecovery`

Названия могут отличаться, но смысл должен быть явным.

### 4.1.6. Ошибки state machine

Ошибка недопустимого перехода должна содержать:

- from status;
- to status;
- reason;
- entity_id, stage_id или state_id, если доступны;
- человекочитаемое сообщение.

---

## 4.2. Атомарный task claiming / runtime lock

Текущий runtime не должен позволять двум одновременным вызовам `run_due_tasks` или `run_entity_stage` обработать одну и ту же `entity_stage_state` дважды.

### 4.2.1. Требуемое поведение

Задача считается claimed только если SQLite атомарно перевёл её из eligible состояния в `queued`.

Eligible состояния:

```text
pending
retry_wait with next_retry_at <= now
```

Дополнительные условия:

- stage active;
- file exists;
- source file stable and still matches registered DB snapshot;
- workflow_url not empty;
- attempts < max_attempts.

### 4.2.2. Claim должен быть атомарным

Codex должен реализовать safe claim pattern. Возможный подход:

1. В транзакции выбрать кандидатов.
2. Для каждого кандидата выполнить `UPDATE ... WHERE id = ? AND status = expected_status AND attempts < max_attempts AND file_exists = 1`.
3. Считать задачу claimed только если `affected_rows == 1`.
4. Немедленно перечитать claimed row из БД.
5. Не запускать HTTP, если claim не получен.

Если для надёжности нужны дополнительные поля вроде `claimed_at`, `claim_token`, `claim_owner`, можно добавить их, но только с аккуратной migration strategy. Не поднимай schema version без необходимости.

### 4.2.3. `run_due_tasks`

`run_due_tasks` должен использовать новый claim path.

Запрещено:

- сначала получить список eligible tasks, а потом запускать их без атомарного claim;
- выполнять HTTP по task record, если claim update не прошёл;
- создавать `stage_runs` для задачи, которая не была claimed.

### 4.2.4. `run_entity_stage`

`run_entity_stage` может оставаться manual/debug path, но он не должен обходить защиту от двойного запуска.

Допустимо:

- manual/debug path может запускать `retry_wait` до `next_retry_at`, если это уже принято текущей архитектурой;
- но он не может запускать `done`, `failed`, `blocked`, `skipped`, active `queued`, active `in_progress`;
- он не может запускать unstable/changed source file;
- он должен использовать тот же claim или эквивалентную атомарную защиту.

### 4.2.5. Reconciliation для active states

Проверить и усилить reconciliation:

- stale `in_progress` старше `runtime.stuck_task_timeout_sec` остаётся по текущему правилу: `retry_wait` или `failed`;
- stale `queued`, если такой state возможен, должен быть восстановлен безопасно;
- после перезапуска приложение не должно навсегда оставлять задачу в `queued` без возможности дальнейшей обработки.

---

## 4.3. Защита от partially-written / unstable JSON-файлов

ТЗ требует, чтобы partially-written файл не запускался в обработку. Нужно реализовать это явно.

### 4.3.1. Runtime config

Добавить необязательную настройку runtime:

```yaml
runtime:
  file_stability_delay_ms: 1500
```

Если поле отсутствует, использовать safe default `1500` ms или другой разумный default, явно описанный в README/docs.

Если не хочешь менять YAML model, можно сделать internal constant, но предпочтительно дать настройку через runtime config.

### 4.3.2. Scanner behavior

Во время `scan_workspace` файл должен считаться unstable и временно пропускаться, если:

- его `mtime` слишком свежий относительно `file_stability_delay_ms`;
- metadata до чтения и после чтения отличается;
- размер изменился во время чтения;
- mtime изменился во время чтения;
- файл недоступен из-за временной блокировки/записи.

Unstable файл:

- не должен регистрироваться как valid entity;
- не должен перезаписывать уже зарегистрированный DB snapshot;
- не должен считаться permanent invalid JSON;
- должен давать `app_events` уровня `info` или `warning` с кодом вроде `unstable_file_skipped`;
- должен попадать в scan summary или settings counter, если это легко сделать без большого UI rewrite.

### 4.3.3. Executor pre-flight guard

Перед claim/start execution нужно убедиться, что source file:

- существует;
- всё ещё regular file;
- достаточно stable по тому же правилу;
- соответствует зарегистрированному DB snapshot хотя бы по size + mtime, а лучше по checksum;
- не был изменён после последнего scan/registration.

Если файл changed/unstable before execution:

- не отправлять HTTP в n8n;
- не создавать `stage_runs` как реальную попытку n8n;
- не увеличивать attempts;
- не переводить state в failed;
- записать `app_events` с понятным кодом вроде `source_file_unstable_before_execution` или `source_file_changed_before_execution`;
- вернуть task outcome `skipped` или отдельный skipped reason в summary.

Цель: operator должен нажать `Scan workspace`, дождаться стабильного файла, и только потом запускать execution.

### 4.3.4. Не превращать временный partial JSON в ошибку бизнес-валидации

Если файл временно содержит битый JSON из-за активной записи, scanner не должен сразу создавать permanent invalid event `invalid_json_file`, если metadata показывает, что файл свежий/нестабилен.

---

## 4.4. Terminal-stage и optional `output_folder`

Утверждённая модель допускает `output_folder` как optional для terminal-stage.

### 4.4.1. Config validation

Обновить YAML validation:

- `input_folder` обязателен всегда;
- `workflow_url` обязателен для executable stages;
- `next_stage` опционален;
- `output_folder` обязателен, если stage имеет `next_stage`;
- `output_folder` может отсутствовать или быть пустым, если stage terminal (`next_stage` отсутствует/null/empty).

### 4.4.2. Internal model

Выбрать один из подходов:

1. Сделать `output_folder: Option<String>` в domain model и аккуратно обновить Rust/TS DTO.
2. Оставить `output_folder: String`, но нормализовать missing terminal output folder в пустую строку и явно документировать это.

Предпочтительно использовать `Option<String>`, если изменение не приводит к чрезмерной миграции. Если выбираешь empty string, убедись, что UI не показывает это как ошибку.

### 4.4.3. Runtime behavior

Если stage terminal и execution успешен:

- source state становится `done`;
- target copy не создаётся;
- state не должен становиться `blocked` только из-за отсутствия next stage;
- `stage_runs` записывается как success.

Если stage имеет `next_stage`, но target stage missing/inactive:

- это structural blocked/failure по текущей принятой модели;
- HTTP success + невозможная copy должна оставаться `blocked` с понятным `error_type`, как уже реализовано.

---

## 4.5. Source JSON и SQLite source of truth

Не возвращать старое поведение `ready: true` / mutation source JSON в этом этапе.

Текущее принятое архитектурное решение:

- source JSON во время execution не мутируется;
- исходный файл остаётся физическим входным артефактом;
- runtime status/history хранится в SQLite;
- next-stage JSON создаётся из n8n response payload со статусом `pending`;
- reconciliation scan не должен перетирать SQLite execution state статусом из source JSON.

Codex должен:

- проверить, что это не регрессирует;
- добавить/обновить тест, если его нет;
- обновить README/docs, чтобы operator не ожидал, что source JSON получит `done` или `ready: true`.

---

## 4.6. Dashboard / Diagnostics alignment

Stage 5 Dashboard уже read-only. Не нужно переписывать его.

Нужно только добавить минимальную видимость, если она полезна после Stage 5.5:

- runtime safety / file stability config в Settings/Diagnostics;
- last scan unstable count, если добавлен;
- stale queued reconciliation count, если добавлен;
- app_events для state machine / claim errors должны отображаться в existing error panels.

Dashboard read path по-прежнему не должен:

- сканировать файловую систему автоматически;
- запускать n8n;
- менять execution state;
- выполнять reconciliation без явного действия пользователя, если текущая архитектура этого не делает.

---

# 5. Schema / migration guidance

Перед изменениями внимательно проверь текущую schema version.

Если изменения можно сделать без schema bump, не поднимай версию просто ради косметики.

Если добавляешь новые persistent columns, например:

- `claimed_at`;
- `claim_token`;
- `claim_owner`;
- `queued_at`;
- `last_transition_reason`;
- `last_transition_error`;

тогда:

- поднять schema version с v4 до v5;
- реализовать fresh DB bootstrap at v5;
- реализовать safe migration v4 -> v5;
- убедиться, что v1/v2/v3 -> v5 путь не ломается;
- добавить tests на migration.

Если schema остаётся v4, в delivery report явно объяснить почему schema bump не потребовался.

---

# 6. Required tests

Добавить automated tests. Не использовать реальный n8n.

## 6.1. State machine unit tests

Покрыть:

- все допустимые переходы;
- несколько недопустимых переходов, например:
  - `pending -> done`;
  - `done -> in_progress`;
  - `failed -> queued`;
  - `blocked -> in_progress`;
- reason-specific transitions;
- internal `queued -> pending` recovery, если добавлен.

## 6.2. Database transition tests

Проверить, что DB wrappers:

- не пропускают недопустимый transition;
- корректно обновляют timestamps/last_error/next_retry_at;
- не меняют attempts при claim recovery;
- не позволяют повторный `in_progress` для active task.

## 6.3. Atomic claim tests

Проверить минимум:

1. Один pending state claim-ится ровно один раз.
2. Две последовательные попытки claim одного state: первая success, вторая no-op/skipped.
3. Две connection/transaction simulation не создают две execution попытки.
4. `run_due_tasks` не создаёт два `stage_runs` для одного task при повторном вызове до завершения.
5. `run_entity_stage` не обходит active `queued`/`in_progress` state.

Если возможно, добавить concurrency-style test с двумя threads и temporary SQLite DB. Если это слишком нестабильно для CI, сделать deterministic two-connection test.

## 6.4. Partially-written / unstable file tests

Покрыть:

1. Fresh/unstable JSON файл пропускается scanner-ом и не регистрируется как invalid.
2. После того как файл становится stable, следующий scan регистрирует его.
3. Metadata changes during read приводят к skip, не к invalid business error.
4. Уже зарегистрированный файл, изменённый после scan, не отправляется в n8n до повторного стабильного scan.
5. Executor pre-flight guard не создаёт `stage_runs`, не увеличивает attempts и не вызывает mock HTTP для unstable/changed source file.

## 6.5. Terminal-stage tests

Покрыть:

1. Terminal stage без `output_folder` валиден.
2. Stage с `next_stage`, но без `output_folder`, невалиден.
3. Successful terminal-stage execution marks state `done`, writes successful `stage_run`, and creates no target file.
4. Missing/inactive target stage with non-terminal transition still results in structural blocked behavior.

## 6.6. Regression tests for Stage 1–5

Убедиться, что продолжают проходить existing tests по:

- workdir bootstrap;
- config validation;
- SQLite bootstrap/migration;
- discovery/registration;
- managed copy;
- n8n success/failure/retry;
- stuck reconciliation;
- Dashboard read model;
- frontend build.

---

# 7. Frontend requirements

Frontend changes должны быть минимальными.

Обязательно:

- TypeScript types должны соответствовать Rust DTO, если DTO изменены;
- `npm.cmd run build` должен проходить;
- Settings/Diagnostics должны не падать, если добавлены новые runtime fields;
- Dashboard/StageEditor должны корректно отображать terminal stage без `output_folder`;
- no white screen on fresh workdir / invalid config / empty stages / terminal-only pipeline.

Не добавлять тяжёлые frontend dependencies ради Stage 5.5.

---

# 8. Documentation requirements

Создать/обновить:

```text
docs/codex_stage5_5_progress.md
docs/codex_stage5_5_instruction_checklist.md
docs/codex_stage5_5_delivery_report.md
```

Обновить `README.md`, если изменились:

- runtime config;
- file stability behavior;
- state machine guarantees;
- claim/locking behavior;
- terminal stage config rules;
- schema version.

## 8.1. Delivery report must include

- summary of closed Stage 1–5 gaps;
- state machine implementation details;
- task claiming / locking implementation details;
- file stability / partial write guard behavior;
- terminal-stage behavior;
- schema/migration status;
- tests added/updated;
- exact verification commands run;
- known limitations;
- whether Stage 5.5 is ready for review.

## 8.2. Progress log must include

- what documents were reread;
- major code areas inspected;
- implementation steps;
- test/verification results;
- any deviations from this instruction and why.

## 8.3. Checklist must be honest

Do not mark checklist items done unless implementation and tests exist.

---

# 9. Required verification commands

Run and report exact results.

Rust formatting:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
```

Rust tests on Windows/MSVC environment:

```powershell
cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'
```

Frontend build:

```powershell
npm.cmd run build
```

If the environment requires a slightly different command syntax, use the project’s existing verified commands and document exactly what was run.

Do not claim manual UI walkthrough unless it actually happened. For this task, automated tests + build are sufficient.

---

# 10. Acceptance criteria

Stage 5.5 can be accepted only if all are true:

1. Formal state machine module exists.
2. All important runtime status transitions are validated through the state machine or a single DB wrapper using it.
3. Invalid status transitions are rejected and tested.
4. `run_due_tasks` uses atomic claim/lock semantics.
5. `run_entity_stage` does not bypass atomic active-task protection.
6. The same entity/stage task cannot be processed twice by overlapping/sequential duplicate claims.
7. Stale `queued`, if possible, is reconciled safely or impossible by design.
8. Scanner skips unstable/partially-written files without treating them as permanent invalid JSON.
9. Executor does not send unstable/changed source files to n8n.
10. Executor does not create `stage_runs` or increment attempts for skipped unstable/changed files.
11. Terminal stage without `output_folder` is valid.
12. Non-terminal stage with `next_stage` but missing `output_folder` is invalid.
13. Successful terminal-stage execution becomes `done` and does not create target copy.
14. Source JSON is still not mutated during execution.
15. SQLite remains runtime source of truth.
16. Existing Stage 1–5 functionality does not regress.
17. Rust tests cover state machine, claim, unstable file guard, terminal stage, and regressions.
18. `cargo fmt` passes.
19. Rust tests pass.
20. `npm.cmd run build` passes.
21. README/docs are updated honestly.
22. `docs/codex_stage5_5_delivery_report.md` clearly states ready/not ready for review.

---

# 11. Expected final response from Codex

When finished, respond with:

```md
# Stage 5.5 Delivery Summary

## Implemented
...

## Closed Stage 1–5 gaps
...

## State machine
...

## Atomic claiming / locking
...

## File stability guard
...

## Terminal stage behavior
...

## Schema / migration
...

## Files changed
...

## Tests
...

## Verification commands
...

## Known limitations
...

## Stage 5.5 acceptance status
...
```

Do not overstate. If something is partially done, say so explicitly.

---

# 12. Final instruction

This is a stabilization task, not a feature expansion task.

Prioritize correctness, reliability, and testability of the runtime core. The goal is to make Stage 1–5 solid enough that future Stage 6/7/8 work can build on them without revisiting fundamental state, locking, file-stability, or terminal-stage behavior.
