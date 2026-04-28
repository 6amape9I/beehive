# beehive — Stage 9 Codex Task

# Stabilization, Full Manual QA, Demo Workdir, Multi-Output n8n Support, and Release Readiness

Ты работаешь в проекте **beehive**.

Этот документ является **единственным источником истины** для реализации **Stage 9**. Stage 9 — это не новый продуктовый экран, а этап подготовки к показу и первому реальному использованию. Нужно довести уже собранный MVP до стабильного состояния, подготовить demo workdir, провести полный ручной прогон и закрыть критичные edge-cases.

Перед началом работы обязательно перечитай:

- `README.md`;
- `instructions/beehive_stage1_codex_task.md`;
- `instructions/beehive_stage2_codex_task.md`;
- `instructions/beehive_stage3_codex_task.md`;
- `instructions/beehive_stage4_codex_task.md`;
- `instructions/beehive_stage5_codex_task.md`;
- `instructions/beehive_stage5_5_codex_task.md`;
- `instructions/beehive_stage6_codex_task.md`;
- `instructions/beehive_stage6_polish_codex_task.md`;
- `instructions/beehive_stage7_codex_task.md`;
- `instructions/beehive_stage8_codex_task.md`;
- все delivery reports/checklists по Stage 1–8 в `docs/`;
- текущий код:
  - `src-tauri/src/executor`;
  - `src-tauri/src/file_ops`;
  - `src-tauri/src/discovery`;
  - `src-tauri/src/database`;
  - `src-tauri/src/commands`;
  - `src-tauri/src/config`;
  - `src-tauri/src/pipeline_editor`;
  - `src-tauri/src/workdir`;
  - `src/pages/*`;
  - `src/types/domain.ts`;
  - `package.json`;
  - `src-tauri/Cargo.toml`.

Не полагайся на память. Проверяй фактическое состояние кода.

---

## 0. Главный фокус Stage 9

Stage 9 должен подготовить beehive к показу и первому внутреннему использованию.

Фокус:

1. Полная ручная проверка приложения.
2. Demo workdir с реальными JSON-примерами.
3. One-action запуск проекта.
4. Поддержка n8n response как списка JSON неизвестной длины.
5. Улучшение стабильности, edge-cases и UX ошибок.
6. Проверка retry/recovery/reconciliation.
7. Нагрузочный/объёмный прогон на большом числе JSON.
8. Release build и пользовательская инструкция.

Это не этап для больших новых фич вроде полноценного фонового daemon, multi-user, ACL, React Flow graph editor, n8n REST workflow management или distributed processing.

---

## 1. Накопительный polish backlog, который можно закрыть в Stage 9

После Stage 7–8 накопились некритичные polish-пункты. Stage 9 — подходящее место закрыть их, если они не превращаются в большой rewrite.

Закрыть:

1. `pipeline.yaml.bak...` filename должен быть устойчив к двум save в одну секунду.
   - Добавить миллисекунды/наносекунды или UUID в backup/temp filename.
2. В command wrapper для draft validation при backend error вернуть validation issue с `severity = error`, а не `is_valid = true` + отдельный `errors`.
3. Убрать лишнюю перезагрузку Workspace Explorer при выборе файла, если она реально присутствует.
   - `selectedFileId` не должен быть причиной повторного `get_workspace_explorer`, если это не нужно.
4. В Workspace Explorer trail-node folder action синхронизировать с backend-safe policy или DTO.
   - Missing file не должен выглядеть как гарантированно openable folder без причины.
5. Зафиксировать “config repair mode” как deferred, если не делается сейчас.
   - Не реализовывать полноценный repair mode, если это затянет Stage 9.

Не делать:
- большой split `pipeline_editor/mod.rs`, если это не нужно для Stage 9;
- миграцию БД без необходимости;
- новый design system;
- background scheduler.

---

## 2. One-action запуск проекта

### 2.1. Цель

Пользователь/демонстратор должен иметь понятный способ запустить проект одной командой.

Минимальный результат:

```powershell
npm.cmd run app
```

или, если проект запускается на Unix-like окружении:

```bash
npm run app
```

Команда должна запускать desktop-приложение Tauri в dev/demo режиме.

### 2.2. Требования к scripts

Обновить `package.json` так, чтобы были понятные команды:

```json
{
  "scripts": {
    "app": "...",
    "demo:reset": "...",
    "demo": "...",
    "verify": "...",
    "release": "..."
  }
}
```

Рекомендуемая семантика:

