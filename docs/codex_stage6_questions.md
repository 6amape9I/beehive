# Stage 6 Questions And Decisions

Source of truth: `instructions/beehive_stage6_codex_task.md`.

## Manual retry for failed/blocked

- Context: The instruction allows combined reset+retry for `failed` / `blocked` only if it is safe; otherwise the UI should offer Reset first.
- Decision: Use the conservative path. `Retry now` is available for `pending` and `retry_wait`; `failed` and `blocked` must be reset to pending first.
- Reason: This keeps operator intent explicit and avoids hiding structural blocked/failure context behind a combined action.

## Skip status scope

- Context: The instruction requires at least `pending -> skipped` and permits more statuses if semantics are clear.
- Decision: Allow skip for `pending` and `retry_wait`; reject `done`, `queued`, `in_progress`, `failed`, and `blocked`.
- Reason: Skipping `retry_wait` is a clear operator decision to stop a delayed retry. Skipping failed/blocked would risk hiding important diagnostics.

## JSON editing scope

- Context: Stage 6 should edit business JSON safely and must not allow silent runtime state corruption.
- Decision: Expose backend-mediated editing for `payload` and `meta` only.
- Reason: `id`, stage, status, attempts, retries, and runtime state remain controlled by scanner/runtime/database logic.

