# B10. Runtime Contract Hardening: URL Decode, Partial Outputs, and Output Cardinality
{
  "registered_count": 2,
  "skipped_count": 1,
  "invalid_count": 1,
  "conflict_count": 1,
  "outputs": [
    {
      "artifact_id": "...",
      "entity_id": "...",
      "target_stage_id": "stage_1",
      "status": "registered|idempotent_skipped|invalid|conflict|failed",
      "message": null
    }
  ]
}

Where to store:

prefer a new JSON column if easy;
otherwise store in an app event with code output_registration_report;
do not lose original n8n response manifest.
7. Stage output cardinality enforcement
7.1 New stage fields

Add or expose:

allow_zero_outputs: bool
allow_multiple_outputs: bool

Legacy:

allow_empty_outputs: bool // deprecated alias for allow_zero_outputs
7.2 Validation rules

For status=success manifest:

outputs.len() == 0:
    allowed only if allow_zero_outputs = true

outputs.len() == 1:
    allowed always

outputs.len() > 1:
    allowed only if allow_multiple_outputs = true

Violation should be:

manifest_blocked

not retry.

Reason: n8n returned a structurally valid response, but it violates stage contract. Retrying the same stage will likely produce the same contract violation.

7.3 UI wording

Replace old/technical wording with two toggles:

[ ] Разрешено 0 выходов
[ ] Разрешено несколько выходов

Small help text:

По умолчанию stage ожидает ровно 1 output. Если workflow может отфильтровать вход и ничего не вернуть — включите "Разрешено 0 выходов". Если workflow может породить несколько новых сущностей — включите "Разрешено несколько выходов".

Do not use isTerminal in UI.

Do not use next_stage in UI.

8. Entity Detail bug

Fix direct entity operations:

GET    /api/workspaces/{workspace_id}/entities/{entity_id}
PATCH  /api/workspaces/{workspace_id}/entities/{entity_id}
DELETE /api/workspaces/{workspace_id}/entities/{entity_id}
POST   /api/workspaces/{workspace_id}/entities/{entity_id}/restore

They must work for:

миллиграмм
symptom_Кольца_Кайзера-Флейшера_e74b3ffa92f0
disease_Астроцитарная_опухоль_взрослого_ef18fec939e7
9. Timeout is not the main B10 task

There are real long-running LLM stages. Current request timeout can still be configured in pipeline runtime.

Do not turn B10 into a timeout project.

But do not regress timeout behavior.

If you touch config defaults, preserve LLM-friendly runtime and mention it in feedback.

10. Files likely to change

Backend:

src-tauri/src/http_api/mod.rs
src-tauri/src/s3_manifest.rs
src-tauri/src/executor/mod.rs
src-tauri/src/database/mod.rs
src-tauri/src/domain/mod.rs
src-tauri/src/config/mod.rs
src-tauri/src/services/pipeline.rs

Frontend:

src/pages/StageEditorPage.tsx
src/types/domain.ts
src/lib/apiClient/types.ts
src/lib/apiClient/httpClient.ts
src/lib/apiClient/tauriClient.ts
src/lib/runtimeApi.ts

Docs:

docs/n8n_s3_manifest_contract.md
docs/operator_entities_upload_runbook.md
docs/beehive_s3_b10_runtime_contract_hardening_plan.md
docs/beehive_s3_b10_runtime_contract_hardening_feedback.md
11. Tests

Add tests for all critical behavior.

URL decoding:

path param Cyrillic entity_id decodes before service call
query param Cyrillic search decodes
invalid percent escape returns 400
plus sign stays plus in path
plus sign becomes space in query value

Cardinality:

default stage rejects 0 outputs
default stage accepts exactly 1 output
default stage rejects many outputs
allow_zero_outputs accepts 0 outputs
allow_multiple_outputs accepts many outputs
allow_zero_outputs + allow_multiple_outputs accepts 0 and many outputs
legacy allow_empty_outputs=true maps to allow_zero_outputs=true

Partial output registration:

3 outputs: 2 valid, 1 conflict -> 2 registered, run success with warning/report
all outputs conflict as idempotent duplicates -> run success
all outputs invalid/conflict and no registered/idempotent -> run blocked, not retry
duplicate artifact_id inside manifest -> clearly reported and no retry loop
same entity_id + same target stage conflict does not remove valid sibling outputs

n8n response shape:

manifest array rejected
manifest string rejected
manifest wrapped in body rejected
manifest with business payload field rejected
source key URL-encoded instead of literal key rejected with clear message

Existing behavior:

B7 selected runner
B9 upload/import
Workspace CRUD
Stage CRUD without next_stage UI
S3 save_path routing
12. Smoke

Create or update smoke:

scripts/web_operator_contract_smoke.mjs

Smoke should use temp registry/workspace root and mock/webhook where possible.

Scenario:

1. Create workspace.
2. Create stage_0 with allow_zero_outputs=false, allow_multiple_outputs=true.
3. Create stage_1 target stage.
4. Import one source JSON.
5. Mock n8n returns 3 outputs:
   - valid output A
   - valid output B
   - conflicting output C
6. Verify A and B are visible as pending children.
7. Verify source is done with warning/report if valid outputs were registered.
8. Verify output registration report exists.
9. Verify GET entity detail works for Cyrillic entity_id.

If mock webhook is too heavy for JS smoke, do it in Rust tests and state clearly in feedback.

13. Verification commands

Run:

cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
rg "@tauri-apps/api/core|invoke\\(" src -n
git diff --check

If any command cannot run, explain exactly why.

Do not claim tests passed unless they actually ran.

14. Feedback

Create:

docs/beehive_s3_b10_runtime_contract_hardening_plan.md
docs/beehive_s3_b10_runtime_contract_hardening_feedback.md

Feedback must include:

what changed
which files changed
URL decode behavior
cardinality behavior
legacy allow_empty_outputs migration behavior
partial output registration behavior
what happens on duplicate/conflicting output
what happens on zero outputs
what happens on many outputs
n8n manifest contract summary
commands run
test results
smoke results
known risks
what remains for B11

Required checkpoint line:

ТЗ перечитано на этапах: after_plan, after_url_decode_design, after_cardinality_design, after_partial_output_design, after_backend_runtime_changes, after_ui_changes, after_tests, after_smoke, before_feedback
15. Acceptance criteria

B10 is accepted only if:

1. Entity view/edit/delete/restore works with Cyrillic entity_id in URL.
2. Query search works with Cyrillic.
3. Stage has two operator toggles:
   - allow zero outputs
   - allow multiple outputs
4. Default stage means exactly one output.
5. Zero outputs are allowed only when the stage allows zero outputs.
6. Multiple outputs are allowed only when the stage allows multiple outputs.
7. One bad output does not discard valid sibling outputs.
8. Duplicate/idempotent outputs do not cause retry loops.
9. Output conflicts are reported clearly.
10. Manifest-level errors remain strict.
11. n8n manifest contract docs exist and explain every field.
12. Existing selected-run and upload flows still work.
16. Non-goals

Do not implement:

Postgres migration
RBAC
background workers
async manifest polling
n8n REST workflow editor
production 22k-file run
visual graph editor
full README rewrite
17. Product principle

Beehive must be strict about the manifest contract, but forgiving about individual bad outputs.

A successful n8n run that produced 10 outputs must not lose 9 good outputs because 1 output conflicted.


Эта формулировка делает именно то, что ты предложил: вместо `isTerminal` у stage появляются две простые настройки. Default остаётся строгим `1 -> 1`, а более сложные случаи явно включаются оператором.