# Beehive S3 B1.1 Plan

## 1. What B1 delivered

B1 added the S3 control-plane foundation: storage-aware domain/config structs, S3 stage `input_uri`, `save_path_aliases`, S3 route resolution, manifest parser, S3-mode executor with empty webhook body and technical pointer headers, and S3 pointer registration in SQLite `entity_files`. Local mode remained working and the final B1 verification passed.

## 2. B1 risks being hardened

B1.1 hardens three runtime risks before real S3/n8n smoke:

- zero-output success was inferred from `next_stage = null`, which is unsafe for branching stages;
- S3 outputs used `artifact_id` as logical `entity_id`;
- multi-output registration validated outputs first but registered each output separately.

## 3. Empty-output policy

Add explicit `allow_empty_outputs: bool` to stage config/domain/draft/SQLite/TS models. In S3 mode the default is `false`. A success manifest with zero outputs is accepted only when the source stage has `allow_empty_outputs = true`. Local mode remains compatible because local terminal behavior is not driven by S3 manifests.

## 4. Entity and artifact identity separation

Extend manifest outputs to require both `artifact_id` and `entity_id`. `artifact_id` is the physical artifact/run output id. `entity_id` is the logical Beehive entity id used in `entities` and `entity_stage_states`. Persist `artifact_id` explicitly in `entity_files` and keep `entity_id` as logical identity. Add `relation_to_source` with the documented enum.

## 5. Transactional/idempotent output registration

Add batch S3 pointer registration that:

- validates all DB registration conflicts before mutation;
- performs all output inserts/updates and child state upserts in one SQLite transaction;
- treats same `producer_run_id + artifact_id + bucket/key` as idempotent;
- rejects same `producer_run_id + artifact_id` with different bucket/key;
- rejects same bucket/key already registered to a different entity;
- rolls back the whole batch on any failure.

The existing single-pointer registration remains as a wrapper for tests/source bootstrap and local helpers.

## 6. Config/domain/schema fields

Add or change:

- `StageDefinition.allow_empty_outputs`;
- `StageDefinitionDraft.allow_empty_outputs`;
- `StageRecord.allow_empty_outputs`;
- `RawStageDefinition.allow_empty_outputs`;
- `EntityFileRecord.artifact_id`;
- `EntityFileRecord.relation_to_source`;
- `PersistEntityFileInput.artifact_id`;
- `RegisterS3ArtifactPointerInput.relation_to_source`;
- SQLite schema v6: `stages.allow_empty_outputs`, `entity_files.artifact_id`, `entity_files.relation_to_source`;
- TypeScript domain fields matching the Rust domain.

## 7. Tests to add/update

- S3 omitted `allow_empty_outputs` defaults to false.
- S3 zero-output success with `allow_empty_outputs=false` is rejected even when `next_stage` is absent.
- S3 zero-output success with `allow_empty_outputs=true` is accepted.
- Pipeline editor draft validate/save preserves `storage`, `input_uri`, `save_path_aliases`, and `allow_empty_outputs`.
- S3 stage without `input_folder` passes draft validation and does not create local directories.
- Manifest output missing `entity_id` rejects.
- Duplicate `artifact_id` inside one manifest rejects.
- `same_entity` with mismatched `entity_id` rejects.
- `child_entity` with distinct `entity_id` accepts.
- Output `artifact_id=art_a`, `entity_id=entity_x` registers `entity_x`, not `art_a`.
- Multi-output batch registers all outputs in one transaction.
- Conflict on later output rolls back earlier outputs.
- Replay of identical outputs is idempotent.
- Same `producer_run_id + artifact_id` with different key rejects.
- Same bucket/key for another entity rejects.
- S3 pointer actions and previews do not behave like local files.
- Local mode tests continue to pass.

## 8. Docs to update

- `README.md`: top-level note that local-era sections do not describe S3 mode.
- `docs/s3_control_plane_architecture.md`: B1.1 policy, identity, transaction/idempotency.
- `docs/s3_n8n_contract.md`: `allow_empty_outputs`, required `entity_id`, `relation_to_source`, conflict/idempotency rules.
- `docs/beehive_s3_b1_1_feedback.md`: final report and verification.

## 9. Not done in B1.1

B1.1 will not call real S3, call real n8n, implement credential UI, implement a full S3 browser/editor, rewrite the UI, introduce a high-load scheduler, read S3 business JSON during execution, or send business JSON to n8n.

## 10. Execution checklist

- [x] after_plan reread.
- [x] Empty-output policy implemented.
- [x] after_empty_output_policy reread.
- [x] Entity/artifact identity implemented.
- [x] after_entity_artifact_identity reread.
- [x] Transactional registration implemented.
- [x] after_registration_transaction reread.
- [x] Stage Editor/docs/operator visibility hardened.
- [x] after_stage_editor_docs_visibility reread.
- [x] Required commands run.
- [x] after_tests reread.
- [x] Feedback written.
- [x] before_feedback reread recorded.
