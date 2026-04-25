# beehive — Stage 5 Codex Task

## Этап 5. UI обзорного уровня

Ты работаешь в проекте **beehive**.

Это задание является единственным источником истины для Stage 5. Перед началом работы обязательно перечитай актуальные документы проекта, delivery reports предыдущих этапов и текущий код. Не полагайся на память.

Цель Stage 5 — дать оператору наглядный обзор состояния пайплайна: какие stages существуют, как они связаны, сколько сущностей находится в каждом статусе, какие задачи сейчас активны, какие ошибки произошли последними, и можно ли быстро понять, где система требует внимания.

**Ручное UI-тестирование не выполняй: только automated logic tests, frontend build/typecheck и минимальный UI smoke без mouse-driven walkthrough.**

---

## 0. Контекст текущего состояния

К моменту Stage 5 в проекте уже должны существовать:

- Tauri desktop application.
- Workdir-based project model.
- `pipeline.yaml`.
- SQLite database.
- schema v4.
- logical `entities`.
- physical `entity_files`.
- `entity_stage_states`.
- `stage_runs`.
- app events / diagnostics.
- scanner/reconciliation.
- managed next-stage copy.
- n8n execution foundation.
- retry mechanics.
- stuck task reconciliation.
- minimal runtime UI.

Stage 5 не должен переписывать runtime core. Он должен построить поверх уже существующей модели удобный обзорный интерфейс и, если нужно, добавить backend read-models/queries для Dashboard.

---

## 1. Главная цель этапа

Реализовать обзорный Dashboard, на котором оператор видит общую картину системы:

- stages и связи между ними;
- агрегированные счётчики по статусам;
- активные задачи;
- последние ошибки;
- общее состояние runtime;
- быстрые действия верхнего уровня.

Dashboard должен быть полезен даже при 100–200 JSON-файлах сейчас и не должен деградировать при росте до тысяч файлов в день.

---

## 2. Что НЕ входит в Stage 5

Не реализовывать в этом этапе:

- новый execution engine;
- background daemon;
- parallel worker pool;
- scheduler;
- полноценную очередь с автоматическим фоновым запуском;
- редактор графа stages drag-and-drop;
- создание/редактирование stage через UI, если этого ещё нет;
- визуальный конструктор workflow;
- управление n8n workflows через n8n REST API;
- authentication/authorization;
- продвинутый design system;
- полноценный audit/event explorer;
- массовые операции над entities;
- reset/requeue actions для runtime state, если это требует изменения state machine.

Допустимы только небольшие UI actions, уже поддержанные существующим backend:

- scan workspace;
- run due tasks;
- reconcile stuck tasks;
- refresh dashboard data;
- переходы на существующие страницы entities/stage detail/entity detail, если такие routes уже есть.

---

## 3. Основные пользовательские сценарии

### 3.1 Оператор открывает приложение

Оператор должен быстро понять:

- какой workdir открыт;
- сколько всего entities известно системе;
- сколько stages active/inactive;
- есть ли задачи в работе;
- есть ли ошибки;
- когда последний раз выполнялся scan;
- когда последний раз запускались задачи;
- есть ли pending/retry/failed/blocked состояния.

### 3.2 Оператор смотрит граф stages

Оператор должен видеть:

- список stages;
- направление переходов;
- stages без `next_stage`;
- inactive/archived stages;
- stages с ошибками или blocked entities;
- stage-level счётчики.

Для Stage 5 достаточно статичного readable graph. Не нужен drag-and-drop редактор.

### 3.3 Оператор ищет проблемное место

Dashboard должен подсвечивать:

- stages с `failed`;
- stages с `blocked`;
- stages с большим количеством `retry_wait`;
- текущие `in_progress`;
- последние error-level app events;
- последние failed/blocked stage runs.

### 3.4 Оператор запускает технические действия

Dashboard может давать кнопки:

- `Scan workspace`;
- `Run due tasks`;
- `Reconcile stuck tasks`;
- `Refresh`.