- `npm run app` — запускает desktop app в dev mode.
- `npm run demo:reset` — пересоздаёт/подготавливает demo workdir до чистого состояния.
- `npm run demo` — готовит demo workdir и запускает приложение.
- `npm run verify` — запускает frontend build + Rust tests или печатает, какие команды нужно выполнить на Windows/MSVC.
- `npm run release` — запускает Tauri release build.

Если команда отличается из-за Tauri CLI или окружения, выбери фактически рабочий вариант и задокументируй его.

### 2.3. Optional default workdir

Если это легко и безопасно, добавь возможность запускать demo с предвыбранным workdir через env var, например:

```text
BEEHIVE_DEFAULT_WORKDIR=demo/workdir
```

Но это не должно ломать обычный сценарий выбора workdir через UI.

Если автоподстановка workdir требует слишком много изменений, достаточно:

- `npm run demo:reset` создаёт demo;
- `npm run app` запускает приложение;
- README и manual QA checklist явно говорят открыть `demo/workdir`.

---

## 3. Demo workdir

### 3.1. Цель

В репозитории должна появиться demo-папка, на которой можно показать всё заявленное:

- выбор workdir;
- scan;
- dashboard;
- entities table;
- entity detail;
- JSON payload editor;
- retry/reset/skip/open file/open folder;
- stage editor;
- workspace explorer;
- n8n run;
- multi-output response;
- managed copies;
- missing/invalid examples;
- release/manual verification.

### 3.2. Расположение

Создать:

```text
demo/
  README.md
  workdir/
    pipeline.yaml
    stages/
      incoming/
      n8n_output/
      review/
      invalid_samples/
    logs/
  scripts/               // optional; можно вместо этого использовать корневой scripts/
scripts/
  reset-demo.mjs         // если удобнее держать root scripts
  generate-demo-data.mjs // для нагрузочного сценария
```

Не коммитить тяжёлые generated bulk-data файлы на 5000–10000 штук. Для нагрузки сделать генератор.

### 3.3. Demo pipeline

В `demo/workdir/pipeline.yaml` создать pipeline минимум из двух stages.

Рекомендуемая структура:

```yaml
project:
  name: beehive-demo
  workdir: .

runtime:
  scan_interval_sec: 5
  max_parallel_tasks: 3
  stuck_task_timeout_sec: 120
  request_timeout_sec: 60
  file_stability_delay_ms: 300

stages:
  - id: semantic_split
    input_folder: stages/incoming
    output_folder: stages/n8n_output
    workflow_url: https://n8n-dev.steos.io/webhook/b0c81347-5f51-4142-b1d9-18451d8c4ecf
    max_attempts: 2
    retry_delay_sec: 5
    next_stage: review

  - id: review
    input_folder: stages/n8n_output
    output_folder: ""
    workflow_url: https://n8n-dev.steos.io/webhook/b0c81347-5f51-4142-b1d9-18451d8c4ecf
    max_attempts: 2
    retry_delay_sec: 5
    next_stage: null
```

`workflow_url` должен быть взят из пользовательского требования. Не использовать этот внешний endpoint в автоматических тестах. Автотесты должны использовать mock HTTP server.

### 3.4. Demo JSON shape

Каждый demo input file должен быть полноценной beehive JSON-сущностью:

```json
{
  "id": "demo-ceramic-001",
  "current_stage": "semantic_split",
  "next_stage": "review",
  "status": "pending",
  "payload": {
    "...": "business fields only"
  },
  "meta": {
    "source": "demo",
    "created_at": "2026-04-27T00:00:00Z"
  }
}
```

Важное правило: пользователь редактирует только информационные поля внутри `payload`. Служебные поля root-level (`id`, `current_stage`, `next_stage`, `status`, `meta.beehive`, runtime state) не должны редактироваться обычным JSON editor.

В demo payload не добавлять beehive-служебные заголовки. Вложенный `payload` должен содержать только бизнес-информацию, похожую на пользовательские примеры.

### 3.5. Обязательные demo entities

Создать минимум 10 demo JSON-файлов в `demo/workdir/stages/incoming`.

Обязательно включить три пользовательских примера как payload:

#### `demo-ceramic-001.json`

