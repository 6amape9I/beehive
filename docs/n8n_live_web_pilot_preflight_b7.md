# n8n Live Web Pilot Preflight B7

This is a practical preflight for the B7 web-operator pilot. It is not a full n8n workflow validator and it does not add production workflow JSON to the repository.

## Checked Repository Fixtures

Checked:

```text
docs/n8n_workflows/beehive_s3_pointer_smoke_body_json.json
docs/n8n_web_mvp_preflight_b6.md
docs/n8n_s3_pointer_workflow_adapter.md
docs/n8n_live_workflow_preflight_b4.md
```

## B7 Pilot Checklist

- Webhook method must be `POST`.
- Webhook `responseMode` must be `responseNode`.
- Workflow must read the JSON body control envelope.
- `source_bucket` must come from body.
- `source_key` must come from body.
- Workflow must not use `X-Beehive-Source-Key` or other source-key headers for source selection.
- Workflow must not reference old node names such as `Read Beehive headers`.
- Workflow must not use Search/List Bucket as production source selection.
- Workflow must not contain `/main_dir/pocessed`.
- Manifest schema must be `beehive.s3_artifact_manifest.v1`.
- Manifest outputs must include `artifact_id`, `entity_id`, `relation_to_source`, `bucket`, `key`, and `save_path`.
- Output `save_path` must match a Beehive stage alias.

## Findings From Repo Fixtures

The body-JSON smoke fixture uses:

```text
httpMethod = POST
responseMode = responseNode
bucketName = $json.body.source_bucket
fileKey = $json.body.source_key
save_path = $json.body.save_path
```

Search results found no active `X-Beehive-Source-Key`, `Read Beehive headers`, or `/main_dir/pocessed` references in the checked smoke fixture.

Important limitation: the checked repository smoke fixture is a small contract example, not a committed production workflow. Before real B7 pilot execution, the live n8n workflow must be inspected in n8n to confirm the actual response manifest includes an `outputs[]` array with all required output fields.

## Known Risks To Check In Live n8n

- Some smoke workflow variants can still have a Code node reading `$('Read Beehive headers')` while the graph already uses body/Edit Fields.
- Some semantic workflow variants can still contain `Manual Trigger -> Search bucket -> Download file` demo branches.
- Some older workflow variants can still use `/main_dir/pocessed`.
- Some workflows return a partial manifest object through Set nodes but omit `outputs[]` fields required by Beehive.
- n8n S3 credentials must be able to download exactly `body.source_bucket/body.source_key`.
- Upload nodes must write under `body.save_path` or another route-compatible prefix.

## Pilot Decision

Do not run a real selected web pilot against a live workflow until the live n8n graph passes the checklist above or returns a valid body-JSON manifest in a controlled smoke.

B7 runtime evidence: the live webhook used by the smoke returned a Beehive-valid manifest for one selected source, uploaded one processed S3 object, and Beehive registered the child pointer. The exact run id and output key are recorded in `docs/beehive_s3_b7_real_web_pilot_report.md`.

If a future live graph cannot be inspected or fails any item, record the exact blocker in `docs/beehive_s3_b7_real_web_pilot_report.md`.