После действия Dashboard должен обновлять данные.

---

## 4. Требуемый результат

К концу Stage 5 пользователь должен видеть общую картину системы на одном экране:

1. Summary cards.
2. Stage graph.
3. Stage status counters.
4. Active tasks block.
5. Last errors block.
6. Basic operational actions.
7. Loading/error/empty states.

---

## 5. Архитектурный принцип

Stage 5 — это read-oriented UI слой.

Если текущих backend-команд достаточно, можно использовать их. Но предпочтительно добавить один агрегирующий backend endpoint/command для Dashboard, чтобы UI не делал много несогласованных запросов и не собирал критичные агрегаты на клиенте.

Рекомендуемый подход:

```text
SQLite/runtime data
        ↓
backend dashboard read model
        ↓
Tauri command get_dashboard_overview
        ↓
frontend runtimeApi.getDashboardOverview()
        ↓
DashboardPage
```

---

## 6. Backend requirements

### 6.1 Добавить dashboard read model

Добавить backend command:

```rust
get_dashboard_overview(workdir_path: String) -> Result<DashboardOverview, String>
```

или использовать существующий runtime context pattern проекта, если команды уже принимают workdir иначе. Следуй текущей архитектуре command layer, не вводи новый несовместимый стиль.

Команда должна:

- загрузить runtime context/workdir;
- открыть SQLite;
- при необходимости выполнить лёгкую reconciliation только если это уже принято архитектурой для read-команд;
- вернуть агрегированную модель для Dashboard;
- не мутировать execution state без явной необходимости;
- не запускать tasks;
- не обращаться в n8n;
- не сканировать всю файловую систему автоматически, если пользователь не нажал scan.

### 6.2 DTO: DashboardOverview

Предлагаемый контракт:

```ts
type DashboardOverview = {
  generated_at: string;

  project: {
    name: string;
    workdir_path: string;
  };

  totals: {
    entities_total: number;
    entity_files_total: number;
    stages_total: number;
    active_stages_total: number;
    inactive_stages_total: number;
    active_tasks_total: number;
    errors_total: number;
    warnings_total: number;
  };

  runtime: {
    last_scan_at: string | null;
    last_run_at: string | null;
    last_successful_run_at: string | null;
    last_error_at: string | null;
    due_tasks_count: number;
    in_progress_count: number;
    retry_wait_count: number;
    failed_count: number;
    blocked_count: number;
  };

  stage_graph: {
    nodes: DashboardStageNode[];
    edges: DashboardStageEdge[];
  };

  stage_counters: DashboardStageCounters[];

  active_tasks: DashboardActiveTask[];

  last_errors: DashboardErrorItem[];

  recent_runs: DashboardRunItem[];
};
```

Не обязательно использовать ровно эти имена, если в проекте уже есть принятый naming style, но frontend/backend должны быть типизированы и согласованы.

### 6.3 DashboardStageNode

```ts
type DashboardStageNode = {
  id: string;
  label: string;
  input_folder: string;
  output_folder: string | null;
  workflow_url: string | null;
  is_active: boolean;
  archived_at: string | null;
  next_stage: string | null;
  position_index: number;
  health: "ok" | "warning" | "error" | "inactive";
};
```

Health semantics:

- `inactive` — stage inactive/archived.
- `error` — есть `failed` или `blocked` states на этом stage.
- `warning` — есть `retry_wait` или stale `in_progress`.
- `ok` — нет очевидных проблем.

### 6.4 DashboardStageEdge

```ts
type DashboardStageEdge = {
  from_stage_id: string;
  to_stage_id: string;
  is_valid: boolean;
  problem: string | null;
};
```

Rules:

- edge valid, если target stage существует и active;
- edge invalid, если `next_stage` указан, но target missing;
- edge invalid/warning, если target inactive;
- stage без `next_stage` просто terminal stage, это не ошибка.

### 6.5 DashboardStageCounters

