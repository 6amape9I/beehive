# n8n Contract for Beehive Runtime

## Beehive input artifact

Files placed into an active stage input folder are Beehive artifacts. The root JSON object must keep runtime fields and business payload separate:

```json
{
  "id": "raw-001",
  "current_stage": "raw",
  "next_stage": "raw_entities",
  "status": "pending",
  "payload": {
    "raw_text": "Text for n8n",
    "source_name": "manual-smoke"
  },
  "meta": {
    "operator_note": "local test"
  }
}
```

`Scan workspace` registers this file in SQLite. The source file is not mutated during execution.

## Payload-only HTTP request

When `Run due tasks` executes a stage, Beehive sends only the parsed `payload` value to the stage `workflow_url`:

```json
{
  "raw_text": "Text for n8n",
  "source_name": "manual-smoke"
}
```

Beehive does not send these runtime fields to n8n:

```text
entity_id
stage_id
entity_file_id
attempt
run_id
meta.beehive
```

The runtime metadata remains in SQLite tables such as `entity_files`, `entity_stage_states`, `stage_runs`, and `app_events`. `stage_runs.request_json` stores the actual payload-only request body.

## Supported n8n responses

Preferred root array:

```json
[
  {
    "entity_name": "castle",
    "save_path": "main_dir/processed/raw_entities"
  },
  {
    "target_entity_name": "mobile phone",
    "save_path": "main_dir/processed/raw_representations"
  }
]
```

Existing wrapper object:

```json
{
  "success": true,
  "payload": [
    {
      "entity_name": "castle",
      "save_path": "main_dir/processed/raw_entities"
    }
  ],
  "meta": {
    "workflow": "mock"
  }
}
```

Single direct business object:

```json
{
  "entity_name": "castle",
  "save_path": "main_dir/processed/raw_entities"
}
```

Wrapper error response:

```json
{
  "success": false,
  "message": "business validation failed"
}
```

`success: false`, invalid JSON, non-object output items, or invalid wrapper payload types fail the execution attempt.

## save_path routing

For each output object, Beehive resolves `save_path` against active stage `input_folder` values after safe logical normalization.

Example active stages:

```yaml
stages:
  - id: raw_entities
    input_folder: main_dir/processed/raw_entities
  - id: raw_representations
    input_folder: main_dir/processed/raw_representations
```

Valid routes:

```text
save_path main_dir/processed/raw_entities -> stage raw_entities
save_path main_dir/processed/raw_representations -> stage raw_representations
```

When a route is found, Beehive writes a Beehive-wrapped child artifact into:

```text
workdir / matched_stage.input_folder / generated-child-id.json
```

The target artifact keeps the n8n output object as `payload`, including `save_path` if n8n returned it. Local trace metadata is written under `meta.beehive`, but it will not be sent back to n8n on later execution because requests are payload-only.

## Safe path rules

Accepted:

```text
main_dir/processed/raw_entities
stages/raw_entities
```

Rejected:

```text
../outside
/etc/passwd
C:\Users\bad\file
\\server\share
empty string
```

Beehive never writes to the raw `save_path`. It only writes under the selected workdir and only when the normalized `save_path` matches an active stage input folder.

## Legacy /main_dir compatibility

Legacy workflow values that start with `/main_dir/...` are treated as logical Beehive paths, not OS absolute paths.

```text
/main_dir/processed/raw_entities
```

is normalized for matching as:

```text
main_dir/processed/raw_entities
```

Other leading-slash paths, such as `/etc/passwd`, are rejected.

## next_stage fallback

If an output object has no `save_path`, Beehive uses the existing `next_stage` fallback:

```text
output item has save_path -> route by save_path
output item has no save_path and source stage has next_stage -> route to next_stage input_folder
output item has no save_path and source stage has no next_stage -> blocked route error
terminal stage returns no output -> success without target files
```

This keeps older n8n workflows working while allowing new workflows to branch with `save_path`.

## Blocked routes

If any output item has an unsafe, unknown, ambiguous, or non-string `save_path`, Beehive does not write target files for that response. The source stage state becomes `blocked`, the `stage_run` is finished with `success = false`, and `app_events` records the route problem.

B0 uses all-or-nothing planning for response artifacts: route and collision checks happen before target files are written.

## Manual smoke test flow

1. Configure `pipeline.yaml` with one source stage and one or more target stages. Target `input_folder` values must be relative paths inside the workdir.
2. Put a Beehive artifact JSON file into the source stage input folder.
3. Run `Scan workspace`.
4. Configure the source stage `workflow_url` to a local mock webhook or a real n8n webhook.
5. Return either a root array, wrapper payload, or direct object response with `save_path`.
6. Run `Run due tasks`.
7. Verify that `stage_runs.request_json` contains only the source `payload`.
8. Verify that target files were created under matched active stage input folders and registered as pending target stage states.
