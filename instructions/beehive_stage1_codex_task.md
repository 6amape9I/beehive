# beehive — Stage 1 Task Specification for Codex

## Document purpose

This document is the **single source of truth** for the implementation of **Stage 1** of the desktop application **beehive**.

The programmer agent **Codex** must treat this document as the authoritative specification for the work to be performed. If any implementation detail is unclear, Codex must choose the solution that best preserves architectural correctness, future scalability, clean separation of concerns, and alignment with the long-term direction of the product.

This is **not** a brainstorming note. This is an implementation specification.

---

# 1. Project overview

## 1.1. Project name
**beehive**

## 1.2. Product idea
beehive is a desktop application for orchestrating JSON files through a stage-based processing pipeline, where each stage is associated with a workflow in **n8n**.

The application operates around a local **workdir** that contains:
- pipeline configuration,
- SQLite state database,
- stage folders,
- logs,
- JSON entities that move through the pipeline as copies between stages.

The application is intended to become a reliable operator-facing desktop tool for:
- configuring processing stages,
- monitoring entity progression,
- validating configuration and runtime state,
- later orchestrating actual workflow execution,
- and providing visibility into pipeline health, failures, and file placement.

## 1.3. Long-term direction
The long-term product will support:
- a graph-based stage model,
- mostly linear pipelines in day-to-day usage,
- file state tracking,
- retries,
- stage transitions,
- history of runs,
- operator inspection,
- UI views for dashboard, entities, stage configuration, and workspace tree.

However, **Stage 1 is only the project foundation stage**.

---

# 2. Scope of Stage 1

## 2.1. Stage 1 objective
Codex must build a **clean, scalable, correct foundation** for the beehive desktop application using:

- **Tauri**
- **React**
- **TypeScript**
- **SQLite**

The result must be a working desktop application skeleton that can:

1. launch locally as a desktop app,
2. provide a usable UI shell with routing/navigation,
3. open an existing `workdir`,
4. initialize a new `workdir`,
5. load and validate `pipeline.yaml`,
6. create/open `app.db`,
7. bootstrap the initial SQLite schema,
8. synchronize stage definitions from YAML into the database,
9. present configuration/bootstrap state in the UI.

## 2.2. What Stage 1 is **not**
Stage 1 is **not** the runtime orchestration engine.

Codex must **not** implement the following in Stage 1:
- actual n8n workflow execution,
- task queue processing,
- retry engine runtime,
- file scanning runtime,
- state machine execution,
- actual JSON entity processing,
- transition logic between stages,
- full stage CRUD editor,
- graph routing logic,
- advanced workspace reconciliation,
- stage-run history runtime,
- domain-heavy manual operations.

Stage 1 is about **foundation, bootstrap, structure, and future-proof architecture**.

---

# 3. Approved product assumptions (must be respected)

These decisions are already approved and must be used by Codex.

## 3.1. Approved status model
The following domain statuses are approved and must exist as shared domain types:

- `pending`
- `queued`
- `in_progress`
- `retry_wait`
- `done`
- `failed`
- `blocked`
- `skipped`

These statuses do not need full runtime behavior in Stage 1, but they **must** be represented in domain types and be ready for later stages.

## 3.2. Approved stage model
The product uses a **graph-capable stage model**, but the most common user scenario is linear.

For Stage 1, the application should assume one primary `next_stage` per stage, while keeping architecture clean enough for future extension.

Each stage conceptually has:
- unique `id`,
- input folder,
- output folder,
- workflow reference or workflow URL,
- retry policy,
- next stage rule.

## 3.3. Approved JSON entity model
The long-term minimal JSON entity structure is:

```json
{
  "id": "entity-0001",
  "current_stage": "ingest",
  "next_stage": "normalize",
  "status": "pending",
  "payload": {},
  "meta": {
    "created_at": "2026-04-23T12:00:00Z",
    "updated_at": "2026-04-23T12:00:00Z",
    "source": "manual"
  }
}
```

Stage 1 does not need to process these entities, but the codebase must be structured with these domain concepts in mind.

## 3.4. Approved pipeline config model
The YAML config model should support this structure:

```yaml
project:
  name: beehive
  workdir: ./data

runtime:
  scan_interval_sec: 5
  max_parallel_tasks: 3
  stuck_task_timeout_sec: 900

stages:
  - id: ingest
    input_folder: stages/incoming
    output_folder: stages/normalized
    workflow_url: http://localhost:5678/webhook/ingest
    max_attempts: 3
    retry_delay_sec: 10
    next_stage: normalize
```

