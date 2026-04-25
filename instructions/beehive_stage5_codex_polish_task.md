# beehive — Stage 5 Polish Task

Stage 5 is mostly accepted, but before closing it we need a small polish patch focused on Dashboard correctness.

Do not implement new product features.
Do not redesign the whole UI.
Do not perform full manual UI walkthrough.
Use only automated logic tests, frontend build/typecheck, and minimal UI smoke if convenient.

## Context

Stage 5 introduced the operator-level Dashboard:

- backend dashboard read model;
- `get_dashboard_overview` command;
- summary cards;
- stage graph;
- stage counters;
- active tasks;
- last errors;
- recent runs;
- operational actions.

The implementation is good overall, but there are three correctness issues that must be fixed before Stage 5 closure.

---

## Required fixes

### 1. Fix StageGraph visual semantics

Current problem:

The backend returns real graph edges from `stages.next_stage`, but the frontend StageGraph visually renders arrows between neighboring stage cards based on array order.

This can mislead the operator.

Example:

```text
A -> C
B -> D
C -> terminal
````

The UI must not visually imply:

```text
A -> B -> C -> D
```

Required behavior:

* StageGraph must represent real edges from `overview.stage_graph.edges`.
* Do not draw arrows merely because two cards are adjacent.
* Terminal stages with no `next_stage` should be shown as terminal, not as broken.
* Invalid/missing/inactive target edges must be visible.
* If visual line rendering is too much for this polish pass, implement a clear explicit edge list inside the graph block:

```text
A → C
B → D
C → terminal
```

Preferred simple solution:

* Keep stage cards.
* Remove fake adjacent arrows.
* Add a real “Stage links” / “Edges” section that lists actual edges.
* For each edge show:

  * source stage;
  * target stage;
  * valid/invalid badge;
  * problem text if any.

Acceptance:

* No fake arrows based only on card order.
* Real `edges` are visible.
* Invalid edges are visually distinguishable.

---

### 2. Expand StageCountersTable to show all important counters

Current problem:

The DTO includes more counters than the table displays. Dashboard should show stage-level status counters clearly.

Required columns:

* Stage
* Active
* Total
* Pending
* Queued
* In progress
* Retry
* Done
* Failed
* Blocked
* Skipped
* Unknown
* Existing files
* Missing files
* Last activity

If the table becomes wide, use compact labels, but do not hide diagnostically important counters like `unknown`, `queued`, or `missing_files`.

Acceptance:

* All important status counters from the DTO are visible.
* `unknown` is visible.
* `queued` is visible.
* `total` is visible.
* `existing_files` and `missing_files` are visible.
* Empty/zero values render cleanly.

---

### 3. Synchronize `active_tasks_total` with ActiveTasksPanel

Current problem:

Backend totals count active statuses including `queued`, but ActiveTasksPanel does not list `queued`.

Required behavior:

Choose one consistent rule.

Preferred:

* Include `queued` in ActiveTasksPanel query/list.
* Keep `active_tasks_total` counting:

  * `pending`
  * `queued`
  * `in_progress`
  * `retry_wait`

ActiveTasksPanel should display `queued` rows if they exist.

Acceptance:

* If there are queued tasks, they appear in ActiveTasksPanel.
* Summary count and list definition no longer disagree.
* Add or update backend test to cover queued task visibility.

---

## Tests

Add/update tests where relevant.

Required backend/read-model tests:

1. StageGraph does not rely on node order:

   * create stages where order differs from `next_stage`;
   * verify returned edges represent actual `next_stage` links.

2. Invalid edge visibility:

   * missing target stage returns invalid edge with problem text;
   * inactive target stage returns invalid or warning edge with problem text.

3. Active tasks includes queued:

   * create `queued` state;
   * verify it is included in active task list;
   * verify total count and list semantics are consistent.

4. Stage counters include queued/skipped/unknown if such statuses exist in DB/domain:

   * verify counts are correctly aggregated.

Frontend:

* Do not introduce heavy frontend test framework if not already present.
* TypeScript build must pass.
* Component-level tests are optional only if existing infrastructure already supports them.

---

## Verification commands

Run and document exact commands/results:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
```

On Windows/MSVC:

```cmd
cmd.exe /c "call \"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat\" >nul && cargo test --manifest-path src-tauri\Cargo.toml"
```

Frontend:

```bash
npm run build
```

Do not perform mouse-driven manual UI testing. Minimal app-start smoke is enough if convenient.

---

## Documentation updates

Update Stage 5 docs:

* `docs/codex_stage5_delivery_report.md`
* `docs/codex_stage5_instruction_checklist.md`
* `docs/codex_stage5_progress.md`

Docs must mention:

* StageGraph no longer draws fake order-based arrows;
* StageCountersTable now exposes full counters;
* queued tasks are included consistently;
* verification commands and results;
* no full manual UI walkthrough was performed.

---

## Out of scope

Do not implement:

* graph editor;
* drag-and-drop stages;
* React Flow unless already added and justified;
* new execution engine;
* background worker;
* reset/requeue functionality;
* large UI redesign;
* n8n integration changes.

---

## Expected final report

Return a concise summary:

```md
# Stage 5 Polish Delivery

## Fixed
...

## Files changed
...

## Tests added/updated
...

## Verification
...

## Known limitations
...

## Stage 5 closure recommendation
...
```

Stage 5 can be closed only after these three polish items are fixed and the standard verification commands pass.
