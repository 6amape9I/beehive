# V1 Patch. Entity S3 View and Reset-to-Pending Actions

## 0. Context

This is a small but important V1 patch before starting Beehive V2.

Beehive V1 is now usable for S3+n8n workflow execution, but operators need two missing actions on each Entity:

1. View how this entity/artifact looks in S3.
2. Force-reset failed/blocked/retry-like entity-stage states back to pending with attempts reset, so the operator can retest a previously failed pipeline without reuploading/recreating the entity.

Do not start V2 architecture work in this patch. Do not add RabbitMQ/Postgres. Do not refactor the whole project.

## 1. Required process

Before code, create:

```text
docs/v1_patch_entity_view_reset_plan.md

After code, create:

docs/v1_patch_entity_view_reset_feedback.md

Feedback must include:

what was changed;
files changed;
API routes added;
UI actions added;
tests run;
what was not implemented;
manual test instructions.

Reread checkpoints:

after_plan
after_backend_design
after_s3_view_design
after_reset_design
after_ui_changes
after_tests
before_feedback
2. Feature A: View S3 JSON for entity/artifact
2.1 Product behavior

In Entity Detail and/or Entity list, add action:

View S3 JSON

Russian label if UI is Russian:

Показать JSON из S3

The action should show the actual JSON object stored in S3 for the selected file/artifact.

It should not open the local filesystem. It should not expose S3 credentials to the browser. The backend reads S3 and returns JSON to the UI.

2.2 Scope

This action is artifact/file-instance based, not only entity-based, because one entity can have multiple file instances/stage artifacts.

Preferred UI placement:

Entity Detail -> File Instances table -> each row has "View S3 JSON"

Optional secondary placement:

Entities list -> latest artifact -> "View S3 JSON"
2.3 Backend route

Add HTTP route:

GET /api/workspaces/{workspace_id}/entity-files/{entity_file_id}/s3-json

or, if current naming prefers entity route:

GET /api/workspaces/{workspace_id}/entities/{entity_id}/files/{entity_file_id}/s3-json

Choose the route that fits current API style best.

The route must:

Decode path params.
Resolve workspace.
Load entity file record by id.
Check storage_provider = s3.
Use stored bucket and key.
Fetch object body from S3.
Validate it is JSON.
Return pretty/structured JSON to frontend.
2.4 S3 client

If the current S3 client has list/head/put but no get-object body method, add:

get_json_object(bucket, key) -> Result<serde_json::Value, String>

or lower-level:

get_object_bytes(bucket, key) -> Result<Vec<u8>, String>

Then parse as UTF-8 JSON in service layer.

Return clear errors:

not_s3_artifact
s3_object_not_found
s3_read_failed
s3_json_invalid
entity_file_not_found
2.5 UI

Show JSON in a modal/drawer:

Title: S3 JSON
Subtitle: s3://bucket/key
Buttons:
  Copy JSON
  Copy S3 URI
  Close

Use a scrollable pre/code block or JSON viewer if one already exists.

Do not show AWS/S3 credentials.

If JSON is large, still cap UI rendering or warn:

Large JSON preview; showing first N KB

Bigger streaming can be deferred.

2.6 Tests

Add backend tests:

route rejects non-S3 artifact;
route returns 404 for missing entity_file_id;
route calls S3 get for correct bucket/key;
valid JSON returns 200;
invalid JSON returns s3_json_invalid;
Cyrillic S3 JSON is returned correctly.

If mocking AWS S3 is already supported, use mock trait. Do not hit real S3 in unit tests.

3. Feature B: Reset failed/blocked entity-stage states to pending
3.1 Product behavior

In Entity Detail and entity/stage state UI, add action:

Reset to pending

Russian label:

Вернуть в ожидание

Meaning:

Take a failed/blocked/retry-like state and make it runnable again.
Reset attempts to 0.
Clear error fields that would confuse the next run.
Keep history/stage_runs/app_events.
Do not delete S3 objects.
Do not delete existing output artifacts.
3.2 Which states can be reset

Allow reset from:

failed
blocked
retry_wait

Optionally allow from:

done

only with an explicit dangerous confirmation, but default B16 patch should not include done reset unless it already exists safely.

Do not allow reset from:

queued
in_progress

unless there is no active lease and a separate reconcile-stuck action has cleaned it up.

If state is in_progress, return:

state_in_progress_cannot_reset
Use Reconcile stuck worker states first.
3.3 Backend route

Add route:

POST /api/workspaces/{workspace_id}/entities/{entity_id}/stages/{stage_id}/reset-to-pending

or artifact-specific if current state model requires file id:

POST /api/workspaces/{workspace_id}/entity-files/{entity_file_id}/stages/{stage_id}/reset-to-pending

Choose the route that matches the current state model best. Since entity_stage_states is entity/stage based, entity/stage route is likely enough.

Request body:

{
  "confirm": true,
  "reason": "manual retest after fixing n8n workflow"
}

Backend must:

Decode workspace_id/entity_id/stage_id.
Load state.
Validate current status is resettable.
Ensure no active worker lease for that state.
Set:
status = pending
attempts = 0
next_retry_at = null
last_error = null or equivalent fields if present
last_started_at maybe keep or clear according to current model
last_finished_at maybe keep or clear according to current model
updated_at = now
Insert app_event:
code: entity_stage_state_manual_reset
actor/source: operator/manual
old_status
new_status
reason
Return updated state.

Do not delete stage_runs; history must remain.

3.4 Attempts reset

Attempts must be reset to zero. This is explicitly required for retesting:

attempts = 0

If there are separate attempt counters in stage_runs or file records, do not delete historical runs. Only reset the current state counter used for retry eligibility.

3.5 UI

Add button near each failed/blocked state in Entity Detail / Stage Timeline:

Вернуть в ожидание

Show confirmation modal:

Эта операция не удалит историю и S3-файлы.
Она сбросит attempts в 0 и позволит воркерам снова взять задачу.
Продолжить?

Fields:

Reason [optional text]
Confirm
Cancel

After success:

Refresh entity detail.

Show toast:

State reset to pending.
The row should become selectable/runnable by workers.
3.6 Tests

Backend tests:

failed -> pending resets attempts to 0;
blocked -> pending resets attempts to 0;
retry_wait -> pending resets attempts to 0;
queued reset rejected;
in_progress reset rejected when active lease exists;
reset inserts app_event;
stage_runs history remains;
Cyrillic entity_id route decodes and resets correctly.

Frontend/build:

button appears only for resettable states;
confirmation modal works;
API error shown clearly.
4. Interaction with workers

Reset must not conflict with active worker leases.

If reset target has active lease:

return error active_worker_lease_exists
message: "This state is currently leased by a worker. Stop/reconcile the worker lease first."

If stale/inconsistent lease exists, operator should use the existing Reconcile stuck worker states action first.

Do not force-release active leases inside reset endpoint.

5. Security / safety
Do not expose S3 credentials to frontend.
Do not allow path traversal or arbitrary bucket/key fetch from browser input.
The S3 JSON view endpoint must only fetch bucket/key from an existing entity_file record in the selected workspace.
Reset endpoint must only mutate state in the selected workspace.
Reset endpoint must create an audit event.
No S3 object deletion in this patch.
6. Docs

Create/update:

docs/v1_patch_entity_view_reset_plan.md
docs/v1_patch_entity_view_reset_feedback.md

Optional:

docs/operator_entity_actions.md

Documentation should explain:

View S3 JSON;
Reset to pending;
when reset is allowed;
what reset does not do;
why history is preserved.
7. Commands to run

Run and report exact results:

cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
git diff --check
8. Acceptance criteria

Patch is accepted if:

1. Entity Detail File Instances has View S3 JSON action.
2. View S3 JSON fetches actual S3 JSON through backend.
3. Browser never receives S3 credentials.
4. Failed/blocked/retry_wait state can be reset to pending.
5. Reset clears attempts to 0.
6. Reset preserves stage_runs history.
7. Reset creates app_event audit entry.
8. Reset is blocked for in_progress with active lease.
9. Cyrillic entity_id routes work.
10. Tests/build pass.
9. Non-goals

Do not implement:

Postgres/RabbitMQ V2;
large refactor;
new queue architecture;
bulk reset all;
editing business JSON;
deleting S3 objects;
direct browser S3 access;
full README rewrite.