Codex may enrich the internal model if necessary, but must remain compatible with this structure.

## 3.5. Approved SQLite table direction
The application must prepare for the following tables:
- `settings`
- `stages`
- `entities`
- `entity_stage_states`
- `stage_runs`
- `app_events`

Stage 1 must bootstrap these tables even if not all are fully used yet.

## 3.6. Approved initial screen set
The application should have the following screen-level structure:
- Dashboard
- Entities
- Entity Detail
- Stage Editor
- Workspace Explorer
- Settings / Diagnostics / Workdir Setup

Some of these may be placeholders in Stage 1, but they must exist in the UI architecture.

---

# 4. Technical stack requirements

Codex must implement Stage 1 on the following stack:

## 4.1. Desktop shell
- **Tauri**

## 4.2. Frontend
- **React**
- **TypeScript**

## 4.3. Storage
- **SQLite**

## 4.4. Config format
- **YAML**

## 4.5. Additional expectations
Codex should choose mature, pragmatic libraries rather than exotic ones.

Strict requirements:
- strong typing,
- clean separation of concerns,
- no chaotic temporary hacks,
- no hardcoded OS-specific assumptions beyond what is required by Tauri,
- no “we’ll fix it later” structure that causes future rewrite.

---

# 5. High-level architectural expectations

Codex must create a project foundation with clear boundaries between the following concerns.

## 5.1. UI layer
Responsible for:
- layout,
- navigation,
- screen rendering,
- showing bootstrap/config/database state,
- showing stage list loaded from config.

## 5.2. Domain/model layer
Responsible for:
- shared domain types,
- pipeline config types,
- stage definitions,
- workdir-related state models,
- bootstrap state definitions,
- initialization state representation.

## 5.3. Config layer
Responsible for:
- reading `pipeline.yaml`,
- parsing YAML,
- validating YAML structure,
- exposing validated config to the rest of the app.

## 5.4. Workdir layer
Responsible for:
- opening a workdir,
- creating a new workdir,
- checking required files/directories,
- reporting workdir health.

## 5.5. Database layer
Responsible for:
- opening SQLite,
- initializing schema,
- syncing stages into DB,
- exposing clean bootstrap methods.

## 5.6. App bootstrap/orchestration layer
Responsible for:
- initialization flow,
- error handling during startup,
- coordinating workdir + config + database setup,
- producing final app initialization state.

Codex must not collapse these concerns into a few giant files.

---

# 6. Required implementation outcomes

Codex must implement all items in this section.

## 6.1. Create the desktop project foundation
Initialize a working Tauri + React + TypeScript application.

The project must:
- compile,
- run locally,
- have stable base structure,
- be understandable for future incremental development.

The folder structure does **not** need to match an exact template, but it must clearly separate app shell, features, domain types, infrastructure, and utility code.

A good expected direction would be something structurally similar to:

```text
beehive/
  src/
    app/
    components/
    pages/
    features/
      workdir/
      pipeline-config/
      database/
      diagnostics/
    types/
    lib/
  src-tauri/
  public/
  README.md
```

Codex may choose a different layout if it is clearly superior and well justified.

## 6.2. Build an application shell
Implement a basic application shell with:
- main layout,
- navigation,
- header,
- content area.

The shell must be stable enough to host multiple screens going forward.

A minimal but clean sidebar or top navigation is acceptable.

## 6.3. Implement screens/pages
At minimum, create routable pages or equivalent view structure for:
- Dashboard
- Entities
- Entity Detail
- Stage Editor
- Workspace Explorer
- Settings / Diagnostics / Workdir Setup

Stage 1 does not require completed business logic for all pages.
However:
- the pages must exist,
- navigation to them must work,
- at least some pages must already show real bootstrap data.

## 6.4. Implement workdir opening flow
The user must be able to select or specify a `workdir`.

The application must then:
- verify that the path exists for “open existing workdir”,
- check for `pipeline.yaml`,
- check for `app.db`,
- verify directory structure as needed,
- report status in UI.

## 6.5. Implement new workdir initialization flow
The application must support creating a new workdir.

At minimum, it must create:

```text
/workdir
  /pipeline.yaml
  /app.db
  /stages
  /logs
```

Codex must decide whether `pipeline.yaml` is initialized from a default template. This is strongly recommended.

The generated default config should be valid and readable.

## 6.6. Load YAML config
The application must be able to:
- read `pipeline.yaml`,
- parse it,
- convert it into a typed internal config model,
- expose it to UI and bootstrap logic.

