# B1. S3 Artifact Control Plane Foundation

## 0. Назначение этапа

B1 — первый практический этап после стратегического перехода Beehive на S3 artifact storage.

Главная цель B1:

```text
заложить storage-agnostic control-plane foundation, где Beehive управляет S3 artifact pointers и запускает n8n без отправки business JSON.
```

B1 не должен пытаться завершить весь S3 migration сразу. Он должен аккуратно добавить основу, на которой следующий этап сможет сделать реальный S3 reconciliation и smoke pipeline.

## 1. Стратегическое решение, обязательное к соблюдению

Beehive теперь не является программой, которая отправляет JSON-сущности в n8n.

В S3 mode Beehive должен запускать n8n stage через technical artifact pointer:

```text
bucket
key
optional version_id / etag
workspace_id
stage_id
run_id
manifest location/prefix
```

n8n сам скачивает business JSON из S3, обрабатывает его и пишет outputs обратно в S3.

n8n не должен сам выбирать, какой artifact взять в production path. Beehive выбирает eligible artifact, claim'ит его и передаёт pointer.

## 2. Что B1 должен реализовать

### 2.1 Storage model foundation

Добавить storage-agnostic domain model.

Минимальные новые структуры:

```rust
StorageProvider = Local | S3

ArtifactLocation {
  provider: StorageProvider,
  local_path: Option<String>,
  bucket: Option<String>,
  key: Option<String>,
  version_id: Option<String>,
  etag: Option<String>,
}

S3StorageConfig {
  bucket: String,
  workspace_prefix: String,
  region: Option<String>,
  endpoint: Option<String>,
}

StageStorageConfig {
  stage_id: String,
  input_uri: Option<String>,
  input_folder: Option<String>,
  save_path_aliases: Vec<String>,
}
```

Exact names may differ, but the model must clearly separate:

```text
local file path
S3 bucket/key
logical save_path route
stage id
```

Do not remove old local fields yet. Existing local mode and existing tests must remain usable.

### 2.2 pipeline.yaml schema extension

Extend config parsing to support optional storage section:

```yaml
project:
  name: beehive-s3-dev
  workdir: .

storage:
  provider: s3
  bucket: steos-s3-data
  workspace_prefix: main_dir
  region: null
  endpoint: null

runtime:
  scan_interval_sec: 5
  max_parallel_tasks: 3
  stuck_task_timeout_sec: 120
  request_timeout_sec: 300
  file_stability_delay_ms: 300

stages:
  - id: raw
    input_uri: s3://steos-s3-data/main_dir/raw
    workflow_url: https://n8n-dev.steos.io/webhook/...
    max_attempts: 2
    retry_delay_sec: 10
    next_stage: null
    save_path_aliases:
      - main_dir/raw
      - /main_dir/raw

  - id: raw_entities
    input_uri: s3://steos-s3-data/main_dir/processed/raw_entities
    workflow_url: https://n8n-dev.steos.io/webhook/...
    max_attempts: 2
    retry_delay_sec: 10
    next_stage: semantic_rich
    save_path_aliases:
      - main_dir/processed/raw_entities
      - /main_dir/processed/raw_entities
```

For backward compatibility, old local config must still work:

```yaml
stages:
  - id: incoming
    input_folder: stages/incoming
    output_folder: stages/out
    workflow_url: http://localhost:5678/webhook/test
```

Validation rules:

```text
storage.provider may be absent → local mode
storage.provider = local → old local mode
storage.provider = s3 → S3 mode
S3 mode requires storage.bucket
S3 mode requires storage.workspace_prefix
S3 stage should have input_uri or a route resolvable from storage bucket/prefix
input_uri must be s3://bucket/key-prefix
save_path_aliases must be logical, not unsafe OS paths
local input_folder must not be required for S3 stages
old local stages must not be broken
```

### 2.3 Logical route resolver for S3

Evolve the existing `save_path` resolver into a storage-neutral route resolver.

It should resolve:

```text
save_path → target stage → ArtifactLocation/S3 prefix
```

Accepted examples:

```text
main_dir/processed/raw_entities
/main_dir/processed/raw_entities
s3://steos-s3-data/main_dir/processed/raw_entities
```

Rejected examples:

```text
../outside
/etc/passwd
C:\Users\bad\file
\\server\share
s3://unknown-bucket/main_dir/processed/raw_entities
s3://steos-s3-data/../../outside
empty string
```

Rules:

```text
normalize slash-separated logical paths;
legacy /main_dir/... is logical, not OS absolute;
unknown save_path → blocked route;
ambiguous save_path → blocked route;
unsafe save_path → blocked route;
route matching must use active stage config;
never silently create a route not present in pipeline config.
```

### 2.4 S3 artifact manifest model

Add parser/validator for technical n8n run manifests.

Manifest schema v1:

```json
{
  "schema": "beehive.s3_artifact_manifest.v1",
  "workspace_id": "semantic-dev",
  "run_id": "run_123",
  "source": {
    "bucket": "steos-s3-data",
    "key": "main_dir/raw/input_001.json",
    "version_id": null,
    "etag": null
  },
  "status": "success",
  "outputs": [
    {
      "artifact_id": "art_001",
      "bucket": "steos-s3-data",
      "key": "main_dir/processed/raw_entities/art_001.json",
      "save_path": "main_dir/processed/raw_entities",
      "content_type": "application/json",
      "checksum_sha256": null,
      "size": 12345
    }
  ],
  "created_at": "2026-05-12T00:00:00Z"
}
```

Error manifest:

```json
{
  "schema": "beehive.s3_artifact_manifest.v1",
  "workspace_id": "semantic-dev",
  "run_id": "run_123",
  "source": {
    "bucket": "steos-s3-data",
    "key": "main_dir/raw/input_001.json"
  },
  "status": "error",
  "error_type": "llm_invalid_json",
  "error_message": "Model returned invalid JSON",
  "outputs": [],
  "created_at": "2026-05-12T00:00:00Z"
}
```

Validation rules:

```text
schema must equal beehive.s3_artifact_manifest.v1
run_id must match active stage run
source bucket/key must match claimed source artifact
status must be success or error
success manifest may have zero outputs only for terminal/no-output stages
output bucket must match allowed storage bucket unless config explicitly allows otherwise
output key must be under target stage prefix resolved by save_path
output save_path must resolve to active stage unless fallback policy says otherwise
error manifest must have error_type and error_message
manifest must not contain business payload fields
```

### 2.5 S3 mode n8n trigger contract

For S3 mode, Beehive must not send business JSON in HTTP request body.

Preferred B1 request shape:

```text
POST workflow_url
Content-Type: application/octet-stream or no body
Accept: application/json
X-Beehive-Workspace-Id: ...
X-Beehive-Run-Id: ...
X-Beehive-Stage-Id: ...
X-Beehive-Source-Bucket: steos-s3-data
X-Beehive-Source-Key: main_dir/raw/input_001.json
X-Beehive-Source-Version-Id: optional
X-Beehive-Source-Etag: optional
X-Beehive-Manifest-Prefix: main_dir/runs/{run_id}/
```

For compatibility, B1 may support query parameters instead of headers:

```text
?workspace_id=...&run_id=...&stage_id=...&source_bucket=...&source_key=...
```

But B1 acceptance should prefer headers because they do not pollute business input.

Do not put business JSON in body.

If implementation constraints require storing an audit JSON in SQLite, that is allowed. But the actual sent body in S3 mode must be empty or non-business technical-only. Tests must verify this.

### 2.6 Executor behaviour in S3 mode

Current local executor can remain for local mode.

Add S3 mode path:

```text
load task → claim state → start stage_run → call n8n with artifact pointer → parse manifest response → validate manifest → register output artifact pointers → finish run/state
```

B1 does not need real S3 GetObject/PutObject/ListObjects.

B1 may use mocked source artifact rows and mocked n8n manifest responses.

Expected state outcomes:

```text
valid success manifest with outputs → source done, child artifacts pending
valid success manifest without outputs on terminal stage → source done
error manifest → retry_wait or failed according to attempt policy
invalid manifest → retry_wait/failed or blocked, depending error type
invalid save_path/unknown target stage → blocked
HTTP timeout/network error → retry_wait/failed
```

### 2.7 Artifact registration for S3 outputs

When n8n manifest contains output artifacts, Beehive must register pointers, not write files.

Minimum persisted data:

```text
entity_id / artifact_id
stage_id resolved from save_path
storage_provider = s3
bucket
key
version_id optional
etag optional
checksum optional
size optional
source artifact id
producer run id
status pending for target stage
```

B1 can adapt existing `entity_files` table or introduce an `entity_artifacts` abstraction. Do the smallest safe change, but do not fake S3 keys as local file paths without explicit provider metadata.

If schema migration is too large, create adapter structs and document the temporary compatibility layer. Do not silently make S3 look like a local path.

### 2.8 Documentation

Create or update:

```text
docs/s3_control_plane_architecture.md
docs/s3_n8n_contract.md
docs/beehive_s3_b1_plan.md
docs/beehive_s3_b1_feedback.md
```

`docs/s3_n8n_contract.md` must explain:

```text
Beehive does not send business JSON;
n8n receives artifact pointer;
n8n downloads from S3;
n8n uploads outputs to S3;
n8n produces manifest;
save_path maps to configured stage prefixes;
invalid route is blocked;
Search bucket nodes are demo-only, not production orchestration.
```

## 3. What B1 must not do

B1 must not:

```text
remove local mode;
rewrite UI broadly;
implement real high-load scheduler;
implement credential manager UI;
call real S3 bucket in tests;
call real n8n endpoint in tests;
manage n8n workflows through n8n REST API;
read business JSON from S3 for execution;
send business JSON to n8n;
let n8n decide source artifact via bucket search in production path;
silently accept unknown save_path;
turn invalid routes into arbitrary S3 writes;
```

