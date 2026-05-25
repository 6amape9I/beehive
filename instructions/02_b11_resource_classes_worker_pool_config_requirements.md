# 02. B11 Technical Requirements: Resource Classes and Worker Pool Configuration

## 0. Цель B11

B11 — первый шаг к параллельной обработке больших объёмов данных.

Цель B11:

```text
добавить stage.resource_class и runtime.worker_pools config;
дать пользователю простой checkbox “Использует локальную LLM”;
подготовить Beehive к B12 DB-backed leases and workers;
не запускать реальные background workers в B11.
```

B11 не должен решать всю очередь сразу. Он должен стабилизировать модель ресурсов.

## 1. Почему это нужно

Будущая нагрузка:

```text
~22 000 исходных документов;
часть документов может породить ещё больше child artifacts;
часть stages вызывает локальную LLM;
локальная LLM должна иметь жёсткий лимит параллелизма;
обычные stages могут выполняться шире.
```

Мы хотим уметь настраивать, например:

```text
10 default workers
1 local_llm worker
```

Чтобы Beehive не отправлял в n8n больше одной локальной LLM-задачи одновременно.

## 2. Product/UI requirement

В Stage create/edit UI добавить checkbox:

```text
[ ] Использует локальную LLM
```

Mapping:

```text
unchecked -> resource_class = default
checked   -> resource_class = local_llm
```

Operator help text:

```text
Если включено, этот stage будет выполняться отдельным пулом local_llm с ограниченным параллелизмом.
```

Не показывать пользователю raw enum как обязательное поле.

## 3. Backend domain model

Добавить enum/string type для resource class.

Suggested Rust shape:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceClass {
    Default,
    LocalLlm,
}
```

Если в текущем коде проще хранить строку, допустимо, но нужно централизованно валидировать allowed values:

```text
default
local_llm
```

StageDefinition должен получить:

```rust
resource_class: ResourceClass
```

Default:

```text
resource_class = default
```

## 4. Pipeline YAML

Новый stage config:

```yaml
stages:
  - id: semantic_enrichment
    workflow_url: https://n8n-dev.steos.io/webhook/...
    input_uri: s3://steos-s3-data/workspace/stages/semantic_enrichment
    save_path_aliases:
      - workspace/stages/semantic_enrichment
    resource_class: local_llm
```

Если `resource_class` отсутствует:

```text
default
```

Unknown resource class should reject config with clear error:

```text
invalid resource_class '...' for stage '...'; expected default or local_llm
```

## 5. Runtime worker pool config

Add runtime config:

```yaml
runtime:
  worker_pools:
    default:
      concurrency: 10
    local_llm:
      concurrency: 1
```

Suggested Rust model:

```rust
pub struct WorkerPoolsConfig {
    pub default: WorkerPoolConfig,
    pub local_llm: WorkerPoolConfig,
}

pub struct WorkerPoolConfig {
    pub concurrency: u32,
}
```

Default if absent:

```yaml
worker_pools:
  default:
    concurrency: 1
  local_llm:
    concurrency: 1