## 6.7. Validate YAML config
Validation must be implemented.

At minimum, validation must check:
- `project` exists,
- `project.name` exists,
- `project.workdir` exists,
- `runtime` exists or defaults are safely applied,
- `stages` is an array,
- stage IDs are unique,
- `input_folder` exists as a config field,
- `workflow_url` exists for executable stages,
- `max_attempts >= 1`,
- `retry_delay_sec >= 0`.

The application must surface validation errors in a usable way in UI.

Validation output should be structured, not just free-form strings.

## 6.8. Implement SQLite bootstrap
The application must:
- create/open `app.db`,
- initialize required schema if missing,
- do so through a dedicated database/bootstrap layer.

Stage 1 must create at minimum these tables:
- `settings`
- `stages`
- `entities`
- `entity_stage_states`
- `stage_runs`
- `app_events`

Codex should include sensible primary keys and minimal structural fields required for Stage 2.

It is acceptable if some tables are only partially used in Stage 1, but the bootstrap must be real.

## 6.9. Synchronize stage definitions into SQLite
Once YAML is loaded and validated, the application must sync stage definitions into the `stages` table.

Required behavior:
- initial insert for new stages,
- update existing stage definitions on reload,
- no duplicate stage records for same stage id.

This does not require a full migration engine, but the behavior must be deterministic and clean.

## 6.10. Build initialization state model
Codex must define and use an explicit initialization state model for the application.

At minimum, the app should distinguish between:
- app not configured,
- workdir selected,
- config loaded,
- config invalid,
- database ready,
- bootstrap failed,
- fully initialized.

The UI must reflect these states clearly.

## 6.11. Expose bootstrap state in the UI
At least the following data should be visible in a meaningful form:
- current project name,
- current workdir path,
- database path,
- config status,
- database status,
- number of stages loaded,
- list of stage IDs.

This is not optional. Stage 1 must provide visible confirmation that the foundation works.

## 6.12. Diagnostics visibility
The app must have a simple diagnostics or settings view showing technical bootstrap information.

At minimum include:
- selected workdir path,
- config file path,
- database file path,
- config validation result,
- last config load timestamp or equivalent,
- stage count.

## 6.13. README
Codex must write a useful `README.md` that explains:
- project purpose,
- stack,
- how to install dependencies,
- how to run the app,
- how to initialize/open a workdir,
- what Stage 1 currently supports,
- what Stage 1 intentionally does not support yet.

---

# 7. Domain typing requirements

Codex must define shared types/interfaces for the core concepts.

At minimum the following typed concepts must exist in a proper place:
- `StageStatus`
- `PipelineConfig`
- `ProjectConfig`
- `RuntimeConfig`
- `StageDefinition`
- `WorkdirState`
- `AppInitializationState`
- `ConfigValidationResult`

If Codex uses schemas (for example schema validation), the schema and TypeScript types must remain aligned.

`any` should be avoided unless absolutely unavoidable.

---

# 8. UI requirements in more detail

Stage 1 UI does not need to look polished, but it must be organized and practical.

## 8.1. App shell
Must include:
- persistent navigation,
- clear current page rendering,
- room for future feature growth.

## 8.2. Workdir setup page
This page must allow the user to:
- choose/open an existing workdir,
- initialize a new one,
- see whether setup succeeded,
- see errors if setup failed.

## 8.3. Dashboard page
The dashboard must show real bootstrap data such as:
- project name,
- workdir path,
- database readiness,
- config validity,
- number of stages,
- list or summary of stages.

## 8.4. Stage Editor page
In Stage 1 this does not need a full editor.
However it must display:
- loaded stage definitions,
- key fields of each stage,
- at least enough information to confirm config parsing and DB sync are working.

## 8.5. Settings/Diagnostics page
This page must show technical setup status.

## 8.6. Placeholder pages
Entities, Entity Detail, and Workspace Explorer may be placeholders, but they must be wired into the app shell and clearly marked as future stages.

---

# 9. Database bootstrap expectations

Codex must implement **real schema bootstrap**, not a fake placeholder.

The exact schema is flexible, but the tables must be present and reasonably shaped for future use.

## 9.1. Minimum suggested table responsibilities

### `settings`
Application-level settings and simple key/value bootstrap metadata.

### `stages`
Persistent record of stage definitions from the pipeline config.

### `entities`
Future persistent record of JSON entities.

### `entity_stage_states`
Future persistent state per entity per stage.

