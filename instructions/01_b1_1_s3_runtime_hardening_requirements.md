# B1.1. S3 Runtime Hardening before Real Smoke

## 0. Назначение этапа

B1.1 — короткий, но обязательный hardening-этап между B1 foundation и B2 real S3/n8n smoke.

B1 добавил storage-aware foundation: S3 storage model, manifest parser, S3 route resolver, S3-mode executor branch and output pointer registration. Это хорошая основа, но перед реальным прогоном нужно закрыть три архитектурных риска:

```text
1. Явно различать terminal/no-output stage и branching stage без next_stage.
2. Развести logical entity_id и physical artifact_id.
3. Сделать multi-output S3 registration transactional/idempotent.
```

Главная цель B1.1:

```text
не допустить “успешного” S3 run без outputs там, где outputs ожидались;
не превращать каждый artifact_id в новый logical entity_id;
не оставлять частично зарегистрированные child artifacts при ошибке регистрации.
```

B1.1 не должен делать real S3 List/Get/Put и не должен запускать настоящий n8n. Это backend/runtime hardening, tests, docs, and contract cleanup.

## 1. Обязательное стратегическое правило

Beehive remains the control plane.

In S3 mode:

```text
Beehive does not send business JSON to n8n.
Beehive selects and claims exactly one S3 artifact pointer.
Beehive triggers n8n with technical headers/pointer only.
n8n downloads business JSON from S3 and writes outputs to S3.
n8n returns a technical manifest.
Beehive validates the manifest and registers artifact pointers.
```

Do not roll back to local JSON transfer. Do not let n8n choose production input through S3 Search Bucket. Search/List nodes can exist only in docs/demo workflows, not in Beehive production orchestration.

## 2. What to read before coding

Read these first:

```text
README.md
instructions/00_beehive_s3_global_vision.md
instructions/01_b1_s3_control_plane_requirements.md
instructions/02_b1_agent_bootstrap.md
docs/beehive_s3_b1_plan.md
docs/beehive_s3_b1_feedback.md
docs/s3_control_plane_architecture.md
docs/s3_n8n_contract.md
src-tauri/src/domain/mod.rs
src-tauri/src/config/mod.rs
src-tauri/src/save_path.rs
src-tauri/src/s3_manifest.rs
src-tauri/src/executor/mod.rs
src-tauri/src/database/mod.rs
src-tauri/src/database/entities.rs
src-tauri/src/pipeline_editor/mod.rs
src/types/domain.ts
```

Before editing runtime code, create:

```text
docs/beehive_s3_b1_1_plan.md
```

The plan must explain:

```text
1. What B1 delivered.
2. Which B1 risks are being hardened.
3. How empty-output policy will work.
4. How entity_id and artifact_id will be separated.
5. How transactional/idempotent output registration will work.
6. Which config/domain/schema fields will be added or changed.
7. Which tests will be added.
8. Which docs will be updated.
9. What will not be done in B1.1.
```

Do not start code edits before writing this plan.

## 3. Required reread checkpoints

Reread this instruction at these checkpoints and record them in feedback:

```text
after_plan
after_empty_output_policy
after_entity_artifact_identity
after_registration_transaction
after_stage_editor_docs_visibility
after_tests
before_feedback
```

Feedback must contain the exact line:

```text
ТЗ перечитано на этапах: after_plan, after_empty_output_policy, after_entity_artifact_identity, after_registration_transaction, after_stage_editor_docs_visibility, after_tests, before_feedback
```

## 4. B1.1 scope

### 4.1 Explicit empty-output policy

B1 currently treats a S3 success manifest with zero outputs as valid when `source_stage.next_stage` is absent. This is unsafe because a branching stage may intentionally have no single `next_stage`, yet still be required to return outputs through `save_path`.

Add explicit S3 output policy to stage config.

Recommended minimal config field:

```yaml
stages:
  - id: semantic_split
    input_uri: s3://steos-s3-data/main_dir/raw
    workflow_url: https://n8n-dev.steos.io/webhook/...
    allow_empty_outputs: false
    save_path_aliases:
      - main_dir/raw
```

For terminal/no-output stage:

```yaml
stages:
  - id: final_archive
    input_uri: s3://steos-s3-data/main_dir/final
    workflow_url: https://n8n-dev.steos.io/webhook/...
    allow_empty_outputs: true
```

Rules:

```text
In S3 mode, success manifest with zero outputs is valid only if source stage allow_empty_outputs = true.
Default allow_empty_outputs must be false in S3 mode.
Branching stages must not be able to mark source done with zero outputs unless explicitly configured.
Local mode legacy behavior may remain compatible; do not break old local terminal stages.
```

