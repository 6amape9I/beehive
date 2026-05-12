# Beehive S3 B1 Plan

## 1. Что понял из стратегического изменения

Beehive переходит из роли локального JSON-прогонщика в control-plane для S3+n8n pipeline. В S3 mode Beehive должен выбирать конкретный artifact, claim'ить его, создавать `run_id`, передавать n8n только технический pointer на S3 object, принимать technical manifest, валидировать routes и регистрировать output artifact pointers. Business JSON в S3 mode не должен уходить в HTTP body.

## 2. Что уже есть после B0

- Local executor уже claim'ит `entity_stage_states`, пишет `stage_runs` и обновляет runtime state через state machine.
- Local n8n request уже payload-only: runtime metadata не отправляется.
- Local response handling поддерживает root array, wrapper `success/payload/meta` и direct object.
- `src-tauri/src/save_path.rs` уже умеет безопасно нормализовать local `save_path` и маппить его на active stage `input_folder`.
- `file_ops` уже делает all-or-nothing planning перед local file writes.
- SQLite schema v4 хранит `entities`, `entity_files`, `entity_stage_states`, `stage_runs`, `app_events`.

## 3. Какие старые local-mode части нельзя ломать

- Старый `pipeline.yaml` без `storage` должен парситься как local mode.
- Local stages с `input_folder`, `output_folder`, `next_stage` должны работать как сейчас.
- `Scan workspace` и local file reconciliation остаются filesystem-based.
- B0 payload-only local request остается JSON body с source `payload`.
- Local `save_path` routing и `next_stage` fallback должны остаться совместимыми.
- Existing Rust tests должны остаться валидными.

## 4. Как добавлю storage model

Добавлю domain structs/enums:

- `StorageProvider = Local | S3`;
- `ArtifactLocation` с явным разделением `local_path` и `bucket/key`;
- `S3StorageConfig`;
- `StageStorageConfig` / stage-level fields для `input_uri` и `save_path_aliases`.

Минимальный persist layer для B1: добавить schema v5 с metadata columns в `stages` и `entity_files`, не удаляя старые local columns. Это даст явные `storage_provider`, `bucket`, `key`, `version_id`, `etag`, `checksum`, `size`, `source_artifact_id`, `producer_run_id`, не превращая S3 key в local path без metadata.

## 5. Как расширю pipeline.yaml

Расширю parser:

- optional `storage`;
- `storage.provider` absent или `local` -> local mode;
- `storage.provider = s3` требует `bucket` и `workspace_prefix`;
- stage-level `input_uri` и `save_path_aliases`;
- local `input_folder` остается required только для local stages;
- S3 stage принимает `input_uri` без `input_folder`;
- `input_uri` должен быть `s3://bucket/key-prefix`.

Для совместимости `StageDefinition.input_folder` сохраню как string, но в S3 stages он может быть пустым, а S3 route будет работать через `input_uri`/aliases.

## 6. Как реализую S3 route/save_path resolver

Эволюционирую `save_path` в storage-neutral resolver:

- local API оставлю для старого `file_ops`;
- добавлю S3 API, который принимает storage config и active stage config;
- нормализует `main_dir/...`, legacy `/main_dir/...` и `s3://bucket/prefix`;
- rejects empty, `..`, Windows drive, UNC, unknown bucket, unknown prefix, ambiguous aliases;
- возвращает target stage и `ArtifactLocation`/S3 prefix;
- не пишет files и не создает routes, которых нет в config.

## 7. Как реализую manifest parser

Добавлю отдельный module `src-tauri/src/s3_manifest.rs`:

- parse `beehive.s3_artifact_manifest.v1`;
- validate `schema`, `run_id`, source bucket/key, `status`;
- validate success/error manifests;
- reject obvious business payload fields в root/output objects;
- validate output bucket and `save_path` через S3 route resolver;
- allow zero outputs only when stage is terminal/no-output.

## 8. Как реализую S3-mode n8n trigger без business JSON

Добавлю S3 executor branch рядом с local path:

- task source берется из DB artifact metadata, а не из local JSON payload;
- `stage_runs.request_json` хранит technical audit envelope;
- HTTP `POST` идет с empty body;
- headers содержат `X-Beehive-Workspace-Id`, `X-Beehive-Run-Id`, `X-Beehive-Stage-Id`, `X-Beehive-Source-Bucket`, `X-Beehive-Source-Key`, optional version/etag и `X-Beehive-Manifest-Prefix`;
- response body трактуется как technical manifest.

B1 не делает real S3 calls.

## 9. Как зарегистрирую output artifact pointers

Добавлю registration function для manifest outputs:

- resolve `save_path` -> target stage;
- insert/update `entity_files` row с `storage_provider = s3`, bucket/key metadata и synthetic technical `file_path` вроде `s3://bucket/key`;
- `payload_json` остается `{}` или technical empty placeholder, потому что business JSON не читается из S3;
- `entity_stage_states` для target stage становится `pending`;
- связь source/producer хранится через `source_artifact_id` и `producer_run_id`.

## 10. Какие tests добавлю

- Config parsing: old local valid, S3 valid, missing bucket invalid, invalid `input_uri` invalid, S3 stage without `input_folder` valid.
- Route resolver: relative logical route, legacy `/main_dir`, `s3://bucket/prefix`, unknown bucket/prefix, Windows/UNC/`..`, ambiguous aliases.
- Manifest parser: valid success/error, wrong schema, source mismatch, output bucket mismatch, output route mismatch, business payload fields.
- Executor S3 mock: empty body, required headers, success manifest registers child pointer, error manifest schedules retry/failure, invalid save_path blocks, terminal no-output success marks done.
- Backward compatibility: retained local payload-only and local save_path/next_stage behavior.

## 11. Какие команды запущу

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
git diff --check
```

Если `cargo` отсутствует, зафиксирую точный failure в feedback и не буду утверждать, что Rust tests прошли.

## 12. Какие риски вижу

- Schema v5 надо сделать аккуратно, чтобы существующие v4 DB мигрировали без потери local rows.
- UI stage editor может пока не быть полноценным S3 editor; если он сохраняет YAML, storage fields нельзя молча потерять.
- Discovery остается local-only, поэтому S3 artifacts в B1 будут регистрироваться через test/helper path, не через real S3 scan.
- Rust toolchain в окружении может отсутствовать, как на B0.

## 13. Что точно не буду делать в B1

- Real S3 List/Get/Put calls.
- Credential manager UI.
- n8n workflow management через n8n REST API.
- High-load scheduler/worker pool.
- Broad UI redesign.
- Reading S3 business JSON in execution path.
- Sending business JSON to n8n in S3 mode.
- Production source selection через n8n Search bucket.
- Silent acceptance of unknown `save_path`.

## 14. Чеклист выполнения

- [ ] `after_plan` reread.
- [ ] Storage/config model added.
- [ ] `after_config_model` reread.
- [ ] S3 route resolver added.
- [ ] `after_route_resolver` reread.
- [ ] S3 manifest model/validator added.
- [ ] `after_manifest_model` reread.
- [ ] S3 executor branch and mock tests added.
- [ ] `after_executor_s3_mode` reread.
- [ ] Tests/format/build attempted.
- [ ] `after_tests` reread.
- [ ] Architecture/contract docs updated.
- [ ] Feedback written.
- [ ] `before_feedback` reread recorded.
