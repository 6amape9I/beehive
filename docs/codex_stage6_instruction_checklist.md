# Stage 6 Instruction Checklist

Source of truth: `instructions/beehive_stage6_codex_task.md`.

## Source Review

- [x] Stage 6 instruction re-read before implementation.
- [x] README and current backend/frontend code reviewed.
- [x] Prior docs index and accepted orphan `stage_runs` follow-up reviewed.
- [x] Stage 6 instruction re-read after backend/frontend contract implementation.
- [x] Stage 6 instruction re-read before final report.

## Backend Read Models

- [x] `list_entities` uses SQLite filtering/search/sorting/pagination.
- [x] Entity table rows include attempts, max attempts, last error, HTTP status, retry timestamp, and timestamps.
- [x] Entity table query avoids payload/meta JSON loading.
- [x] `get_entity` returns files, stage states, stage runs, timeline, selected JSON, and allowed actions.
- [x] Timeline is ordered by pipeline graph order where possible and includes historical/inactive states with rows.

## Manual Actions

- [x] Backend exposes retry now, reset to pending, and skip commands.
- [x] Manual action rules come from backend allowed-actions DTO.
- [x] Manual action state changes use the state machine wrapper.
- [x] Manual actions write app events.
- [x] Stage run history is preserved after reset.

## File / JSON Operations

- [x] Open file command resolves registered file path safely.
- [x] Open folder command resolves registered folder safely.
- [x] JSON save edits only business payload/meta.
- [x] JSON save checks disk snapshot before atomic write.
- [x] JSON save does not overwrite SQLite runtime state from JSON status.

## Frontend

- [x] Entities table supports search, filters, sort, pagination, refresh, and clear filters.
- [x] Entities rows navigate to Entity Detail.
- [x] Entity Detail shows header, timeline, file instances, JSON viewer/editor, run history, actions, and diagnostics.
- [x] Loading, empty, and error states are present.

## Tests And Verification

- [x] Rust tests cover entity table read model.
- [x] Rust tests cover entity detail payload.
- [x] Rust tests cover manual actions.
- [x] Rust tests cover JSON save safety.
- [x] Rust tests cover safe open path resolution.
- [x] `cargo fmt --manifest-path src-tauri/Cargo.toml` passed.
- [x] Rust tests through `vcvars64.bat` passed: 74 passed.
- [x] `npm.cmd run build` passed.
- [x] No real n8n endpoint used by tests.
- [x] No full manual UI walkthrough claimed.

## Notes

- Manual retry for `failed` / `blocked` is intentionally not combined with reset. The UI/backend expose Reset first, then Retry/Run when the state is pending.
- JSON editing is intentionally scoped to `payload` and `meta`; no rich diff/version history was implemented.
- OS-level open itself is not mouse-driven UI QA; automated tests cover registered path resolution and unknown file id rejection.