Possible implementation names may differ, but the semantics must be explicit. If you choose `terminal: true` instead of `allow_empty_outputs`, document clearly that it gates empty success manifests. The preferred field is `allow_empty_outputs` because it states the runtime behavior directly.

Update:

```text
StageDefinition
StageDefinitionDraft
StageRecord
Pipeline parser
Pipeline editor draft preservation
SQLite stages table if stage config is stored there
TypeScript domain types
S3 manifest validation context
S3 n8n contract docs
```

Tests required:

```text
S3 non-terminal/branching stage with no next_stage and allow_empty_outputs=false rejects zero-output success manifest.
S3 terminal/no-output stage with allow_empty_outputs=true accepts zero-output success manifest.
Old local terminal stage still works.
S3 default allow_empty_outputs is false when omitted.
Pipeline YAML serializes/preserves allow_empty_outputs.
```

### 4.2 Separate logical entity identity from physical artifact identity

B1 registered output pointers using manifest `artifact_id` as `entity_id`. This is unsafe. A logical entity can produce many physical artifacts over stages, and one stage can also create child entities.

Update the S3 manifest contract so each output explicitly contains both:

```json
{
  "artifact_id": "art_001",
  "entity_id": "entity_001",
  "bucket": "steos-s3-data",
  "key": "main_dir/processed/raw_entities/art_001.json",
  "save_path": "main_dir/processed/raw_entities",
  "relation_to_source": "child_entity",
  "content_type": "application/json",
  "checksum_sha256": null,
  "size": 12345
}
```

Required fields for success outputs:

```text
artifact_id: non-empty, unique within manifest
entity_id: non-empty logical entity id
bucket
key
save_path
relation_to_source
```

Recommended `relation_to_source` enum:

```text
same_entity        # enrichment/weight/metadata update of the same logical entity
child_entity       # extraction created a new logical entity
representation_of  # output is representation/alias of source or target entity
candidate_parent   # semantic placement/found-parent result
relation_artifact  # graph/relation metadata artifact
other              # allowed only with explicit docs/tests
```

Rules:

```text
artifact_id is a physical artifact/run output identifier.
entity_id is the logical entity identifier used in Beehive entities/entity_stage_states.
For same_entity outputs, entity_id should equal the source logical entity id.
For child_entity outputs, entity_id may differ from source entity_id.
For relation/representation outputs, entity_id must still be explicit; if a different relationship is needed, use relation_to_source and optional source_entity_id/target_entity_id fields.
Do not silently fall back to artifact_id as entity_id in S3 mode.
If entity_id is missing in manifest output, the manifest is invalid.
```

Database changes:

```text
Add explicit artifact_id column if not already persisted in entity_files/entity_artifacts.
Keep entity_id as logical entity id.
Persist producer_run_id.
Persist copy_source_file_id/source_artifact link.
If schema migration is needed, bump schema version from v5 to v6.
Do not lose existing local rows.
```

If the existing table is still named `entity_files`, it may remain for compatibility, but S3 pointer records must clearly represent artifacts. Do not encode artifact identity only inside `file_path`.

Tests required:

```text
S3 manifest output with artifact_id=art_a and entity_id=entity_x registers entity_id=entity_x, not art_a.
Two artifacts for the same entity across stages remain one logical entity with multiple artifact/file rows.
Missing entity_id in output rejects manifest.
Duplicate artifact_id inside one manifest rejects manifest.
same_entity output with mismatched entity_id is rejected or marked invalid according to documented rule.
child_entity output with distinct entity_id is accepted.
```

### 4.3 Transactional and idempotent multi-output registration

B1 validates all manifest outputs before registration, but DB registration is not guaranteed all-or-nothing. Fix this before real smoke.

Required behavior:

```text
All manifest outputs are validated before any DB mutation.
All output pointer registrations for a successful S3 manifest happen in one SQLite transaction.
If any output registration fails, no child artifact pointer and no child stage state from this manifest should be partially committed.
Retry/replay of the same manifest should be idempotent.
```

Idempotency rules:

Use stable natural keys. Recommended:

```text
producer_run_id + artifact_id
or bucket + key + version_id
```

Rules:

```text
Same run_id + same artifact_id + same bucket/key -> no duplicate, compatible already-exists behavior.
Same run_id + same artifact_id + different bucket/key -> blocked/failed registration conflict.
Same bucket/key already registered to different entity_id -> blocked/failed conflict.
Same bucket/key registered to same entity_id and same artifact_id -> idempotent success.
```

Tests required:

```text
Multi-output manifest registers all outputs in one transaction.
Forced failure on output N rolls back outputs 1..N-1.
Replaying identical success manifest does not duplicate entity_files/entity_stage_states.
Replaying same artifact_id with different key is rejected.
Same output key for another entity is rejected.
```

### 4.4 Stage Editor and config preservation hardening