```json
{
  "id": "demo-ceramic-001",
  "current_stage": "semantic_split",
  "next_stage": "review",
  "status": "pending",
  "payload": {
    "entity_name": "керамика",
    "source_parent_name": "материал",
    "source_grandparent_name": "материя",
    "source_entity_context": null,
    "source_semantic_description": null,
    "parent_name_candidate": null,
    "grandparent_name_candidate": null,
    "parent_name": "string | null",
    "grandparent_name": "string | null",
    "entity_weight": null,
    "entity_fullness": null,
    "relation_id": null,
    "relation_weight": null,
    "relation_truth": null,
    "strength": null,
    "source_entity_id": null,
    "current_languages": [],
    "ready": null,
    "representations": []
  },
  "meta": {
    "source": "demo",
    "created_at": "2026-04-27T00:00:00Z"
  }
}
```

#### `demo-horizon-001.json`

```json
{
  "id": "demo-horizon-001",
  "current_stage": "semantic_split",
  "next_stage": "review",
  "status": "pending",
  "payload": {
    "entity_name": "горизонт",
    "source_parent_name": "граница",
    "source_grandparent_name": "пространство",
    "source_entity_context": null,
    "source_semantic_description": null,
    "parent_name_candidate": null,
    "grandparent_name_candidate": null,
    "parent_name": "string | null",
    "grandparent_name": "string | null",
    "entity_weight": null,
    "entity_fullness": null,
    "relation_id": null,
    "relation_weight": null,
    "relation_truth": null,
    "strength": null,
    "source_entity_id": null,
    "current_languages": [],
    "ready": null,
    "representations": []
  },
  "meta": {
    "source": "demo",
    "created_at": "2026-04-27T00:00:00Z"
  }
}
```

#### `demo-castle-001.json`

```json
{
  "id": "demo-castle-001",
  "current_stage": "semantic_split",
  "next_stage": "review",
  "status": "pending",
  "payload": {
    "entity_name": "замок",
    "source_parent_name": "здание",
    "source_grandparent_name": "сооружение",
    "source_entity_context": null,
    "source_semantic_description": null,
    "parent_name_candidate": null,
    "grandparent_name_candidate": null,
    "parent_name": "string | null",
    "grandparent_name": "string | null",
    "entity_weight": null,
    "entity_fullness": null,
    "relation_id": null,
    "relation_weight": null,
    "relation_truth": null,
    "strength": null,
    "source_entity_id": null,
    "current_languages": [],
    "ready": null,
    "representations": []
  },
  "meta": {
    "source": "demo",
    "created_at": "2026-04-27T00:00:00Z"
  }
}
```

Остальные demo files должны использовать тот же schema-паттерн payload. Примеры entity names:

- `стекло`
- `мост`
- `река`
- `архив`
- `компас`
- `облако`
- `сигнал`

Payload должен быть полезным и похожим на реальные онтологические данные, не пустым `{}`.

### 3.6. Demo invalid samples

Добавить один-два invalid samples в `demo/workdir/stages/incoming` или отдельный documented scenario. Например:

- invalid JSON syntax;
- JSON без `id`;
- JSON без `payload`.

Если invalid files ломают “happy path” demo, держи их в `demo/workdir/stages/invalid_samples` и в `demo/README.md` объясни, как скопировать их в `stages/incoming` для проверки invalid scan UX.

### 3.7. Demo reset

`npm run demo:reset` должен приводить demo workdir к стабильному baseline:

- удалить `demo/workdir/app.db`, если существует;
- очистить generated output folders (`stages/n8n_output`, `stages/review`) от generated artifacts;
- восстановить input JSON examples;
- восстановить `pipeline.yaml`;
- создать нужные папки;
- не требовать ручного копирования файлов.

Можно реализовать через Node script или Rust helper. Выбери самый простой cross-platform вариант.

---

## 4. n8n response: список JSON неизвестной длины

### 4.1. Новое обязательное поведение

На выходе n8n workflow может быть сразу несколько JSON-элементов.

Например:

- вход: 1 source entity;
- n8n response: список из 3 JSON payload objects;
- результат: beehive должен сохранить 3 разных JSON-файла в папку следующего stage.

### 4.2. Supported response contract

Поддержать минимум следующие формы ответа:

#### Preferred modern contract: root array

```json
[
  { "entity_name": "..." },
  { "entity_name": "..." },
  { "entity_name": "..." }
]
```

#### Object wrapper with payload array

```json
{
  "success": true,
  "payload": [
    { "entity_name": "..." },
    { "entity_name": "..." }
  ],
  "meta": {
    "workflow": "demo"
  }
}
```

#### Backward-compatible single object payload

Для совместимости с существующими тестами и workflows можно поддержать старый формат как один output item:

```json
{
  "success": true,
  "payload": {
    "entity_name": "..."
  },
  "meta": {}
}
```

Но README должен объяснять, что Stage 9 preferred response — список JSON payload objects.

### 4.3. Response validation rules

Каждый output item должен быть JSON object.

Invalid cases:

- root is scalar/string/number;
- payload array contains non-object item;
- payload missing when object wrapper requires next-stage output;
- `success: false`;
- HTTP status non-2xx.

Для invalid response сохраняется failed/retry behavior как раньше.

### 4.4. Target JSON wrapping rule

n8n output item считается **business payload only**. Beehive должен обернуть каждый output item в full beehive entity JSON:

```json
{
  "id": "generated-child-id",
  "current_stage": "target_stage_id",
  "next_stage": "target_next_stage_or_null",
  "status": "pending",
  "payload": {
    "...": "n8n output object"
  },
  "meta": {
    "created_at": "...",
    "updated_at": "...",
    "source": "n8n",
    "beehive": {
      "created_by": "n8n_response",
      "source_entity_id": "...",
      "source_entity_file_id": 123,
      "source_stage_id": "...",
      "target_stage_id": "...",
      "stage_run_id": "...",
      "output_index": 0,
      "output_count": 3
    }
  }
}
```

Do not put beehive service metadata inside `payload`.

### 4.5. Child entity id / filename policy

Implement deterministic, collision-safe IDs and file names for multi-output children.

Required policy:

1. If output item has a safe explicit string field `id` and this does not collide unsafely, it may be used as child `id`.
2. Otherwise generate child id from:
   - source `entity_id`;
   - target stage id;
   - output index;
   - short hash of canonical output payload.

Example:

```text
{source_entity_id}__{target_stage_id}__{index}_{hash8}
```

Example filename:

```text
demo-ceramic-001__review__0_a1b2c3d4.json
```

Requirements:

- output ids must be stable for same source + target + output index + payload;
- rerun/retry with same response must not create duplicate incompatible files;
- identical output payloads in the same response must still have unique ids because output index is included;
- unsafe existing file collision must be blocked/failed with clear event.

### 4.6. Multi-output copy transaction behavior

Implementation should be as safe and idempotent as practical:

1. Parse and validate response completely before writing files.
2. Compute all planned output files and checksums before writing.
3. Preflight target collisions:
   - compatible existing file: register/reuse as `already_exists`;
   - incompatible existing file: fail/block according to existing safe file operation rules.
4. Write each target JSON atomically via temp file + rename.
5. Register every target file in SQLite as managed copy.
6. Link every child to `copy_source_file_id`.
7. Only mark source stage `done` after all required child outputs are created/registered successfully.
8. If partial creation happens due to unexpected crash, retry should not duplicate compatible existing files.

### 4.7. Stage run history

`stage_runs.response_json` should store the actual response body or normalized response body.

For multi-output success, app_events should include:

- `output_count`;
- source entity id;
- source file id;
- target stage id;
- created/registered target paths.

### 4.8. Terminal stage behavior

If current stage has no `next_stage`:

- validate response as successful according to contract;
- do not create target files;
- mark current state `done`;
- store response in `stage_runs`.

Do not invent terminal output folder behavior in Stage 9.

### 4.9. Tests for multi-output

Add Rust tests with mock HTTP server:

1. root array with 3 objects creates 3 target JSON files.
2. object wrapper with `payload: [object, object]` creates 2 target JSON files.
3. backward-compatible `payload: object` creates 1 target JSON file.
4. output item scalar causes retry/failed, no target files.
5. duplicate rerun with compatible existing files is idempotent.
6. generated child JSON contains root service fields and payload-only business object.
7. child `meta.beehive` contains source/run/output metadata.
8. terminal stage with array response does not create target files and marks done.
9. target collision with different content fails/blocks safely.
10. source entity remains done only after all outputs are registered.

---

## 5. Stabilization / edge-cases

Review and harden these areas:

### 5.1. Workdir and config

- missing workdir;
- missing `pipeline.yaml`;
- invalid `pipeline.yaml`;
- missing `app.db`;
- invalid stage folders;
- inactive stage with historical files;
- terminal stage;
- stage removed from YAML but still in SQLite history.

Do not add full config repair mode unless it is small. Document as deferred if not done.

### 5.2. Scanner

- partially written file;
- file changed during scan;
- missing file after registration;
- restored file;
- duplicate entity id in same stage;
- same entity across multiple stages;
- invalid JSON;
- JSON without `id`;
- JSON without `payload`;
- non-JSON files;
- large number of JSON files.

