# Stage 5 Delivery Report

## Summary

Stage 5 implements a read-only operator Dashboard overview on top of the existing Stage 4 runtime foundation.

The Dashboard now shows project/workdir context, summary cards, a static stage graph, per-stage counters, active tasks, recent warning/error events, and recent stage runs. It uses real SQLite runtime data through one backend read model command.

The Stage 5 polish patch fixed Dashboard correctness around graph link semantics, full counter visibility, and queued active task consistency.

## Backend / read model

- Added `dashboard` backend module with `get_dashboard_overview`.
- Added Tauri command `get_dashboard_overview(path)`.
- Added Rust DTOs for dashboard overview, project context, totals, runtime summary, stage graph, stage counters, active tasks, error items, and recent runs.
- Added lightweight dashboard query indexes with `CREATE INDEX IF NOT EXISTS`.
- Kept SQLite `user_version = 4`; no schema v5 migration was added.
- Dashboard read path does not scan files, run tasks, call n8n, or mutate execution state.

## Frontend / UI

- Replaced the Stage 4 summary-only Dashboard with a Stage 5 overview.
- Added typed `getDashboardOverview` wrapper.
- Added dashboard components:
  - `DashboardActions`
  - `SummaryCards`
  - `StageGraph`
  - `StageCountersTable`
  - `ActiveTasksPanel`
  - `LastErrorsPanel`
  - `RecentRunsPanel`
- Operational buttons remain manual:
  - `Refresh`
  - `Scan workspace`
  - `Run due tasks`
  - `Reconcile stuck`
- Each action reloads dashboard overview after completion and shows loading/error feedback.
- StageGraph no longer draws visual arrows between neighboring cards based on array order.
- StageCountersTable now displays all important DTO counters, including total, queued, skipped, unknown, existing files, and missing files.

## Stage graph behavior

- Nodes come from `stages`.
- Edges come from `stages.next_stage`.
- Real edges are shown explicitly in a Stage Links list.
- Terminal stages without `next_stage` are not errors.
- Missing target stages and inactive target stages are shown as link problems.
- Node health is derived from active/inactive state, failed/blocked counters, retry/in-progress counters, and edge problems.

## Counter semantics

- Stage counters are aggregated from `entity_stage_states`, not file JSON status.
- File counts are aggregated from `entity_files`.
- Due tasks are counted from active stages with `pending` or due `retry_wait` states.
- Active tasks include `in_progress`, `queued`, `retry_wait`, and `pending`, limited to 50 rows.
- Last errors are warning/error `app_events`, limited to 20 rows.
- Recent runs come from `stage_runs`, limited to 20 rows.

## Tests

Added Rust dashboard read-model tests for:

- fresh DB / no entities overview;
- valid, missing, and inactive graph edges;
- graph edges independent of stage card order;
- per-stage status counters;
- queued/skipped/unknown counter aggregation;
- active task inclusion including queued, plus done-state exclusion;
- latest warning/error event limits and ordering;
- recent run limits and success/failure fields;
- read-only behavior for execution state.

## Verification commands

- `cargo fmt --manifest-path src-tauri/Cargo.toml`: passed.
- `cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'`: passed, 51 Rust tests.
- `npm.cmd run build`: passed.

## Manual UI testing statement

No mouse-driven UI walkthrough was performed. No screenshots or visual QA are claimed.

## Known limitations

- Dashboard graph is a static readable overview, not a drag-and-drop editor.
- Dashboard does not implement an audit explorer, reset/requeue controls, daemon, scheduler, or n8n REST API integration.
- Dashboard does not auto-scan and does not auto-run tasks on page load.

## Stage 5 acceptance status

Stage 5 is ready for review based on implemented read model, real-data Dashboard wiring, backend aggregation tests, and passing technical verification.
