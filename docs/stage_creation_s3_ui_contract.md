# S3 Stage Creation UI Contract

## Request

```json
{
  "stage_id": "semantic_rich",
  "workflow_url": "https://n8n.example/webhook/semantic_rich",
  "max_attempts": 3,
  "retry_delay_sec": 30,
  "allow_zero_outputs": false,
  "allow_multiple_outputs": false
}
```

Only `stage_id` and `workflow_url` are required. The UI asks for stage identity, production n8n webhook URL, retry settings, whether zero-output success is allowed, and whether multiple outputs are allowed.

The legacy `allow_empty_outputs` request field is still accepted as an alias for `allow_zero_outputs`, but new UI should send `allow_zero_outputs`.

## Backend Validation

Backend rejects:

- empty or unsafe stage IDs;
- duplicate stage IDs in active config/history;
- non-HTTP workflow URLs;
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

`next_stage` links are deprecated for the current S3 operator contract. n8n routes outputs by returning manifest outputs with target stage `save_path` aliases.

The old link route remains as a compatibility endpoint:

```text
POST /api/workspaces/{workspace_id}/stages/{stage_id}/next-stage
```

It returns `next_stage_deprecated` and does not update the pipeline.
