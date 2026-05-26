# Worker Retry Policy

## Default Behavior

Beehive keeps retries bounded by each stage `max_attempts`.

When attempts remain, transient failures move the stage state to:

```text
retry_wait
```

When attempts are exhausted, they move to:

```text
failed
```

Deterministic contract/config failures move directly to:

```text
blocked
```

## Retryable Failures

Retryable failures include:

- network errors
- n8n request timeout
- HTTP 5xx from workflow execution
- local LLM timeout or overload surfaced through the workflow
- temporary S3 operation failure
- temporary worker execution failure
- SQLite busy/locked errors when the retry helper succeeds before the task outcome is written

Current backoff uses the stage `retry_delay_sec`. Due retry rows are demoted behind normal pending work during worker claim.

## Blocking Failures

Blocking failures include:

- invalid JSON response shape
- contract response shape errors
- invalid manifest root or schema
- S3 workspace/run/source mismatch
- unsafe `save_path`
- unknown `save_path` route
- output cardinality violation when stage config forbids zero or multiple outputs
- missing required S3 metadata
- inactive or missing next stage
- empty workflow URL or invalid stage config

These failures usually need a workflow, stage config, or operator fix. Retrying the same payload without a fix is expected to waste worker capacity.

## Empty Outputs

Zero outputs are success only when the stage allows them.

For S3 manifest stages:

```text
allow_zero_outputs=true  => zero outputs can succeed
allow_zero_outputs=false => zero outputs are blocked
```

The deprecated `allow_empty_outputs` alias is still parsed for compatibility, but new docs and UI should use `allow_zero_outputs` wording.

## Queue Ordering

Worker claim always prefers:

```text
pending
due retry_wait
```

Then it applies the configured scheduling policy:

- `depth_first`: child artifacts and recent state first
- `fifo`: older state first

## Manual Recovery

After fixing a workflow or stage config, use manual retry/reset controls:

- retry/reset selected failed or blocked rows from the explorer
- retry/reset an entity/stage from entity detail
- recover expired leases if a worker died mid-run

Manual actions write app events with operator-visible reason/comment where the existing action supports it.

## Remaining Risk

B14 does not implement exponential retry backoff or a full starvation-prevention quota. If depth-first starves old source rows during a long run, switch the workspace config to `fifo` for bulk completion or add an aging/quota scheduler in a follow-up.
