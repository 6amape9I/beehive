# S3 Stage Creation UI Contract

## Request

```json
{
  "stage_id": "semantic_rich",
  "workflow_url": "https://n8n.example/webhook/semantic_rich",
  "next_stage": "weight_entity",
  "max_attempts": 3,
  "retry_delay_sec": 30,
  "allow_empty_outputs": false
}
```

Only `stage_id` and `workflow_url` are required. The UI asks for stage identity, production n8n webhook URL, optional next stage, retry settings, and whether zero-output success is allowed.

## Backend Validation

Backend rejects:

- empty or unsafe stage IDs;
- duplicate stage IDs in active config/history;
- non-HTTP workflow URLs;
- unknown `next_stage`;
- non-S3 workspaces;
- pipeline storage that conflicts with the workspace registry.

## Generated Routes

For workspace:

```text
bucket = steos-s3-data
workspace_prefix = beehive-smoke/test_workflow
stage_id = semantic_rich
```

Backend generates:

```text
input_uri = s3://steos-s3-data/beehive-smoke/test_workflow/stages/semantic_rich
```

and save path aliases:

```text
beehive-smoke/test_workflow/stages/semantic_rich
/beehive-smoke/test_workflow/stages/semantic_rich
s3://steos-s3-data/beehive-smoke/test_workflow/stages/semantic_rich
```

n8n must return manifest outputs with one of the target stage save path aliases. Beehive still validates that output keys are inside the resolved target prefix.

## Persistence

Stage creation writes `pipeline.yaml` atomically, keeps a backup when replacing an existing file, and syncs SQLite stages with the updated config. It does not create local directories for S3 stage routes.

## Connecting Existing Stages

B6 adds a separate link action so stages can be created in any order:

```text
POST /api/workspaces/{workspace_id}/stages/{stage_id}/next-stage
```

Request:

```json
{
  "next_stage": "target_stage_id"
}
```

Clear terminal state:

```json
{
  "next_stage": null
}
```

Backend validates that the source stage exists, the target exists when provided, and source/target are not the same stage. The operation updates `pipeline.yaml` atomically and syncs SQLite.
