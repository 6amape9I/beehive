# beehive — Follow-up Codex Task

## Harden queued crash recovery around orphan `stage_runs`

Ты работаешь в проекте **beehive**.

Этот документ является единственным источником истины для маленькой follow-up задачи после Stage 5.5. Задача узкая: закрыть crash-window между созданием `stage_runs` и переводом `entity_stage_states.status` из `queued` в `in_progress`.

Не расширяй scope. Не начинай Stage 6 в этой задаче.

---

# 1. Контекст

После Stage 5.5 проект уже имеет:

- формальную runtime state machine;
- atomic SQLite claim из `pending` / due `retry_wait` в `queued`;
- stale `queued` claim recovery;
- pre-flight source file safety guard;
- сохранение source JSON immutable;
- SQLite как runtime source of truth.

Однако в текущем execution flow есть узкое окно:

```text
claim task: pending/retry_wait -> queued
pre-flight source file check
insert stage_runs row
update state: queued -> in_progress
send HTTP to n8n
```

Если приложение падает после `insert stage_runs row`, но до `queued -> in_progress`, в БД может остаться:

- `entity_stage_states.status = queued`;
- незавершённый `stage_runs` row;
- при этом HTTP ещё не был отправлен.

Это не приводит к повторной отправке HTTP, но создаёт нечистую audit history и неоднозначную reconciliation-ситуацию.

---

# 2. Цель задачи

Сделать старт stage run атомарным и корректно обработать legacy/orphan rows, которые могли появиться в таком crash-window.

К концу задачи должно быть невозможно получить частичное состояние:

```text
queued state + созданный незавершённый stage_run до RuntimeStart
```

без того, чтобы система могла явно и безопасно его reconciliate.

---

# 3. Что НЕ входит в задачу

Не реализовывать:

- Stage 6;
- новые entity table/detail UI;
- reset/skip/retry UX;
- background daemon;
- scheduler;
- true parallel workers;
- distributed locking;
- schema v5, если можно обойтись без неё;
- n8n REST API;
- credential manager;
- ручной UI walkthrough.

---

# 4. Требуемое решение

## 4.1. Preferred approach: atomic start transaction

Рекомендуемый подход — добавить backend/database helper, который атомарно делает start claimed task.

Примерная идея:

```rust
start_claimed_stage_run(...)
```

Эта функция должна в одной SQLite transaction:

1. Проверить, что state всё ещё `queued`.
2. Проверить state machine transition `queued -> in_progress` с reason `RuntimeStart`.
3. Создать `stage_runs` row.
4. Обновить `entity_stage_states`:
   - `status = in_progress`;
   - `attempts = attempt_no`;
   - `last_started_at = started_at`;
   - `last_finished_at = NULL`;
   - `next_retry_at = NULL`;
   - `updated_at = started_at`.
5. Commit.

Если transaction не committed, не должно оставаться `stage_runs` row.

После commit HTTP можно отправлять.

## 4.2. Executor flow

Обновить `executor::execute_task` так, чтобы больше не было отдельного небезопасного sequence:

```text
insert_stage_run(...)
update_stage_state_for_run_start(...)
```

Вместо этого executor должен вызывать новый atomic helper.

Требуемый flow:

```text
claimed task is queued
source pre-flight check
build request_json
atomic start transaction:
  insert stage_run
  queued -> in_progress
send HTTP
finish stage_run + done/retry/failed/blocked
```

## 4.3. Legacy/orphan reconciliation

Даже после atomic helper нужно обработать возможные legacy rows от предыдущей версии или тестового ручного состояния.

Если stale `queued` state имеет незавершённый `stage_runs` row, который был создан до start, reconciliation должна:

- не отправлять HTTP;
- не увеличивать attempts;
- освободить stale `queued` claim обратно в `pending`;
- пометить orphan `stage_runs` row как unsuccessful/cancelled/reconciled, например:
  - `success = 0`;
  - `error_type = claim_recovered_before_start` или близкий код;
  - `error_message = "Queued claim was recovered before workflow request was sent."`;
  - `finished_at = now`;
  - `duration_ms = 0` или безопасное вычисленное значение, если возможно;
- записать `app_events` с кодом вроде `orphan_stage_run_reconciled`.

Если текущая схема `stage_runs` уже позволяет это сделать без новых колонок, schema version не поднимать.

---

# 5. State machine requirements

Не обходить Stage 5.5 state machine.

Все изменения статуса должны идти через существующий state machine/database wrapper.

Для legacy orphan reconciliation:

```text
queued -> pending
reason: ClaimRecovery
```

Оставить attempts без изменений.

---

# 6. Tests

Добавить automated Rust tests. Не использовать реальный n8n.

Минимум:

1. **Atomic start success**
   - given claimed `queued` state;
   - when start helper is called;
   - then one `stage_runs` row exists;
   - state becomes `in_progress`;
   - attempts increments to expected attempt number.

2. **No partial run on failed transition**
   - simulate state no longer `queued`;
   - start helper must fail;
   - no new `stage_runs` row is inserted.

3. **Legacy orphan queued reconciliation**
   - seed `queued` state with stale `updated_at`;
   - seed unfinished `stage_runs` row for same entity/stage/file;
   - run `reconcile_stuck_tasks`;
   - state becomes `pending`;
   - attempts are not incremented;
   - orphan run is finished as unsuccessful with clear `error_type`;
   - app event is written.

4. **No duplicate HTTP after orphan recovery**
   - after orphan reconciliation, next `run_due_tasks` may create a new run normally;
   - total audit history should be clear: one reconciled orphan, one actual attempt;
   - mock HTTP receives exactly one request.

5. Existing Stage 5.5 tests must continue to pass.

---

# 7. Documentation requirements

Update/create:

```text
docs/codex_followup_orphan_stage_runs_progress.md
docs/codex_followup_orphan_stage_runs_delivery_report.md
docs/codex_followup_orphan_stage_runs_instruction_checklist.md
```

Update README only if the operator-facing runtime/reconciliation explanation changes.

Delivery report must include:

- what crash-window was closed;
- exact atomic start approach;
- orphan `stage_runs` reconciliation behavior;
- schema version decision;
- tests added;
- verification commands;
- known limitations.

---

# 8. Verification commands

Run and report exact results:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
```

```powershell
cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'
```

```powershell
npm.cmd run build
```

Frontend build should still pass even if this task has no frontend changes.

---

# 9. Acceptance criteria

This follow-up is accepted only if:

- stage run start is atomic;
- no future crash can leave `stage_runs` inserted without `queued -> in_progress` commit;
- stale `queued` reconciliation handles legacy orphan `stage_runs`;
- orphan reconciliation is visible in `app_events`;
- attempts are not incremented during claim recovery;
- no HTTP is sent for recovered orphan rows;
- existing Stage 1–5.5 behavior does not regress;
- Rust tests cover the new guarantees;
- `cargo fmt`, Rust tests, and `npm.cmd run build` pass;
- docs are honest.

---

# 10. Expected final response from Codex

Return:

```md
# Follow-up Delivery Summary

## Implemented
...

## Crash-window closed
...

## Atomic start behavior
...

## Orphan stage_runs reconciliation
...

## Schema decision
...

## Tests
...

## Verification commands
...

## Known limitations
...

## Acceptance status
...
```
