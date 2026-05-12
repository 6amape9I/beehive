# Beehive n8n B0 Plan

## 1. Что понял из задачи

B0 меняет runtime contract между Beehive и n8n:

- исходный файл в stage input folder остаётся Beehive artifact с `id`, `current_stage`, `status`, `payload`, `meta`;
- HTTP POST в n8n должен содержать только JSON из `source_file.payload_json`;
- runtime metadata (`entity_id`, `stage_id`, `entity_file_id`, `attempt`, `run_id`, `meta.beehive`) остаётся только в SQLite и локальных артефактах;
- n8n response может быть wrapper object, root array или direct business object;
- output object с `save_path` должен маршрутизироваться в matching active stage `input_folder`;
- output object без `save_path` продолжает использовать старый `next_stage` fallback;
- unsafe or unknown route не должен писать файлы и не должен молча завершать execution как success.

## 2. Текущие места кода для HTTP request

- `src-tauri/src/executor/mod.rs`
  - `execute_task` готовит `request_json`, пишет его в `stage_runs.request_json`, затем передаёт в `call_webhook`.
  - `call_webhook` отправляет POST с `Content-Type: application/json` и `Accept: application/json`.
  - `build_request_json` сейчас строит wrapper с Beehive metadata; это основная точка payload-only изменения.

## 3. Где сейчас строится Beehive metadata wrapper

- `src-tauri/src/executor/mod.rs::build_request_json`
  - парсит `source_file.payload_json` и `source_file.meta_json`;
  - добавляет `meta.beehive.app`, `stage_id`, `entity_file_id`, `attempt`, `run_id`;
  - возвращает wrapper с `entity_id`, `stage_id`, `entity_file_id`, `source_file_path`, `attempt`, `run_id`, `payload`, `meta`.

После B0 эта функция должна возвращать только parsed `source_file.payload_json`.

## 4. Где сейчас response превращается в next-stage files

- `src-tauri/src/executor/mod.rs::validate_response`
  - принимает root array, wrapper `payload` array/object, terminal empty wrapper;
  - сейчас direct business object without `payload` фактически трактуется как wrapper без payload.
- `src-tauri/src/executor/mod.rs::execute_task`
  - вызывает `file_ops::create_next_stage_copies_from_response` только когда `next_stage_required`.
- `src-tauri/src/file_ops/mod.rs::create_next_stage_copies_from_response`
  - всегда вычисляет один target stage через file/stage `next_stage`;
  - создаёт один или несколько child artifacts в этом target stage;
  - регистрирует `entity_files` и `entity_stage_states`.

## 5. Payload-only request implementation

- Изменить `build_request_json` так, чтобы она только парсила и возвращала `source_file.payload_json`.
- Убрать неиспользуемые параметры `task`, `attempt_no`, `run_id` из builder либо оставить с `_` только если это уменьшит churn; предпочтительно упростить сигнатуру.
- Оставить `stage_runs.request_json` как serialized фактически отправленный payload-only body.
- Не менять preflight чтение и checksum check, чтобы source JSON на диске не мутировался и stale source не отправлялся.

## 6. Response contract implementation

- Сохранить wrapper object:
  - `{ "success": false }` -> contract failure;
  - `{ "success": true, "payload": [objects], "meta": object }` -> outputs;
  - `{ "success": true, "payload": object, "meta": object }` -> one output.
- Сохранить root array of objects.
- Добавить direct business object support:
  - object без `success`, без wrapper `payload`, но сам являющийся business output -> one output.
- Сохранять terminal success without output only when response has no output objects and source stage has no `next_stage`.
- Если response has output objects, но route не определить нельзя, execution должен стать blocked/contract/copy error, а не success without output.

## 7. save_path routing implementation

- Добавить `src-tauri/src/save_path.rs` и подключить в `src-tauri/src/lib.rs`.
- Функция route matching:
  - принимает raw `save_path`, `workdir_path`, active stages;
  - trim;
  - normalize separators to `/` for deterministic comparison;
  - allow relative logical path only;
  - allow legacy `/main_dir/...` only as logical `main_dir/...`;
  - reject empty, `..`, Windows drive paths, UNC paths, absolute OS paths other than legacy `/main_dir/...`;
  - match only active `stage.input_folder`;
  - compute target directory under `workdir`;
  - ensure final directory is inside workdir without requiring final file to exist.
