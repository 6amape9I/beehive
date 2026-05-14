# B7 Real Web Pilot Report

## Pilot Status

Result: passed through scripted HTTP web path.

Browser UI was not manually clicked. The built browser frontend was served by `beehive-server` and verified via HTTP, and the operator workflow was exercised through the same HTTP API used by the browser.

Screenshots: omitted.

## Workspace

```text
workspace_id = b7-smoke
server_url = http://127.0.0.1:8788
registry = /tmp/beehive-b7-web-pilot/workspaces.yaml
workdir = /tmp/beehive-b7-web-pilot/workdir
```

## n8n Webhook

Used:

```text
https://n8n-dev.steos.io/webhook/beehive-s3-pointer-smoke-body
```

No secrets are included in this report.

## Source Artifacts

S3 reconcile through HTTP registered:

```text
listed_object_count = 68
metadata_tagged_count = 50
registered_file_count = 50
unmapped_object_count = 18
```

Selected source:

```text
entity_file_id = 1
entity_id = smoke_entity_001
stage_id = smoke_source
source_key = beehive-smoke/test_workflow/raw/smoke_entity_001__порфирия.json
```

## Selected Run

Request:

```text
POST /api/workspaces/b7-smoke/run-selected-pipeline-waves
root_entity_file_ids = [1]
max_waves = 1
max_tasks_per_wave = 1
stop_on_first_failure = true
```

Result:

```text
run_id = b4f89c99-1900-4ccd-bffa-f994bdf6092b
waves_executed = 1
total_claimed = 1
total_succeeded = 1
total_failed = 0
total_blocked = 0
stopped_reason = max_waves_reached
```

## Output Artifact

Output key:

```text
beehive-smoke/test_workflow/processed/smoke-output-b4f89c99-1900-4ccd-bffa-f994bdf6092b.json
```

S3 check:

```text
2026-05-14 07:52:23  2186  smoke-output-b4f89c99-1900-4ccd-bffa-f994bdf6092b.json
```

## Final States

SQLite state evidence:

```text
smoke_entity_001 | smoke_source    | done    | file_exists=1
smoke_entity_001 | smoke_processed | pending | file_exists=1
```

Entity file evidence:

```text
1  | smoke_entity_001 | smoke_source_artifact_001 | smoke_source    | s3 | steos-s3-data | beehive-smoke/test_workflow/raw/smoke_entity_001__порфирия.json | producer_run_id=
51 | smoke_entity_001 | smoke-output-b4f89c99-1900-4ccd-bffa-f994bdf6092b | smoke_processed | s3 | steos-s3-data | beehive-smoke/test_workflow/processed/smoke-output-b4f89c99-1900-4ccd-bffa-f994bdf6092b.json | producer_run_id=b4f89c99-1900-4ccd-bffa-f994bdf6092b
```

Stage run evidence:

```text
b4f89c99-1900-4ccd-bffa-f994bdf6092b | smoke_entity_001 | smoke_source | success=1 | http_status=200 | error_type=
```

## Stage-Run Outputs Endpoint

```text
GET /api/workspaces/b7-smoke/stage-runs/b4f89c99-1900-4ccd-bffa-f994bdf6092b/outputs
```

Returned:

```json
{
  "errors": [],
  "payload": {
    "run_id": "b4f89c99-1900-4ccd-bffa-f994bdf6092b",
    "output_count": 1,
    "outputs": [
      {
        "entity_file_id": 51,
        "entity_id": "smoke_entity_001",
        "artifact_id": "smoke-output-b4f89c99-1900-4ccd-bffa-f994bdf6092b",
        "target_stage_id": "smoke_processed",
        "relation_to_source": "same_entity",
        "storage_provider": "s3",
        "bucket": "steos-s3-data",
        "key": "beehive-smoke/test_workflow/processed/smoke-output-b4f89c99-1900-4ccd-bffa-f994bdf6092b.json",
        "s3_uri": "s3://steos-s3-data/beehive-smoke/test_workflow/processed/smoke-output-b4f89c99-1900-4ccd-bffa-f994bdf6092b.json",
        "runtime_status": "pending",
        "producer_run_id": "b4f89c99-1900-4ccd-bffa-f994bdf6092b"
      }
    ]
  }
}
```

## External Systems

```text
S3 contacted = yes
n8n contacted = yes
S3 output exists = yes
browser UI manually inspected = no
```

## Blockers

No blocker for the B7 single-source selected web pilot.

The pilot intentionally used `max_waves = 1` because the smoke pipeline's `smoke_processed` stage is a terminal placeholder with a non-production local workflow URL. The B7 proof is the selected source execution and child pointer creation through the web path.
