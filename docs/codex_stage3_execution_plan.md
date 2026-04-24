# Stage 3 Execution Plan

Authoritative source: `instructions/beehive_stage3_codex_task.md`.

## Summary

- Evolve Stage 2 discovery into Stage 3 reconciliation.
- Introduce logical entities plus physical file instances via `entity_files`.
- Add schema v3 migration, missing/restored file tracking, stage directory provisioning, and managed next-stage copy.
- Keep filesystem behavior non-destructive and preserve historical records.

## Planned phases

1. Create Stage 3 docs and re-read instructions.
2. Implement schema v3, migration, and domain/API model updates.
3. Implement reconciliation scanner with provisioning and missing/restored behavior.
4. Implement safe file operations and managed next-stage copy.
5. Update frontend runtime views for logical entities and file instances.
6. Run technical verification, finalize docs, and report only verified results.
