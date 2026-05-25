# B11 Resource Classes and Worker Pools Plan

## 1. Что понял из задачи

B11 должен добавить стабильный ресурсный контракт для будущего worker layer:

- `stage.resource_class` со значениями `default` и `local_llm`;
- `runtime.worker_pools` с лимитами concurrency для известных пулов;
- operator-friendly checkbox `Использует локальную LLM`;
- совместимость со старыми `pipeline.yaml` и SQLite;
- без реальных background workers, leases, RabbitMQ/Kafka или переписывания executor.

Beehive остаётся control-plane. n8n остаётся data-plane. В B11 существующие selected/manual run flows продолжают работать как раньше и могут игнорировать `worker_pools`, но stage metadata должна сохранять `resource_class` для B12.

## 2. Какие файлы прочитал

Инструкции:

- `instructions/00_beehive_worker_pools_global_vision.md`
- `instructions/01_codex_agent_working_rules.md`
- `instructions/02_b11_resource_classes_worker_pool_config_requirements.md`

Предыдущие feedback:

- `docs/beehive_s3_b9_entities_upload_simplified_crud_feedback.md`
- `docs/beehive_s3_b10_runtime_contract_hardening_feedback.md`

Кодовые поверхности:

- `src-tauri/src/domain/mod.rs`
- `src-tauri/src/config/mod.rs`
- `src-tauri/src/database/mod.rs`
- `src-tauri/src/services/pipeline.rs`
- `src-tauri/src/services/workspaces.rs`
- `src-tauri/src/pipeline_editor/mod.rs`
- `src-tauri/src/http_api/mod.rs`
- `src-tauri/src/commands/mod.rs`
- `src-tauri/src/discovery/mod.rs`
- `src/types/domain.ts`
- `src/pages/StageEditorPage.tsx`
- `src/components/stage-editor/StageDraftForm.tsx`
- `src/components/stage-editor/StageDraftList.tsx`
- `src/components/stage-editor/ProjectRuntimeForm.tsx`
- `src/lib/runtimeApi.ts`
- `src/lib/apiClient/httpClient.ts`
- `src/lib/apiClient/tauriClient.ts`
- `src/lib/apiClient/types.ts`

## 3. Где сейчас описан StageDefinition / pipeline config

Rust domain:

- `StageDefinition`, `StageDefinitionDraft`, `PipelineConfig`, `RuntimeConfig` находятся в `src-tauri/src/domain/mod.rs`.
- YAML parsing идёт через raw-модели `RawPipelineConfig`, `RawRuntimeConfig`, `RawStageDefinition` в `src-tauri/src/config/mod.rs`.
- Local draft editor собирает `PipelineConfig` из `PipelineConfigDraft` в `src-tauri/src/pipeline_editor/mod.rs`.

Frontend types:

- `StageDefinition`, `StageDefinitionDraft`, `PipelineConfig`, `RuntimeConfig`, create/update request DTOs находятся в `src/types/domain.ts`.

## 4. Где сейчас создаются/редактируются stages

Backend normal S3 CRUD:

- `create_s3_stage_for_workspace`, `update_s3_stage_for_workspace`, `build_s3_stage`, `apply_stage_update` находятся в `src-tauri/src/services/pipeline.rs`.
- HTTP routes парсят `CreateS3StageRequest` / `UpdateS3StageRequest` в `src-tauri/src/http_api/mod.rs`.
- Tauri commands прокидывают те же DTOs в `src-tauri/src/commands/mod.rs`.

Frontend:

- S3 create/update UI находится в `src/pages/StageEditorPage.tsx`.
- Local YAML/draft stage edit form находится в `src/components/stage-editor/StageDraftForm.tsx`.
- Draft stage list находится в `src/components/stage-editor/StageDraftList.tsx`.

## 5. Где сейчас запускается selected run / waves

Selected/manual runtime surfaces уже существуют вне B11:

- selected pipeline orchestration: `src-tauri/src/services/selected_runner.rs`;
- runtime task selection/persistence: `src-tauri/src/database/mod.rs`;
- workflow execution and manifest handling: `src-tauri/src/executor/mod.rs`;
- entity manual actions: `src-tauri/src/services/entities.rs` and related HTTP/command routes.

B11 не меняет executor concurrency behavior и не добавляет worker loops.

## 6. Как добавлю resource_class

Backend:

- Добавлю `ResourceClass` enum в `src-tauri/src/domain/mod.rs` with `serde(rename_all = "snake_case")`.
- Добавлю `resource_class: ResourceClass` в `StageDefinition`, `StageDefinitionDraft`, `StageRecord`, `WorkspaceStageTree` and relevant DTOs.
- Default для отсутствующего YAML поля: `default`.
- Unknown YAML value rejected with a clear validation error mentioning stage id and expected values.
- `CreateS3StageRequest` / `UpdateS3StageRequest` получат `uses_local_llm: Option<bool>` and optional raw `resource_class` for compatibility/internal use.
- Normal create/update mapping: `uses_local_llm=true -> local_llm`, `false -> default`; if raw `resource_class` is used, validate centrally.

Frontend:

- Добавлю `ResourceClass` type and `uses_local_llm?: boolean` helper fields.
- Stage create/edit UI will expose checkbox `Использует локальную LLM`.
- Stage list/card will show a compact `Default` / `Local LLM` badge.

## 7. Как добавлю worker_pools config

Backend:

- Добавлю `WorkerPoolConfig` and `WorkerPoolsConfig` in `src-tauri/src/domain/mod.rs`.
- Добавлю `worker_pools: WorkerPoolsConfig` into `RuntimeConfig` and `RuntimeConfigDraft`.
- Defaults when absent: `default.concurrency=1`, `local_llm.concurrency=1`.
- YAML parser will accept only known pool names `default` and `local_llm`.
- `concurrency` validation: allow `0..=128`; reject negative/not numeric via serde parse failure or validation issue.
- If `runtime.worker_pools` is partially present, missing known pools get default values.

Frontend:

- Local draft runtime state will carry worker pool values so YAML preview/save preserves them.
- B11 will avoid adding a complex worker pool editor in the main UI. If exposed, it will be read-only/diagnostic.

## 8. Как сохраню backward compatibility

- Missing `stage.resource_class` parses as `default`.
- Missing `runtime.worker_pools` parses as default config.
- Existing DB stages get `resource_class='default'` via additive migration.
- Existing API callers that do not send `uses_local_llm` or `resource_class` continue creating default stages.
- Legacy `allow_empty_outputs` alias remains untouched.
- Existing selected runs/manual runs keep using current executor flow.

## 9. Как UI покажет "Использует локальную LLM"

- In `S3StageCreationPanel`, add checkbox with help text:
  `Если включено, этот stage будет выполняться отдельным пулом local_llm с ограниченным параллелизмом.`
- In `StageCrudPanel`, add the same checkbox for active stages.
- In `StageDraftForm`, add the checkbox for local YAML/draft editing.
- In `StageDraftList` and S3 manage list/selected stage area, show `Default` / `Local LLM` as a small badge/label.
- Do not expose raw enum as a required operator field.

## 10. Какие tests добавлю

Backend tests:

- config without `resource_class` loads as `default`;
- config with `resource_class: local_llm` loads correctly;
- unknown `resource_class` is rejected with a clear validation error;
- runtime config without `worker_pools` gets defaults;
- runtime `worker_pools.default/local_llm` parse correctly;
- invalid concurrency is rejected;
- create S3 stage with `uses_local_llm=true` stores `local_llm`;
- update S3 stage can change resource class;
- old workspace/stage configs still load.

Frontend/build checks:

- There are no existing dedicated React component tests, so B11 UI verification will be through TypeScript/Vite builds unless a small local test pattern is already present.
- Ensure DTO types and render paths compile.

## 11. Какие команды запущу

Required by B11:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
rg "@tauri-apps/api/core|invoke\(" src -n
git diff --check
```

If a command cannot run, feedback will record the exact failure.

## 12. Что не буду делать в B11

- No RabbitMQ.
- No Kafka.
- No real background worker loops.
- No DB-backed claim/lease/heartbeat implementation.
- No retry policy rewrite.
- No complex worker pool UI.
- No executor concurrency behavior changes.
- No production n8n workflow changes.
- No cleanup of unrelated deleted/modified files already present in the worktree.

## 13. Риски

- SQLite migration touches the shared `stages` table, so schema version and old DB tests must be updated carefully.
- Many Rust test helpers construct `StageDefinition` directly and will need a default `resource_class`.
- Existing workspace explorer/stage tree DTOs must remain aligned across Rust and TypeScript.
- The worktree already contains unrelated doc/instruction deletions and config changes. B11 changes must not revert or depend on them.

## Checkpoints

Planned reread checkpoints:

- after_plan
- after_domain_config_design
- after_backend_changes
- after_ui_changes
- after_tests
- before_feedback