### `stage_runs`
Future history of attempts/runs.

### `app_events`
Application event log / diagnostics log foundation.

## 9.2. Stage table bootstrap expectation
The `stages` table must at minimum be capable of storing:
- stage ID,
- input folder,
- output folder,
- workflow URL,
- max attempts,
- retry delay,
- next stage,
- timestamps if appropriate.

## 9.3. Schema management expectation
Codex does not need to build a full migration engine in Stage 1.
But schema creation must be deterministic, reusable, and not embedded chaotically in UI code.

---

# 10. Error handling expectations

Errors must be handled like an application, not like a prototype script.

Codex must ensure:
- config load errors are visible,
- YAML validation errors are visible,
- SQLite initialization errors are visible,
- workdir creation/open errors are visible,
- the app does not fail silently.

Errors should be structured enough to support future diagnostics improvements.

---

# 11. Forbidden implementation patterns

The following outcomes are considered unacceptable.

Codex must **not**:

1. Put most logic into a few giant files.
2. Hardcode a single path instead of implementing workdir selection/initialization.
3. Skip SQLite bootstrap and claim it will be added later.
4. Ignore YAML validation and accept raw parsed objects blindly.
5. Build a fake UI that does not reflect actual bootstrap state.
6. Mix file-system logic, config parsing, DB bootstrap, and UI rendering into one module.
7. Implement Stage 2 runtime behavior prematurely instead of finishing Stage 1 cleanly.
8. Return a shallow “starter template” with almost no application-specific logic.
9. Hide important architectural choices.
10. Use temporary hacks that make Stage 2 harder.

---

# 12. Expected repository quality

Codex is expected to produce a foundation that a serious programmer can continue from without having to rewrite the initial setup.

This means:
- the structure must be understandable,
- naming must be coherent,
- responsibilities must be reasonably separated,
- the app must actually run,
- the app must already prove that workdir + YAML + SQLite bootstrap work together.

---

# 13. Manual verification scenarios Codex must perform

Codex must test and report the following scenarios:

1. Fresh app launch.
2. Create a new workdir.
3. Confirm that `pipeline.yaml`, `app.db`, `stages/`, `logs/` are created.
4. Load a valid `pipeline.yaml`.
5. Verify that stages are visible in UI.
6. Verify that stage definitions are synced into SQLite.
7. Open an existing workdir.
8. Test invalid config scenario and confirm UI shows meaningful errors.

Codex must explicitly state what was manually verified.

---

# 14. Deliverables Codex must return

Codex must return the work in a structured report.

The response must include the following sections.

## A. What was implemented
A concise but concrete summary.

## B. Files added or changed
List the files or key file groups created/modified.

## C. Architecture choices
Explain major implementation decisions.

## D. How to run
Give exact commands.

## E. What was manually tested
List scenarios actually checked.

## F. What remains for Stage 2
Clearly separate out intentionally deferred work.

---

# 15. Definition of done

Stage 1 is only accepted if all of the following are true:

1. The desktop application runs locally.
2. The application has a stable UI shell with working navigation.
3. A new workdir can be initialized.
4. An existing workdir can be opened.
5. `pipeline.yaml` is loaded and validated.
6. `app.db` is created/opened.
7. SQLite schema bootstrap works.
8. Stage definitions are synced from YAML into SQLite.
9. Dashboard shows real bootstrap data.
10. Stage Editor shows real stage information from config/bootstrap.
11. Diagnostics/Settings shows technical initialization data.
12. README explains setup and launch.
13. The code structure is clean enough for Stage 2.

If these are not all true, Stage 1 is incomplete.

---

# 16. Guidance on implementation judgment

If Codex encounters choices not explicitly specified here, it must choose according to these principles, in order of priority:

1. architectural cleanliness,
2. future extensibility,
3. clear separation of concerns,
4. reliability of desktop/workdir/bootstrap flow,
5. strong typing,
6. transparency of implementation,
7. minimal but real functionality.

Codex should prefer a smaller but correct Stage 1 over a larger but sloppy one.

---

# 17. Final instruction to Codex

Implement Stage 1 of **beehive** as a real application foundation, not as a placeholder demo.

The resulting codebase must prove the following architectural chain works end-to-end:

**desktop app -> workdir -> pipeline.yaml -> validation -> SQLite bootstrap -> stage sync -> UI visibility**

That chain is the core purpose of Stage 1.

Do not drift into runtime orchestration yet. Do Stage 1 properly.
