# B11 Resource Classes and Worker Pools Feedback

## 1. Что сделано

- Added `stage.resource_class` with supported values `default` and `local_llm`.
- Added `runtime.worker_pools.default/local_llm.concurrency` with defaults and validation.
- Added additive SQLite schema v9 migration for `stages.resource_class`.
- Added S3 stage create/update mapping from `uses_local_llm` to `resource_class`.
- Added Stage Editor checkbox `Использует локальную LLM`.
- Added resource class badges in stage lists/manage UI.
- Added `docs/worker_pools_architecture.md`.
- Did not add background workers, leases, queue UI, RabbitMQ, Kafka, or executor concurrency changes.

## 2. Какие файлы изменены

Main B11 files:

- `src-tauri/src/domain/mod.rs`
- `src-tauri/src/config/mod.rs`
- `src-tauri/src/database/mod.rs`
- `src-tauri/src/dashboard/mod.rs`
- `src-tauri/src/pipeline_editor/mod.rs`
- `src-tauri/src/services/pipeline.rs`
- `src/types/domain.ts`
- `src/pages/StageEditorPage.tsx`
- `src/components/stage-editor/ProjectRuntimeForm.tsx`
- `src/components/stage-editor/StageDraftForm.tsx`
- `src/components/stage-editor/StageDraftList.tsx`
- `docs/beehive_s3_b11_resource_classes_worker_pools_plan.md`
- `docs/beehive_s3_b11_resource_classes_worker_pools_feedback.md`
- `docs/worker_pools_architecture.md`

Mechanical test/helper updates for the new required field:

- `src-tauri/src/discovery/mod.rs`
- `src-tauri/src/executor/mod.rs`
- `src-tauri/src/file_open/mod.rs`
- `src-tauri/src/file_ops/mod.rs`
- `src-tauri/src/s3_manifest.rs`
- `src-tauri/src/s3_reconciliation.rs`
- `src-tauri/src/save_path.rs`
- `src-tauri/src/services/artifacts.rs`
- `src-tauri/src/services/selected_runner.rs`
- `src-tauri/src/services/workspaces.rs`

Pre-existing unrelated worktree changes were not reverted.

## 3. Как выглядит stage.resource_class

YAML:

```yaml
stages:
  - id: semantic_enrichment
    input_uri: s3://steos-s3-data/workspace/stages/semantic_enrichment
    workflow_url: https://n8n-dev.steos.io/webhook/semantic_enrichment
    resource_class: local_llm
```

Rust:

```rust
#[serde(rename_all = "snake_case")]
pub enum ResourceClass {
    Default,
    LocalLlm,
}
```

If `resource_class` is absent, Beehive reads the stage as `default`. Unknown values are rejected with a clear config validation error.

## 4. Как выглядит runtime.worker_pools

```yaml
runtime:
  worker_pools:
    default:
      concurrency: 10
    local_llm:
      concurrency: 1
```

Defaults when absent:

```yaml
worker_pools:
  default:
    concurrency: 1
  local_llm:
    concurrency: 1
```

Known pools only: `default`, `local_llm`. `concurrency` accepts `0..=128`.

## 5. Как старые configs мигрируют/читаются

- Old `pipeline.yaml` without `stage.resource_class` loads as `default`.
- Old `pipeline.yaml` without `runtime.worker_pools` gets default worker pool config.
- Existing SQLite DBs migrate to schema v9 with `stages.resource_class TEXT NOT NULL DEFAULT 'default'`.
- Draft DTOs also default missing worker pool config during Rust deserialization.

## 6. Что изменилось в UI

- S3 create form has checkbox `Использует локальную LLM`.
- S3 stage update form has the same checkbox.
- Local draft stage form has the same checkbox.
- Draft stage list and S3 manage panel show `Default` / `Local LLM` badges.
- Project/runtime panel shows current worker pool limits read-only.
- Raw `resource_class` is not exposed as a required operator field.

## 7. Какие tests добавлены

Backend tests cover:

- missing `resource_class` defaults to `default`;
- `resource_class: local_llm` parses;
- unknown `resource_class` is rejected;
- missing `worker_pools` gets defaults;
- worker pool concurrency parses;
- invalid concurrency and unknown pools are rejected;
- S3 create with `uses_local_llm=true` stores `local_llm`;
- S3 update can change `resource_class`;
- schema migration still bootstraps old v1/v2 DBs to current v9.

Frontend has no dedicated component test harness in this repo. UI coverage is through TypeScript/Vite builds.

## 8. Какие команды запускались и результаты

- `cargo fmt --manifest-path src-tauri/Cargo.toml`: passed.
- `cargo test --manifest-path src-tauri/Cargo.toml`: passed, `190 passed; 0 failed; 3 ignored`.
- `npm run build`: passed.
- `VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build`: passed.
- `python3 scripts/lint_n8n_workflows.py docs/n8n_workflows`: passed after `docs/n8n_workflows` was restored.
- `rg "@tauri-apps/api/core|invoke\(" src -n`: passed; only `src/lib/apiClient/tauriClient.ts` imports `invoke`.
- `git diff --check`: passed.

## 9. Что не реализовано в B11

- No DB-backed claim/lease/heartbeat worker loops.
- No queue/backpressure UI.
- No retry policy rewrite.
- No RabbitMQ/Kafka integration.
- No executor concurrency behavior changes.
- No production n8n workflow changes.

## 10. Риски для B12

- B12 must enforce pool concurrency from DB state, not from selected-run loops.
- `local_llm` protects only one Beehive-started n8n execution; n8n authors must not parallelize local LLM calls inside one `local_llm` workflow.
- Pool disable semantics for `concurrency=0` are parsed now but not enforced until workers exist.
- Workflow lint depends on `docs/n8n_workflows` being present in the worktree.

## 11. Checkpoints перечитывания ТЗ

ТЗ перечитано на этапах: after_plan, after_domain_config_design, after_backend_changes, after_ui_changes, after_tests, before_feedback
