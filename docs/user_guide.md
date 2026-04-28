# beehive User Guide

beehive is a local desktop operator tool for JSON stage pipelines. A workdir contains `pipeline.yaml`, `app.db`, stage folders, logs, and JSON entity files.

## Core Rules

- SQLite is the runtime source of truth for execution state.
- JSON files hold business payload and metadata.
- Do not manually edit root service fields such as `id`, `current_stage`, `next_stage`, `status`, or `meta.beehive`.
- Use `Scan workspace` after adding, restoring, or changing files.
- n8n execution is manual through `Run due tasks` or debug/manual entity actions.

## Workdir

Open or initialize an absolute path outside the app directory. The app rejects relative paths and disguised nested paths.

Required files/folders:

- `pipeline.yaml`
- `app.db`
- `stages/`
- `logs/`

## Pipeline

`pipeline.yaml` defines project, runtime, and stages. Non-terminal stages require an `output_folder`; terminal stages with no `next_stage` may omit it.

Use Stage Editor to validate and save pipeline changes. Saves are atomic and create a backup.

## Scan Workspace

Scan is explicit. It provisions active stage folders, registers valid JSON files, marks missing files, restores returned files, records invalid files, and does not overwrite SQLite execution state from JSON `status`.

## Run Due Tasks

`Run due tasks` claims eligible `pending` or due `retry_wait` states, sends source payload/meta to stage `workflow_url`, records `stage_runs`, and updates SQLite state.

Failures become `retry_wait` while attempts remain or `failed` when exhausted. Structural next-stage copy blocks become `blocked` and are not retried.

Stage 9 supports n8n responses as root arrays or wrapper payload arrays. Each output object becomes a child JSON file in the next stage.

## Dashboard

Dashboard is a read-only overview with manual action buttons. It does not scan or run n8n automatically.

## Entities

Entities table supports server-side search, filters, sorting, and pagination. Use it to find logical entities and inspect attempts, status, and last errors. Search covers entity ID, file path/name, and latest payload text such as the demo business name. If a freshly opened workdir has no rows yet, run `Scan workspace` from Dashboard, Workspace Explorer, or Entities.

## Entity Detail

Entity Detail shows file instances, stage timeline, run history, selected JSON preview/editor, and backend-computed allowed actions.

Business JSON editing is allowed only for `pending`, `retry_wait`, `failed`, `blocked`, and `skipped`. It is blocked for `queued`, `in_progress`, and `done`.

## Workspace Explorer

Workspace Explorer is read-only. It shows stage folders, registered files, missing/invalid artifacts, managed copies, and entity trails. Open file/folder actions are backend-validated.

## Errors

Recent errors are visible in Dashboard and Diagnostics. For technical errors, check app events and stage run history before editing files manually.