```ts
type DashboardStageCounters = {
  stage_id: string;
  stage_label: string;
  is_active: boolean;

  total: number;
  pending: number;
  queued: number;
  in_progress: number;
  retry_wait: number;
  done: number;
  failed: number;
  blocked: number;
  skipped: number;
  unknown: number;

  missing_files: number;
  existing_files: number;

  last_started_at: string | null;
  last_finished_at: string | null;
};
```

Status list должен соответствовать фактической domain model проекта. Не выдумывай новые статусы без необходимости. Если каких-то статусов нет — не добавляй их в enum, но в UI можно иметь fallback `unknown`.

### 6.6 DashboardActiveTask

Активные задачи для Dashboard:

```ts
type DashboardActiveTask = {
  entity_id: string;
  stage_id: string;
  status: string;
  attempts: number;
  max_attempts: number;
  next_retry_at: string | null;
  last_started_at: string | null;
  updated_at: string | null;
  file_path: string | null;
  reason: string | null;
};
```

Включить:

- `in_progress`;
- due `retry_wait`;
- future `retry_wait`;
- optionally `pending` tasks due to run, если их не слишком много.

Ограничить список, например `LIMIT 20` или `LIMIT 50`. Dashboard не должен тянуть тысячи строк.

Suggested ordering:

1. `in_progress`;
2. due `retry_wait`;
3. future `retry_wait`;
4. oldest pending.

### 6.7 DashboardErrorItem

```ts
type DashboardErrorItem = {
  id: number | string;
  level: string;
  event_type: string;
  message: string;
  entity_id: string | null;
  stage_id: string | null;
  run_id: string | null;
  created_at: string;
};
```

Источник:

- `app_events` level error/warn;
- optionally latest failed `stage_runs`.

Ограничение: `LIMIT 10` или `LIMIT 20`.

### 6.8 DashboardRunItem

```ts
type DashboardRunItem = {
  run_id: string;
  entity_id: string;
  stage_id: string;
  success: boolean;
  http_status: number | null;
  error_type: string | null;
  error_message: string | null;
  started_at: string;
  finished_at: string | null;
  duration_ms: number | null;
};
```

Ограничение: последние 10–20 runs.

---

## 7. SQLite/query requirements

### 7.1 Aggregates must come from SQLite

Не считай важные агрегаты на frontend из полного списка entities, если это может стать тяжёлым.

Backend должен получать:

- counts by stage/status;
- latest errors;
- active tasks;
- latest runs;
- stage graph metadata.

### 7.2 Performance

Для Stage 5 не нужна сложная оптимизация, но запросы должны быть разумными:

- использовать `GROUP BY`;
- использовать `LIMIT`;
- не читать все payload JSON для Dashboard;
- не сканировать все файлы на диске;
- не парсить каждый JSON-файл ради счётчиков.

### 7.3 Индексы

Проверь, достаточно ли индексов после schema v4.

Если их нет, добавь lightweight indexes в schema/migration, например:

- `entity_stage_states(stage_id, status)`;
- `entity_stage_states(status, next_retry_at)`;
- `stage_runs(started_at)`;
- `stage_runs(entity_id, stage_id)`;
- `app_events(level, created_at)` или аналогичные реальные поля.

Если добавляешь индексы, это может быть schema v5. Но не поднимай schema version только ради необязательных индексов, если проектная миграционная политика позволяет `CREATE INDEX IF NOT EXISTS` в existing schema maintenance. Следуй уже принятому стилю проекта.

Если меняешь schema version — обязательно:

- fresh DB bootstrap;
- migration from v4 to v5;
- tests.

Не делай schema migration без необходимости.

---

## 8. Frontend requirements

### 8.1 DashboardPage

Реализовать или существенно обновить `DashboardPage`.

Dashboard должен состоять минимум из следующих блоков:

1. Header / Project context.
2. Operational actions.
3. Summary cards.
4. Stage graph.
5. Stage counters.
6. Active tasks.
7. Last errors.
8. Recent runs or compact runtime activity block.