```

Validation:

```text
concurrency must be >= 0 and <= safe upper limit, e.g. 128;
0 means pool disabled, if you choose to support it;
unknown pool names are ignored or rejected, but document the choice;
missing default/local_llm gets default values.
```

Recommended for B11:

```text
require known pools only: default, local_llm;
reject unknown pools with clear config error;
allow 0 to support disabling a pool later;
max 128.
```

## 6. Stage create/update API

Update normal S3 stage create/update request to accept operator-friendly boolean:

```json
{
  "stage_id": "stage_2",
  "workflow_url": "https://n8n-dev.steos.io/webhook/...",
  "uses_local_llm": true,
  "allow_zero_outputs": false,
  "allow_multiple_outputs": true,
  "max_attempts": 3,
  "retry_delay_sec": 30
}
```

Backend maps:

```text
uses_local_llm=true  -> resource_class=local_llm
uses_local_llm=false -> resource_class=default
```

If API already exposes raw `resource_class`, keep it for admin/internal compatibility, but normal UI should use `uses_local_llm`.

Update response DTOs to include both if useful:

```json
{
  "resource_class": "local_llm",
  "uses_local_llm": true
}
```

## 7. UI changes

Update Stage Editor create/edit forms:

```text
Stage ID
Production n8n webhook URL
Max attempts
Retry delay
Allow zero outputs
Allow multiple outputs
Uses local LLM
Generated save_path aliases
```

Do not reintroduce:

```text
next_stage
Connect Stages
manual S3 route editing
```

Stage list/card should show a small badge:

```text
Default
Local LLM
```

Do not create a complex worker configuration UI in B11. If config display is needed, put it under Advanced/Diagnostics read-only.

## 8. Runtime behavior in B11

B11 does not implement background workers.

Existing run methods should keep working:

```text
run selected pipeline waves
run small batch
manual run entity stage
```

For B11, these methods may ignore worker_pools config, but they must preserve `resource_class` in stage metadata so B12 can use it.

Optional but useful: include resource_class in run/stage output DTOs and logs.

Do not change executor concurrency behavior in B11.

## 9. Database/schema impact

If stages are stored in SQLite with `resource_class`, add additive migration.

Rules:

```text
existing stages get resource_class='default';
new stages store selected resource_class;
old DBs open without manual migration;
no destructive migration.
```

If stage metadata is derived only from pipeline.yaml, ensure pipeline parsing and sync writes resource_class into any stage table used by UI/runtime.

## 10. Docs

Create/update:

```text
docs/beehive_s3_b11_resource_classes_worker_pools_plan.md
docs/beehive_s3_b11_resource_classes_worker_pools_feedback.md
docs/worker_pools_architecture.md
```

`docs/worker_pools_architecture.md` should explain:

```text
why Beehive owns concurrency;
what resource_class means;
default vs local_llm;
why RabbitMQ/Kafka are not used yet;
how B12 will add leases/workers;
what limitation remains when one n8n workflow internally calls local LLM multiple times.
```

## 11. Tests

Add backend tests:

```text
pipeline config without resource_class loads as default;
pipeline config with resource_class=local_llm loads correctly;
unknown resource_class is rejected;
runtime config without worker_pools gets defaults;
runtime worker_pools default/local_llm parse correctly;
invalid concurrency rejected;
create stage with uses_local_llm=true stores local_llm;
update stage can change resource_class;
old workspace/stage configs still load.
```

Add frontend/build checks:

```text
Stage Editor renders Uses local LLM checkbox;
Stage list/card shows resource class badge if tests exist;
npm run build passes;
HTTP-mode build passes.
```

Add API tests:

```text
POST stage accepts uses_local_llm;
PATCH stage accepts uses_local_llm;
GET stage/list includes resource_class/uses_local_llm.
```

## 12. Verification commands

Run and report:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
VITE_BEEHIVE_API_BASE_URL=http://127.0.0.1:8787 npm run build
python3 scripts/lint_n8n_workflows.py docs/n8n_workflows
rg "@tauri-apps/api/core|invoke\(" src -n
git diff --check
```

If any command fails, report exact output and reason.

## 13. Feedback requirements

Create:

```text
docs/beehive_s3_b11_resource_classes_worker_pools_feedback.md
```

Include:

```text
1. What was implemented.
2. Files changed.
3. Stage resource_class model.
4. Runtime worker_pools config.
5. Defaults and backward compatibility.
6. UI changes.
7. API changes.
8. Database/schema changes if any.
9. Tests added/updated.
10. Commands run and exact results.
11. What B11 intentionally did not implement.
12. Risks for B12.
13. Reread checkpoints.
```

Required checkpoint line:

```text
ТЗ перечитано на этапах: after_plan, after_domain_config_design, after_backend_changes, after_ui_changes, after_tests, before_feedback
```

## 14. Acceptance criteria

B11 is accepted only if:

```text
1. Stage has resource_class with default/local_llm.
2. Existing stages without resource_class load as default.
3. Stage create/edit UI has “Использует локальную LLM”.
4. Creating/updating stage stores resource_class correctly.
5. runtime.worker_pools config parses with default/local_llm concurrency.
6. Missing worker_pools config uses safe defaults.
7. Invalid resource_class/concurrency is rejected clearly.
8. Existing selected-run/upload/stage CRUD flows still build/test.
9. No background workers are added yet.
10. B12 has a clear path to implement leases and worker loops.
```

## 15. Non-goals

Do not implement in B11:

```text
RabbitMQ;
Kafka;
Postgres migration;
background worker loops;
lease table;
heartbeat;
queue UI;
manual retry UI;
high-load pilot;
n8n queue mode integration;
rewriting executor;
production 22k run.
```

## 16. Final reminder

B11 should be boring and safe.

Its job is to add the resource model that makes B12 possible:

```text
stage says what kind of resource it needs;
runtime says how many workers each resource class may have.
```
