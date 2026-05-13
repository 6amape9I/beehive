# Multi-Output Lineage

## Source Of Truth

B5 uses the existing SQLite model:

```text
entity_files.producer_run_id
```

Every manifest output registered from one stage run carries the producing `run_id`. This supports one source artifact creating zero, one, or many child artifacts without a new lineage table.

## Read Model

The backend exposes stage-run outputs with:

- `run_id`;
- `output_count`;
- output `entity_id`;
- output `artifact_id`;
- target `stage_id`;
- `relation_to_source`;
- `storage_provider`;
- bucket/key and `s3_uri`;
- checksum/etag/version/size metadata;
- child runtime status.

HTTP-shaped route:

```text
GET /api/workspaces/{workspace_id}/stage-runs/{run_id}/outputs
```

Tauri command:

```text
list_stage_run_outputs(workspace_id, run_id)
```

## UI Behavior

Entity Detail stage-run rows can expand and load all output artifacts for that run. The table shows child entity, artifact, target stage, runtime status, relation, and S3 URI.

Workspace Explorer already shows `producer_run_id` and child S3 pointer rows. B5 adds the run-level expansion so one-to-many branching is visible without relying on a single `created_child_path`.

## Runtime Invariants

The existing runtime invariants remain unchanged:

- source state becomes `done` only after manifest outputs validate and register transactionally;
- child artifacts become `pending`;
- unknown, unsafe, or ambiguous `save_path` blocks the run;
- registration conflicts do not silently mark the source `done`;
- zero-output success requires `allow_empty_outputs=true`.