- В `file_ops::create_next_stage_copies_from_response` планировать target stage per output item:
  - if item has `save_path`, resolve matched stage;
  - else fallback to source file/stage `next_stage`;
  - if neither route exists while item exists, block/fail before writing.
- Preferred behavior: all-or-nothing. Сначала построить все plans, затем писать файлы.

## 8. Filesystem safety

- Не писать по raw `save_path` напрямую.
- Использовать только `workdir / matched_stage.input_folder / generated_file_name`.
- Не canonicalize non-existing target file; canonicalize existing workdir and existing target parent after directory provisioning.
- Не разрешать `..`, drive prefix, UNC, empty path, unmatched active stage path.
- Не делать case-insensitive matching, чтобы поведение было одинаковым на Windows/Ubuntu.
- Сохранять существующую collision protection через planned path set и existing target compatibility checks.

## 9. Windows/Ubuntu compatibility

- Treat `/main_dir/...` as legacy logical path only, not Linux absolute path.
- Treat `C:\...`, `C:/...`, `\\server\share`, `//server/share` as unsafe.
- Normalize `\` and `/` for logical comparison, but build real paths with `Path::join`.
- Automated tests use Rust tempdir and local TCP mock server only.
- Verification commands are cross-platform backend/build commands:
  - `cargo fmt --manifest-path src-tauri/Cargo.toml`
  - `cargo test --manifest-path src-tauri/Cargo.toml`
  - `npm run build`

## 10. Tests to add or update

Rust tests in `src-tauri/src/executor/mod.rs` and/or `src-tauri/src/save_path.rs`:

- payload-only request body contains source payload only and excludes Beehive metadata;
- array response routes outputs to multiple stages by `save_path`;
- direct object response routes by `save_path`;
- legacy `/main_dir/...` logical path routes to `main_dir/...`;
- unsafe save_path values are rejected without outside write;
- next_stage fallback still creates target artifacts when no `save_path` exists;
- output without `save_path` and without `next_stage` is not silently lost;
- wrapper response remains supported.

Existing tests that assert old wrapper request body or old terminal array semantics must be updated to the B0 contract.

## 11. Commands to run

- `cargo fmt --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `npm run build`

If `npm run build` cannot run due missing dependencies or platform packages, record exact error in feedback.

## 12. Out of scope for B0

- UI redesign.
- Background daemon / run-until-idle service.
- n8n REST API workflow management.
- Credential manager.
- Real n8n calls in automated tests.
- Database schema rewrite unless an unavoidable blocker appears.
- Broad stage editor or dashboard changes.
- Production corpus regeneration.

## 13. Risks and спорные места

- Direct object response must not accidentally reinterpret existing wrapper objects with `success: true` and no `payload` as business output if operator expected terminal no-output success.
- Existing terminal stage test currently accepts root array response without target copy; B0 requires output objects without route to be blocked when no `next_stage`, so this test must change.
- `save_path` matching must be strict enough to reject OS absolute paths but still accept legacy `/main_dir/...`.
- Multi-output all-or-nothing must avoid partially written files when one later item has unsafe route or collision.
- Existing `ResponseCopyPayload.target_stage_id` supports one target only; B0 multi-route may need this field to become less authoritative while keeping callers working.

## 14. Чеклист выполнения

- [x] Read required instructions and required code.
- [x] Create this plan before runtime code edits.
- [x] Reread TЗ at `after_plan`.
- [x] Implement payload-only request.
- [x] Reread TЗ at `after_request_contract_change`.
- [x] Implement direct object response/no silent loss response behavior.
- [x] Reread TЗ at `after_response_contract_change`.
- [x] Implement save_path routing.
- [x] Reread TЗ at `after_save_path_routing`.
- [x] Implement/verify filesystem safety helper.
- [x] Reread TЗ at `after_filesystem_safety`.
- [x] Add/update Rust tests.
- [x] Reread TЗ at `after_tests`.
- [x] Update `docs/n8n_contract.md`.
- [x] Run required verification commands.
- [x] Reread TЗ at `before_feedback`.
- [x] Create `docs/beehive_n8n_b0_feedback.md`.