### 5.3. Runtime

- pending -> queued -> in_progress -> done;
- network error -> retry_wait;
- retries exhausted -> failed;
- stale queued recovery;
- stale in_progress reconciliation;
- source file changed after scan but before execution;
- stage removed/inactive;
- missing next stage;
- multi-output success.

### 5.4. Manual actions

- retry now;
- reset to pending;
- skip;
- open file;
- open folder;
- JSON payload/meta edit only;
- save rejected for queued/in_progress/done.

### 5.5. UI error UX

Improve where low-risk:

- command error panels should be visible and human-readable;
- long technical error should not break layout;
- success/failure action messages should be clear;
- buttons should disable during active actions;
- empty states should explain next action;
- manual checklist should mention screenshots/notes.

Do not redesign UI. Polish only.

---

## 6. Reconciliation / restart testing

### 6.1. Automated tests

Ensure tests cover:

- stale `queued` returns to `pending` safely;
- orphan `stage_runs` are reconciled;
- stale `in_progress` goes to due `retry_wait` if attempts remain;
- stale `in_progress` goes to `failed` if attempts exhausted;
- no duplicate HTTP request after recovery;
- source JSON runtime status does not overwrite SQLite done/failed on scan.

### 6.2. Manual scenario

Manual QA checklist must include simulated restart / recovery scenario:

1. create or seed an `in_progress` state older than `stuck_task_timeout_sec`;
2. restart app or invoke reconciliation;
3. verify state changes to `retry_wait` or `failed`;
4. verify Dashboard/Entities/Detail reflect it.

If seeding manually is too hard for operator, add a dev-only helper or documented test command. Do not expose dangerous helper in production UI unless clearly marked dev-only.

---

## 7. Load / volume testing

### 7.1. Demo data generator

Create a script:

```text
scripts/generate-demo-data.mjs
```

or equivalent.

It should generate N valid demo JSON files into a selected demo stage folder.

Example commands:

```powershell
node scripts/generate-demo-data.mjs --workdir demo/workdir --stage incoming --count 1000
node scripts/generate-demo-data.mjs --workdir demo/workdir --stage incoming --count 5000
```

If you add npm aliases:

```powershell
npm.cmd run demo:generate -- --count 1000
```

### 7.2. Requirements

Generated files must:

- use valid beehive wrapper JSON;
- have unique ids;
- use realistic payload structure similar to demo examples;
- not include service metadata inside `payload`;
- be deterministic enough for repeatable tests when seed is provided;
- not be committed when generated in large volume.

### 7.3. Load test expectations

Add documentation for manual load test:

- reset demo;
- generate 1000 files;
- scan workspace;
- measure elapsed time from scan summary;
- verify UI remains usable;
- optionally generate 5000 files;
- record result in `docs/manual_stage9_qa_results.md`.

Do not make CI run 5000/10000-file tests by default. Use ignored Rust tests or manual scripts for heavy load.

### 7.4. Scanner optimization

Review scanner performance.

Low-risk optimizations are allowed:

- skip full JSON parse for unchanged file if checksum/mtime/size unchanged and DB snapshot exists;
- avoid unnecessary DB writes for unchanged files;
- keep prepared statements/transactions where safe;
- avoid recursive scan unless explicitly required.

Do not risk correctness to chase benchmark numbers.

---

## 8. Full manual QA is now mandatory

Stage 9 must include a **full manual verification pass**.

You must create:

```text
docs/stage9_manual_qa_checklist.md
docs/stage9_manual_qa_results.md
```

`docs/stage9_manual_qa_checklist.md` should be based on the separate manual checklist instruction file and should be committed.

`docs/stage9_manual_qa_results.md` should contain:

- date/time;
- OS;
- commit SHA;
- commands run;
- manual tester name/initials if available;
- pass/fail/N/A for every major section;
- bugs found;
- screenshots/recording references if any;
- final manual QA status.

If you cannot perform a real full manual UI walkthrough in the execution environment, you must still:
- create the checklist;
- clearly state which sections are not personally executed;
- do not claim full manual QA passed unless actually performed.

However, the project requirement for Stage 9 is that **now we do perform full manual QA before accepting the stage**. The architect/user will use this checklist to accept or reject.

---

## 9. User documentation

Create or update:

```text
docs/user_guide.md
docs/demo_guide.md
docs/release_checklist.md
```

Minimum content:

### `docs/user_guide.md`

