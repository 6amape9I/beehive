# Stage 2 Execution Plan

Authoritative source: `instructions/beehive_stage2_codex_task.md`

## Summary

- Implement Stage 2 as a read-only runtime foundation on top of Stage 1.
- Keep boundaries intact: `Tauri command -> orchestration/service -> discovery/database/workdir modules -> typed results -> React wrappers/pages`.
- Re-read the Stage 2 instruction file and this plan after each meaningful substage, then append the result to the Stage 2 progress/checklist docs.

## Planned implementation sequence

1. Create Stage 2 docs and re-check current Stage 1 code.
2. Implement schema v2 bootstrap/migration, safer stage lifecycle, and stronger workdir validation.
3. Implement discovery, entity registration, app events, and typed Stage 2 commands.
4. Implement frontend runtime views, manual scan flow, and entity/stage/explorer pages.
5. Run build/tests, perform manual verification, and finalize docs/reporting.

## Defaults locked in

- Non-recursive scan of active stage input folders.
- `id` is required; no generated IDs.
- Folder stage is authoritative for discovery.
- Duplicate entity IDs at different paths are rejected and logged.
- Invalid files are represented through `app_events`.
- Manual scan only in Stage 2.
