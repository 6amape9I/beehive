# B14 Worker Runtime Control Feedback

## Summary

B14 implemented DB-backed worker runtime control on top of B13 worker leases.

Done:

- Added SQLite schema v12 for `worker_pool_controls.desired_concurrency` and `is_started`.
- Added worker Start/Stop and per-pool desired concurrency API endpoints.
- Added Workspace Explorer controls for default/local LLM desired counts, Start workers, Stop workers, and per-pool Apply desired.
- Kept server env/YAML as worker loop upper bounds while UI desired state controls actual claims.
- Added `runtime.scheduling_policy` with `depth_first` default and `fifo` option.
- Changed worker claim ordering to prefer `pending` before due `retry_wait`.
- Added depth-first ordering for fresh child artifacts via `entity_files.producer_run_id`.
- Raised SQLite busy timeout default to 30000ms and added `BEEHIVE_SQLITE_BUSY_TIMEOUT_MS`.
- Added limited SQLite busy/locked retry helper for critical worker/control writes.
- Kept transient failures retryable and deterministic contract/config failures blocked.
- Added worker runtime runbook and retry policy docs.

## Checkpoints Reread

- `after_plan`
- `after_b13_review`
- `after_worker_runtime_control_design`
- `after_depth_first_scheduler_design`
- `after_sqlite_lock_design`
- `after_retry_policy_design`
- `after_backend_changes`
- `after_ui_changes`
- `after_tests`
- `after_pilot_or_smoke`
- `before_feedback`

## Runtime Control

Start workers writes:

```text
is_started = true
desired_concurrency = requested count capped by env/YAML upper bound
```

Stop workers writes:

```text
is_started = false
```

Stop does not kill running n8n/S3 work. Active leases are allowed to finish and the summary can report `draining`.

Worker claim now requires:

```text
pool is started
pool is not paused
desired_concurrency > active leases
resource_class matches stage resource_class
```

## Scheduling And Retry

`depth_first` prefers:

```text
pending before retry_wait
child artifacts before source artifacts
newer state timestamps before older ones
```

`fifo` prefers older pending work after the same pending-before-retry demotion.

Anti-starvation MVP is the `fifo` switch. No aging quota was added in B14.

Transient execution failures still go to `retry_wait` while attempts remain. Deterministic contract/config failures now block immediately, including invalid JSON/contract response shape and manifest/copy blocking failures.

## SQLite Lock Hardening

Connection setup now applies:

```text
PRAGMA busy_timeout = BEEHIVE_SQLITE_BUSY_TIMEOUT_MS or 30000
```

The retry helper retries only clear busy/locked messages with backoff:

```text
50ms, 100ms, 200ms, 400ms, 800ms
```

No in-process workspace write gate was added. That remains a B15 candidate if real concurrent pilots still show lock pressure.

## Pilot / Smoke

Production pilot was not run.

Reason: B14 does not add subset-limited background workers, and the instruction explicitly says not to run an unbounded `itg_documents` workspace. Running a full workspace would risk accidental broad processing.

Smoke coverage used instead:

- unit/integration tests for stopped claim, desired concurrency cap, resource class filtering, active lease exclusion, pause/resume, depth-first, fifo, and retry demotion;
- HTTP route parsing smoke for summary/start/stop/update/pause/resume/release missing-workspace responses;
- frontend production builds for normal and HTTP-base-url modes.

`database is locked` frequency was not measured against a real concurrent pilot for the same reason. Deterministic tests cover busy timeout parsing and retry/backoff behavior.

## Verification

```text
cargo fmt --manifest-path src-tauri/Cargo.toml
exit 0
```

```text
cargo test --manifest-path src-tauri/Cargo.toml
exit 0
212 passed; 0 failed; 3 ignored; doc tests passed.
```

```text
npm run build
exit 0
tsc and vite build passed; 88 modules transformed.
```

```text
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
exit 0
tsc and vite build passed; 88 modules transformed.
```

```text
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
exit 0
no output
```

```text
rg "@tauri-apps/api/core|invoke\(" src -n
exit 0
src/lib/apiClient/tauriClient.ts:1:import { invoke } from "@tauri-apps/api/core";
```

```text
git diff --check
exit 0
no output
```

## Residual Risks

- Depth-first starvation is controlled by switching to `fifo`, not by an aging quota.
- Real concurrent SQLite lock frequency still needs a bounded pilot on a dedicated or subset-limited workspace.
- B14 controls claims but does not dynamically spawn/retire worker threads beyond the server-level env/YAML supervisor upper bounds.