- what beehive does;
- how workdir works;
- how pipeline.yaml works;
- how to scan;
- how to run due tasks;
- how retry/failed works;
- how to use Dashboard;
- how to use Entities;
- how to use Entity Detail;
- how to edit payload/meta safely;
- how to use Stage Editor;
- how to use Workspace Explorer;
- how to interpret errors;
- what not to edit manually.

### `docs/demo_guide.md`

- one-action launch;
- demo reset;
- demo workdir structure;
- demo files;
- n8n endpoint used;
- happy path scenario;
- multi-output expected behavior;
- invalid file scenario;
- load generation scenario.

### `docs/release_checklist.md`

- prerequisites;
- build commands;
- test commands;
- manual QA required;
- where release artifacts appear;
- known limitations;
- pre-demo cleanup.

Update `README.md` with short links to these docs.

---

## 10. Release build

Stage 9 must verify release readiness.

Required commands to run and record:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
```

```powershell
cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'
```

```powershell
npm.cmd run build
```

```powershell
npm.cmd run release
```

If release command differs, document actual command.

Acceptance requires:
- release command exists;
- release build either passes, or any platform-specific blocker is clearly documented with exact error;
- no claim of release-ready if release build failed.

---

## 11. Automated test requirements

Add/update tests for:

### 11.1. Multi-output n8n

As specified in Section 4.9.

### 11.2. Demo integrity

Add a lightweight test or script verification:

- `demo/workdir/pipeline.yaml` parses;
- demo input JSON files are valid beehive entities;
- payloads are business-only;
- required folders exist after `demo:reset`.

### 11.3. Existing regressions

Ensure existing tests still pass for:

- workdir bootstrap;
- config validation;
- scanner;
- file safety;
- atomic claim;
- state machine;
- retry;
- reconciliation;
- stage editor validation/save;
- workspace explorer read-only behavior;
- open file/folder path safety;
- JSON edit policy.

### 11.4. Heavy load

Do not run huge load tests by default. Provide manual or ignored tests.

---

## 12. Acceptance criteria

Stage 9 is accepted only if:

1. `npm run app` or documented equivalent starts the app in one action.
2. Demo workdir exists and can be reset.
3. Demo contains realistic JSON entities with payload-only business data.
4. Demo pipeline uses the provided n8n endpoint.
5. App can open demo workdir.
6. Scan registers demo JSON files.
7. Dashboard shows demo stages and counters.
8. Entities Table shows demo entities with filters/search.
9. Entity Detail shows payload/meta editor and manual actions.
10. JSON editor cannot edit service fields.
11. JSON editor is blocked for queued/in_progress/done.
12. Stage Editor can validate/save pipeline changes.
13. Workspace Explorer shows stage tree, files, missing/invalid, trails.
14. Run due tasks can process demo against n8n endpoint, or if endpoint fails, failure/retry is handled visibly.
15. n8n multi-output array creates multiple next-stage JSON files.
16. Retry and failed states behave correctly.
17. Reconciliation/restart scenario is tested.
18. Large demo generation/scanning scenario is documented and at least one practical volume run is recorded.
19. Full manual QA checklist exists.
20. Manual QA results file exists and is honest.
21. User guide exists.
22. Demo guide exists.
23. Release checklist exists.
24. `cargo fmt` passes.
25. Rust tests pass.
26. `npm run build` passes.
27. `npm run release` is attempted and result is documented.
28. No real secrets are committed.
29. No automated tests call the real n8n endpoint.
30. README is updated.

---

## 13. Required docs from Codex

Create/update:

```text
docs/codex_stage9_progress.md
docs/codex_stage9_instruction_checklist.md
docs/codex_stage9_delivery_report.md
docs/stage9_manual_qa_checklist.md
docs/stage9_manual_qa_results.md
docs/user_guide.md
docs/demo_guide.md
docs/release_checklist.md
```

`docs/codex_stage9_delivery_report.md` must include:

```md
# Stage 9 Delivery Report

## Implemented
...

## Demo workdir
...

## Multi-output n8n support
...

## One-action launch
...

## Manual QA
...

## Load testing
...

## Release build
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

Do not claim manual QA or release readiness if not actually verified.

---

## 14. Final instruction

Do not overbuild. Do not start post-MVP features. Stage 9 is about confidence.

The project should now be showable:

- one command to start;
- demo data present;
- full manual checklist present;
- multi-output n8n handled;
- major edge-cases tested;
- release command available;
- documentation useful to a real operator.
