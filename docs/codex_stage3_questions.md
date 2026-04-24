# Stage 3 Questions and Decisions

## 2026-04-24

### Decision: one file instance per `entity_id + stage_id`

- Source: `instructions/beehive_stage3_codex_task.md`
- Chosen default: enforce at most one physical file instance per logical entity in a given stage.
- Rationale: this matches the Stage 3 duplicate rule, keeps reconciliation deterministic, and avoids silently choosing between multiple files in the same stage.

### Decision: compatible existing managed copy returns `already_exists`

- Source: `instructions/beehive_stage3_codex_task.md`
- Chosen default: if a target file already exists for the same logical entity and target stage, and its managed-copy metadata is semantically compatible, the copy operation returns `already_exists` instead of trying to overwrite it.
- Rationale: repeated copy calls must stay deterministic and non-destructive even though `meta.updated_at` and `meta.beehive.copy_created_at` would otherwise make raw checksums differ between attempts.
