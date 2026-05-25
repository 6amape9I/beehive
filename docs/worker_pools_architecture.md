# Worker Pools Architecture

## Purpose

Beehive owns workflow concurrency because it is the control-plane that knows workspaces, stages, artifacts, runtime state, retries, blocked states, and lineage. n8n remains the data-plane: it receives one execution request, runs the workflow, writes outputs, and returns a manifest.

B11 adds the resource model that B12 can use for DB-backed workers:

```yaml
stages:
  - id: semantic_enrichment
    resource_class: local_llm

runtime:
  worker_pools:
    default:
      concurrency: 10
    local_llm:
      concurrency: 1
```

No background worker loops are introduced in B11.

## Resource Classes

`stage.resource_class` describes which worker pool should execute a stage later.

Supported B11 values:

- `default`: normal workflow execution.
- `local_llm`: workflow execution that calls a local LLM and must be protected by a separate concurrency limit.

If `resource_class` is missing from an old `pipeline.yaml`, Beehive reads the stage as `default`. Unknown values are rejected during config validation.

The operator UI does not expose the raw enum. It shows:

```text
[ ] Использует локальную LLM
```

Mapping:

```text
unchecked -> resource_class = default
checked   -> resource_class = local_llm
```

## Worker Pools

`runtime.worker_pools` declares the future concurrency limit per resource class.

Default B11 config when the section is absent:

```yaml
runtime:
  worker_pools:
    default:
      concurrency: 1
    local_llm:
      concurrency: 1
```

Validation rules:

- known pools only: `default`, `local_llm`;
- missing known pool gets default concurrency `1`;
- `concurrency` accepts `0..=128`;
- `0` means the pool can be disabled by a future worker layer.

Existing selected/manual run paths keep their current behavior in B11. They preserve stage resource metadata, but they do not enforce worker pool concurrency yet.

## Why Not RabbitMQ Or Kafka Yet

B11-B14 use internal DB-backed worker pools first. This keeps the deployment small while Beehive proves the lease/retry/backpressure model against real workloads.

RabbitMQ remains the first likely external broker candidate if DB-backed queues become a bottleneck because it matches work-queue semantics: ack/nack, requeue, prefetch, dead-lettering, and separate queues by resource class.

Kafka is not used for this track now. It is strong as an event log or streaming platform, but it is heavier than needed for one task claimed by one worker with retry/dead-letter behavior.

## B12 Direction

B12 should build on B11 with:

- task claim and `lease_until`;
- heartbeat lease extension;
- success/failure lease release;
- recovery when a worker dies and the lease expires;
- no double-claim tests;
- pool-specific claim limits using `stage.resource_class` and `runtime.worker_pools`.

## Limitation

Beehive can limit only workflow executions that it starts. If one n8n execution internally launches multiple parallel local LLM calls, Beehive still sees that as one `local_llm` task.

Workflow authoring rule:

```text
Stage with resource_class=local_llm must not parallelize local LLM calls inside one n8n execution.
```
