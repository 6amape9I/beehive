# B4 n8n Live Workflow Preflight

## Scope

B4 does not manage n8n workflows through the n8n REST API and does not store full production workflows in this repository. This preflight checks only practical MVP contract issues for workflow JSON available locally.

Inspected local fixture:

```text
docs/n8n_workflows/beehive_s3_pointer_smoke_body_json.json
```

No separate full live multi-stage production workflow JSON was supplied in the repository for B4.

## Findings

1. Production webhook uses POST and responseNode: the local body-JSON fixture uses `httpMethod=POST` and `responseMode=responseNode`.
2. Source selection uses JSON body: the fixture references `body.source_bucket` and `body.source_key`.
3. Source-key headers: no `X-Beehive-Source-Key` usage found in active fixture.
4. Missing old node references: no `Read Beehive headers` reference found in active fixture.
5. Search/List Bucket: active fixture does not use S3 Search Bucket/List Bucket for source selection.
6. `save_path`: active fixture derives output key from `body.save_path`; Beehive still validates returned manifest routes.
7. Legacy typo: `/main_dir/pocessed` not present.
8. Manifest response: fixture uses synchronous Respond to Webhook path.

## Pilot Notes

B4 code supports a dedicated second-stage URL through:

```text
BEEHIVE_N8N_STAGE_B_WEBHOOK
```

If that variable is absent, the opt-in real pilot reuses `BEEHIVE_N8N_SMOKE_WEBHOOK` as the second-stage webhook. That is acceptable only as an MVP smoke-compatible pilot when the workflow can download any JSON object from `source_bucket/source_key`, upload to `save_path`, and return a valid manifest.

## Operator Checks Before Real Pilot

- Confirm the imported n8n workflow URL is production/test URL appropriate for the run.
- Confirm n8n S3 credentials can download exactly `body.source_bucket/body.source_key`.
- Confirm output upload uses `body.save_path` or a route-compatible prefix.
- Confirm the response is a synchronous `beehive.s3_artifact_manifest.v1` manifest.
- If using a distinct stage-B workflow, set `BEEHIVE_N8N_STAGE_B_WEBHOOK` before running the real pilot.