Если экран получается длинным — это нормально. Главное — чтобы оператор видел самое важное сверху.

### 8.2 Header / Project context

Показать:

- project name;
- workdir path;
- generated/refreshed time;
- last scan;
- last run;
- small status indicator.

Пример:

```text
beehive
Workdir: C:\...\workdir
Last scan: 2026-04-25 14:21
Last run: 2026-04-25 14:23
```

### 8.3 Operational actions

Кнопки:

- `Refresh`;
- `Scan workspace`;
- `Run due tasks`;
- `Reconcile stuck`.

После каждого действия:

- показать loading state;
- disable relevant buttons while running;
- показать success/error message;
- обновить Dashboard overview.

Не запускай n8n автоматически при открытии Dashboard. Только по кнопке `Run due tasks`.

### 8.4 Summary cards

Показать cards:

- Entities total.
- Active stages.
- Pending / due.
- In progress.
- Retry wait.
- Failed.
- Blocked.
- Last errors.

Cards должны быть clickable только если уже есть routes/filters, которые можно надёжно использовать. Иначе не делай ложные ссылки.

### 8.5 Stage graph

Реализовать простой читаемый stage graph.

Допустимые варианты:

#### Preferred simple implementation

Без новой тяжёлой зависимости:

- CSS grid/flex;
- stage cards arranged by `position_index`;
- arrows using CSS/SVG between cards;
- edge list under graph for invalid/missing links.

Это достаточно для линейного графа и простых branch cases.

#### Allowed with justification

Можно добавить библиотеку графа, например React Flow, только если:

- зависимость не раздувает проект чрезмерно;
- есть явная польза;
- build проходит;
- UI остаётся простым;
- не требуется сложный editor.

Для Stage 5 предпочтительнее не добавлять тяжёлую зависимость.

Graph stage card должен показывать:

- stage id/name;
- active/inactive;
- next stage;
- status counters compactly;
- health badge;
- warning/error indicator.

Пример карточки:

```text
incoming
active · next: normalized

pending 12
running 1
done 83
failed 2
blocked 0
```

### 8.6 Stage counters table

Добавить таблицу или compact grid:

Columns:

- Stage
- Active
- Pending
- In progress
- Retry
- Done
- Failed
- Blocked
- Missing files
- Last activity

Таблица должна быть readable и не требовать горизонтального скролла на обычном desktop width, насколько возможно.

### 8.7 Active tasks block

Показывать:

- entity id;
- stage id;
- status;
- attempts/max;
- next retry;
- last started;
- reason/message if exists.

Для `retry_wait` показать human-readable next retry:

- due now;
- in N minutes;
- at timestamp.

Для `in_progress` показать как долго выполняется, если возможно.

### 8.8 Last errors block

Показывать последние ошибки:

- severity;
- event type;
- message;
- entity/stage/run, если есть;
- timestamp.

Ошибки должны быть compact, но достаточно информативны.

Если ошибок нет — показать нормальное empty state:

```text
No recent errors.
```

### 8.9 Recent runs block

Показывать последние runs:

- success/failure;
- entity;
- stage;
- HTTP status;
- error type;
- duration;
- finished time.

Это не основной audit explorer, просто overview.

### 8.10 Empty states

Dashboard должен корректно работать:

- no workdir selected;
- fresh initialized workdir;
- no stages;
- stages exist but no entities;
- no errors;
- no active tasks;
- database missing but workdir valid;
- pipeline.yaml invalid.

Не допускай белого экрана.

---

## 9. Visual/UX requirements

UI должен быть простым, техническим, плотным, но читаемым.

Рекомендации:

- cards with clear labels;
- consistent status badges;
- avoid decorative complexity;
- no heavy animations;
- no hidden critical information;
- use existing app styling conventions;
- keep responsive desktop layout;
- make error/warning states visually distinguishable;
- use concise text.

Status colors may use existing CSS variables/classes. Если в проекте нет design tokens, добавь минимальные классы без сложной темы.

---