## 4. Suggested implementation order

### 4.1 Read current state

Read:

```text
README.md
docs/beehive_n8n_b0_feedback.md
docs/n8n_contract.md
src-tauri/src/domain/mod.rs
src-tauri/src/config/mod.rs
src-tauri/src/executor/mod.rs
src-tauri/src/file_ops/mod.rs
src-tauri/src/save_path.rs
src-tauri/src/database/*
```

Confirm what B0 changed before editing.

### 4.2 Plan first

Create:

```text
docs/beehive_s3_b1_plan.md
```

Do not edit runtime code before writing the plan.

### 4.3 Add models and config

Add storage provider/config structs and YAML parser support.

Keep old YAML valid.

### 4.4 Add S3 route normalization

Generalize `save_path` route logic so it supports S3 prefixes and legacy logical paths.

### 4.5 Add manifest parser and tests

Implement manifest parsing/validation before touching executor.

### 4.6 Add S3 executor branch

Add S3-mode n8n trigger path with empty/non-business body and header-based control pointer.

Use mock HTTP server tests.

### 4.7 Add artifact registration path

Register manifest outputs as S3 artifact pointers.

Do not write local business JSON in S3 mode.

### 4.8 Verification and docs

Run tests, format, build, and write feedback.

## 5. Tests required

Add Rust tests for:

### 5.1 Config parsing

```text
old local pipeline.yaml still valid;
new S3 pipeline.yaml valid;
S3 mode rejects missing bucket;
S3 mode rejects invalid input_uri;
S3 stage without input_folder is accepted when input_uri exists;
```

### 5.2 Route resolver

```text
save_path main_dir/processed/raw_entities resolves to target S3 stage;
legacy /main_dir/... resolves logically;
s3://bucket/prefix resolves when bucket/prefix match;
unknown bucket rejects;
unknown prefix rejects;
Windows drive path rejects;
UNC path rejects;
../ rejects;
ambiguous aliases reject;
```

### 5.3 Manifest parser

```text
valid success manifest parses;
valid error manifest parses;
wrong schema rejects;
missing run_id rejects;
source mismatch rejects;
output save_path mismatch rejects;
output bucket mismatch rejects;
manifest with business payload rejects or warns according to policy;
```

### 5.4 Executor S3 mock

```text
S3 mode sends no business JSON body;
S3 mode sends required headers or query params;
mock n8n success manifest registers child artifact pointers;
mock n8n error manifest schedules retry or failure;
invalid save_path manifest blocks run;
terminal success with no outputs marks done;
```

### 5.5 Backward compatibility

```text
existing local payload-only tests still pass;
existing next_stage fallback local tests still pass;
local save_path behaviour remains compatible;
```

## 6. Commands to run

On Ubuntu/macOS-like shell:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
git diff --check
```

On Windows PowerShell/Developer Command Prompt:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm.cmd run build
git diff --check
```

Do not report Rust verification as passed if cargo is missing. If cargo is missing, say so explicitly and still run available checks.

## 7. Acceptance criteria

B1 is acceptable if:

```text
storage config exists and parses;
old local config remains valid;
S3 stage config can be represented;
ArtifactLocation / storage model exists;
S3 save_path route resolver exists and is tested;
S3 manifest model exists and is tested;
S3 executor/mock path does not send business JSON;
mock n8n manifest can register output artifact pointers;
invalid save_path causes blocked state;
local mode still works;
docs/s3_control_plane_architecture.md created;
docs/s3_n8n_contract.md created;
docs/beehive_s3_b1_feedback.md created;
Rust tests are run or missing Rust is honestly reported;
Ubuntu compatibility is considered explicitly.
```

## 8. Feedback requirements

Create:

```text
docs/beehive_s3_b1_feedback.md
```

It must include:

```text
1. What was implemented.
2. Files changed.
3. Schema/config changes.
4. How local compatibility is preserved.
5. How S3 artifact locations are represented.
6. How n8n is triggered in S3 mode.
7. Proof that business JSON is not sent in S3 mode.
8. How manifest parsing works.
9. How save_path routing works in S3 mode.
10. How output artifacts are registered.
11. Tests added/updated.
12. Commands run and exact results.
13. What could not be verified.
14. Ubuntu-specific risks.
15. Windows-specific risks.
16. Remaining risks.
17. What should be done in B2.
18. ТЗ reread checkpoints.
```

The feedback must include this exact line with actual checkpoints:

```text
ТЗ перечитано на этапах: after_plan, after_config_model, after_route_resolver, after_manifest_model, after_executor_s3_mode, after_tests, before_feedback
```

## 9. Main output of the next stage

B1 prepares for B2.

Expected B2 focus:

```text
real S3 reconciliation;
S3 list/metadata reading;
register existing S3 artifacts;
manual smoke pipeline with one artifact through n8n;
manifest polling/reconciliation if n8n is asynchronous.
```

B1 is foundation. Do not overreach.