B1 feedback says Stage Editor is still primarily local-mode. B1.1 does not need a full S3 editor UI, but it must not corrupt S3 config.

Required minimal behavior:

```text
Pipeline editor draft model preserves storage, input_uri, save_path_aliases, allow_empty_outputs.
Saving a S3 pipeline through backend validation must not require local input_folder.
If UI cannot edit a field, the backend must preserve it from draft/config or document that S3 config should be edited in YAML manually until B2/B3.
S3 configs should not trigger local directory creation for empty input_folder.
```

Tests required:

```text
S3 pipeline draft validate/save preserves storage/input_uri/save_path_aliases/allow_empty_outputs.
S3 stage without input_folder does not fail backend draft validation.
Local stage folder validation remains strict.
```

### 4.5 Operator visibility minimum

B1.1 does not need full S3 file browser. But Entity Detail / Workspace Explorer / records should not pretend S3 objects are local files.

Minimum requirements:

```text
S3 pointer rows expose storage_provider, bucket, key, artifact_id, entity_id, producer_run_id, checksum/size if known.
S3 pointer rows do not expose Open file/Open folder as local actions.
S3 selected_file_json/latest_json_preview should not suggest that Beehive loaded business JSON from S3.
If preview is not available, show a technical pointer JSON or clear placeholder.
```

Tests or inspection required:

```text
S3 artifact pointer allowed actions: can_open_file=false, can_open_folder=false, can_edit_business_json=false.
Entity detail for S3 pointer does not try to read local path.
Workspace explorer does not mark S3 artifacts missing merely because no local file exists.
```

### 4.6 README and docs cleanup

README is currently local-era and can mislead future agents. Add a clear top-level note without rewriting the whole README:

```text
Current architecture note:
Local mode still exists, but S3 mode is now being introduced through B1/B1.1. For S3 mode, see docs/s3_control_plane_architecture.md and docs/s3_n8n_contract.md. The old local folder execution sections do not describe S3-mode execution.
```

Update docs:

```text
docs/s3_control_plane_architecture.md
docs/s3_n8n_contract.md
docs/beehive_s3_b1_1_feedback.md
```

The contract docs must include:

```text
allow_empty_outputs policy;
entity_id vs artifact_id separation;
relation_to_source enum;
idempotency/conflict rules;
transactional registration expectation;
```

## 5. What B1.1 must not do

Do not:

```text
call real S3;
call real n8n;
implement credential manager UI;
implement high-load scheduler;
rewrite the entire UI;
remove local mode;
read S3 business JSON in execution path;
send business JSON to n8n;
let n8n choose source objects through bucket search;
hide route/manifest failures as success;
accept output objects without explicit entity_id in S3 mode;
```

## 6. Verification commands

Run:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
git diff --check
```

On Windows, also document the expected command form:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm.cmd run build
git diff --check
```

Do not claim tests passed if the commands were not actually run. If the environment is missing Rust/Tauri dependencies, report the exact blocker.

## 7. Required feedback

Create:

```text
docs/beehive_s3_b1_1_feedback.md
```

It must include:

```text
1. What was implemented.
2. Files changed.
3. Schema/config changes.
4. How allow_empty_outputs/terminal policy works.
5. How entity_id and artifact_id are separated.
6. How relation_to_source works.
7. How transactional/idempotent registration works.
8. How local compatibility was preserved.
9. How Stage Editor/config preservation was hardened.
10. How operator visibility for S3 pointers was improved.
11. Tests added/updated.
12. Commands run and exact results.
13. What could not be verified.
14. Ubuntu-specific notes.
15. Windows-specific notes.
16. Remaining risks.
17. What B2 should do next.
18. ТЗ reread checkpoints.
```

The feedback must say explicitly whether B2 can start.

If B2 should be blocked, write:

```text
B2 readiness: blocked
Reason: ...
```

If B2 can start, write:

```text
B2 readiness: ready
Main next output: real S3 reconciliation + one-artifact n8n smoke
```

## 8. Acceptance criteria

B1.1 is acceptable if:

```text
S3 empty-output success is explicit and safe;
branching stage cannot silently complete with zero outputs by merely omitting next_stage;
manifest outputs require entity_id and artifact_id;
artifact_id no longer replaces logical entity_id;
relation_to_source is modeled and documented;
S3 output registration is transactional or proven idempotent/conflict-safe;
replay of same manifest is safe;
partial registration rollback is tested;
Stage Editor/backend config preservation does not corrupt S3 config;
README points to S3 docs and warns about local-era sections;
local mode tests still pass;
all required feedback/docs exist.
```

## 9. Main output for next stage

The main output for B2 is:

```text
A hardened S3 runtime foundation that can safely run one real S3 artifact through n8n without sending business JSON, without silent no-output success, and without corrupting entity/artifact lineage.
```