## 10. Status semantics

Используй текущую domain model.

Expected statuses include, if already present:

- `pending`;
- `queued`;
- `in_progress`;
- `retry_wait`;
- `done`;
- `failed`;
- `blocked`;
- `skipped`.

Если фактическая модель отличается — следуй фактической модели и задокументируй mapping.

Dashboard не должен менять execution state.

---

## 11. Stage graph rules

### 11.1 Linear stages

Если stages:

```yaml
incoming -> normalized -> enriched
```

Dashboard должен отобразить цепочку в таком порядке.

### 11.2 Terminal stage

Если `next_stage = null`, stage считается terminal. Это не ошибка.

### 11.3 Missing target stage

Если `next_stage` указывает на несуществующий stage:

- edge invalid;
- source node health warning/error;
- показать problem text.

### 11.4 Inactive target stage

Если target stage archived/inactive:

- edge invalid or warning;
- source node должен показывать problem.

### 11.5 Branching

Если в будущем появятся branch cases, Stage 5 не обязан идеально раскладывать граф, но не должен падать. Минимально:

- отобразить все nodes;
- отобразить все edges;
- показать unknown/multiple edges в списке.

---

## 12. Commands/API requirements

### 12.1 Backend command

Добавить в Tauri command registry:

```rust
get_dashboard_overview
```

or similarly named command following project convention.

### 12.2 Frontend API wrapper

Добавить в runtime API:

```ts
getDashboardOverview(...)
```

или camelCase по текущему стилю.

### 12.3 Types

Добавить TypeScript types:

- `DashboardOverview`;
- `DashboardStageNode`;
- `DashboardStageEdge`;
- `DashboardStageCounters`;
- `DashboardActiveTask`;
- `DashboardErrorItem`;
- `DashboardRunItem`.

Types должны соответствовать Rust serialization.

### 12.4 Error handling

Frontend должен обрабатывать rejected invoke:

- show error box;
- not crash page;
- allow retry/refresh.

---

## 13. Testing requirements

### 13.1 Backend tests

Добавить Rust tests для dashboard read model.

Минимальные тесты:

1. Fresh DB / no entities:
   - overview returns;
   - stages are shown;
   - counters are zero.

2. Stage graph:
   - valid linear edge is valid;
   - missing next stage is invalid;
   - inactive target is marked as warning/error.

3. Status counters:
   - create states with pending/done/failed/blocked/retry_wait;
   - aggregate counts per stage are correct.

4. Active tasks:
   - includes in_progress;
   - includes retry_wait;
   - orders/limits reasonably;
   - does not include done.

5. Last errors:
   - app_events error/warn included;
   - limit works;
   - ordering latest first.

6. Recent runs:
   - successful and failed runs appear;
   - http_status/error_type/duration included.

7. Dashboard command does not call n8n and does not mutate execution states.

### 13.2 Frontend tests

If the project already has frontend testing infrastructure, add lightweight component tests.

If not, do not introduce heavy testing stack just for Stage 5. Rely on:

- TypeScript compile;
- production build;
- small pure helper tests only if existing setup supports them.

### 13.3 Required verification commands

