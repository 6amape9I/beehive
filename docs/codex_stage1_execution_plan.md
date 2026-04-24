# beehive Stage 1 Execution Plan

This file records the implementation plan approved for Stage 1. The authoritative product specification remains `instructions/beehive_stage1_codex_task.md`.

## Core chain

Stage 1 must prove this chain end to end:

`desktop app -> workdir -> pipeline.yaml -> validation -> SQLite bootstrap -> stage sync -> UI visibility`

## Phases

1. Create source-of-truth logs and repository hygiene.
2. Build Tauri v2 + React + TypeScript project foundation.
3. Implement Rust backend boundaries for domain, config, workdir, database, and bootstrap orchestration.
4. Implement typed React shell, routes, workdir setup, dashboard, stage list, and diagnostics.
5. Verify builds, Rust tests, and manual Stage 1 scenarios.
6. Update README and delivery report.

## Operating rules

- `instructions/beehive_stage1_codex_task.md` is the only product source of truth.
- Re-read the specification after each significant implementation phase.
- Keep runtime orchestration, file scanning, retries, task queues, n8n execution, graph routing, and full CRUD editing out of Stage 1.
- Record questions, progress, verification results, and final delivery in markdown files under `docs/`.
