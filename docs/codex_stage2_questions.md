# Stage 2 Questions And Resolutions

## 2026-04-24

- Question: Which Stage 2 defaults should be locked immediately where the instruction file offers a preferred option?
- Resolution:
  - non-recursive active-stage scanning;
  - required JSON `id`;
  - folder stage authoritative for discovery;
  - duplicate `entity_id` at a different path is rejected and logged;
  - invalid files are surfaced through `app_events`;
  - manual scan only in Stage 2.

- Question: How should removed YAML stages behave once Stage 2 introduces entity/state history?
- Resolution: Keep stage rows stable and mark removed stages inactive instead of deleting them. `stages` now carries `is_active`, `archived_at`, and `last_seen_in_config_at`, and inactive stages are excluded from discovery.

- Question: How should the app handle path normalization edge cases on Windows while still preventing workdirs under the application directory?
- Resolution: Resolve existing paths via canonicalization, resolve non-existing paths through the nearest canonical parent plus normalized remaining segments, reject relative paths, and compare normalized canonical paths against the application directory.

No unresolved Stage 2 product questions are currently blocking implementation.