Run:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
```

On Windows with MSVC environment:

```cmd
cmd.exe /c "call \"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat\" >nul && cargo test --manifest-path src-tauri\Cargo.toml"
```

Frontend:

```bash
npm run build
```

If command names differ on Windows, use the project’s existing verified commands and document exactly what was run.

---

## 14. Documentation requirements

Add/update docs:

- `docs/codex_stage5_delivery_report.md`
- `docs/codex_stage5_instruction_checklist.md`
- `docs/codex_stage5_progress.md`

Update README only if Stage 5 changes operator-facing usage.

### 14.1 Delivery report must include

- summary of implemented Dashboard;
- backend commands/read models added;
- frontend pages/components added;
- what graph supports;
- what counters mean;
- active tasks definition;
- errors definition;
- limitations;
- tests run;
- manual UI testing statement.

### 14.2 Checklist must include

At least:

- [ ] Dashboard overview command implemented.
- [ ] Dashboard frontend wired to real data.
- [ ] Stage graph displays stages and edges.
- [ ] Invalid/missing/inactive edges are visible.
- [ ] Stage counters are aggregated from SQLite.
- [ ] Active tasks block is shown.
- [ ] Last errors block is shown.
- [ ] Recent runs/activity block is shown.
- [ ] Operational buttons refresh data.
- [ ] Empty/loading/error states handled.
- [ ] Backend tests added.
- [ ] `cargo fmt` passed.
- [ ] Rust tests passed.
- [ ] `npm build` passed.
- [ ] No full manual UI walkthrough claimed.

---

## 15. Acceptance criteria

Stage 5 is accepted when:

1. Dashboard opens without crashing.
2. Dashboard uses real runtime data, not placeholders.
3. Stages and links are visible.
4. Each stage shows status counters.
5. Missing/inactive target links are visible as problems.
6. Operator sees active tasks.
7. Operator sees recent errors.
8. Operator sees recent runs/activity.
9. Summary cards reflect aggregate state.
10. Scan/run/reconcile/refresh actions work if backend supports them.
11. Dashboard has sane empty/loading/error states.
12. Backend tests cover dashboard aggregation.
13. Existing Stage 1–4 tests still pass.
14. Frontend build passes.
15. Documentation is updated and honest.

---

## 16. Important implementation notes

### 16.1 Do not regress previous stages

Do not break:

- workdir initialization;
- pipeline.yaml loading;
- schema v4 migration;
- scan/reconciliation;
- entity_files;
- managed copy;
- n8n execution;
- retry mechanics;
- blocked state handling;
- `run_due_tasks`;
- `run_entity_stage`.

### 16.2 Avoid accidental execution

Dashboard read should not execute due tasks automatically.

Execution happens only when user presses `Run due tasks`.

### 16.3 Avoid filesystem-heavy dashboard

Dashboard should not scan workdir by default.

User must press `Scan workspace` for filesystem scan.

### 16.4 Respect SQLite as execution-state source of truth

Do not reintroduce the Stage 4 bug where JSON status overwrites SQLite execution state.

Dashboard counters must be based on `entity_stage_states`, not raw file JSON status.

### 16.5 Keep components maintainable

Prefer splitting Dashboard into small components:

- `SummaryCards`;
- `StageGraph`;
- `StageCountersTable`;
- `ActiveTasksPanel`;
- `LastErrorsPanel`;
- `RecentRunsPanel`;
- `DashboardActions`.

Names may differ, but avoid one huge unmaintainable component.

---

## 17. Suggested file layout

Adjust to actual project structure.

Backend:

```text
src-tauri/src/dashboard/mod.rs
src-tauri/src/commands/mod.rs
src-tauri/src/domain/mod.rs
src-tauri/src/database/mod.rs
src-tauri/src/lib.rs
```

Frontend:

```text
src/pages/DashboardPage.tsx
src/components/dashboard/SummaryCards.tsx
src/components/dashboard/StageGraph.tsx
src/components/dashboard/StageCountersTable.tsx
src/components/dashboard/ActiveTasksPanel.tsx
src/components/dashboard/LastErrorsPanel.tsx
src/components/dashboard/RecentRunsPanel.tsx
src/lib/runtimeApi.ts
src/types/domain.ts
```

Docs:

```text
docs/codex_stage5_delivery_report.md
docs/codex_stage5_instruction_checklist.md
docs/codex_stage5_progress.md
```

---

## 18. Final response expected from Codex

When finished, provide a concise report:

```md
# Stage 5 Delivery Summary

## Implemented
...

## Files changed
...

## Backend/read model
...

## Frontend/UI
...

## Tests
...

## Verification commands
...

## Known limitations
...

## Stage 5 acceptance status
...
```

Do not claim manual UI walkthrough unless it was actually performed. For this stage, it should not be performed except for minimal smoke if convenient.
