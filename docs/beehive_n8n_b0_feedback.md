# Beehive n8n B0 Feedback

## 1. Что сделано

- Создан обязательный план `docs/beehive_n8n_b0_plan.md` до runtime code edits.
- Runtime request contract изменён на payload-only: HTTP body и `stage_runs.request_json` теперь строятся из `source_file.payload_json`.
- Wrapper с Beehive metadata больше не отправляется в n8n.
- Response handling поддерживает:
  - root array of business objects;
  - existing wrapper object with `success` / `payload` / `meta`;
  - single direct business object.
- Output files теперь создаются при наличии response output objects, даже если source stage не имеет `next_stage`.
- Добавлен безопасный `save_path` resolver в `src-tauri/src/save_path.rs`.
- `save_path` routes output item в matched active stage `input_folder`.
- Multi-output response может писать в разные target stages.
- Unsafe, unknown, ambiguous, empty, non-string routes блокируют execution без target file writes.
- Existing `next_stage` fallback сохранён для output objects без `save_path`.
- Terminal no-output success сохранён для wrapper success без output objects.
- No-silent-loss правило добавлено: output object без `save_path` и без `next_stage` блокирует execution.
- Добавлена документация `docs/n8n_contract.md`.

## 2. Изменённые файлы

- `src-tauri/src/executor/mod.rs`
- `src-tauri/src/file_ops/mod.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/save_path.rs`
- `docs/beehive_n8n_b0_plan.md`
- `docs/n8n_contract.md`
- `docs/beehive_n8n_b0_feedback.md`

## 3. Какие требования выполнены

- Payload-only request implemented.
- Runtime metadata remains in SQLite/run records, not in n8n request body.
- `stage_runs.request_json` stores the actual payload-only JSON.
- Root array response remains supported.
- Wrapper `success/payload/meta` response remains supported.
- Direct business object response is supported.
- `save_path` routing matches active stage input folders.
- Legacy `/main_dir/...` is treated as logical `main_dir/...`.
- Unsafe save paths are rejected.
- Multi-route outputs are planned before writes.
- Target files remain Beehive-wrapped artifacts with local `meta.beehive`.
- Target stage states are registered as pending through existing target-file registration.
- Existing `next_stage` fallback remains covered.
- Source JSON is still read/preflighted but not mutated by execution.

## 4. Как изменился n8n request contract

Before B0, Beehive sent a wrapper with `entity_id`, `stage_id`, `entity_file_id`, `attempt`, `run_id`, `payload`, and `meta.beehive`.

After B0, Beehive sends only:

```text
serde_json::from_str(source_file.payload_json)
```

`Content-Type: application/json` and `Accept: application/json` remain unchanged.

## 5. Как работает save_path routing

For each output object:

- if `save_path` is a string, it is normalized and matched to an active stage `input_folder`;
- if `save_path` is absent, Beehive falls back to source file/stage `next_stage`;
- if neither route exists, execution is blocked;
- generated target files are written under `workdir / matched_stage.input_folder`.

## 6. Как обрабатываются unsafe paths

Rejected route forms include:

- empty string;
- `..` components;
- OS absolute paths such as `/etc/passwd`;
- Windows drive paths such as `C:\Users\bad\file`;
- UNC paths such as `\\server\share`;
- paths that do not match active stage input folders.

The response copy path returns `FileCopyStatus::Blocked`, `executor` records a failed `stage_run` with `error_type = copy_blocked`, and the source stage state becomes `blocked`.

## 7. Backward compatibility через next_stage

If n8n returns output objects without `save_path`, Beehive still routes them through the existing `next_stage` behavior. Existing multi-output next-stage copying is retained.

## 8. Tests добавлены/изменены

Added/updated Rust tests cover:

- payload-only request body and forbidden metadata absence;
- array output routing to multiple `save_path` stages;
- direct object response routing by `save_path`;
- legacy `/main_dir/...` logical route;
- unsafe `save_path` rejection;
- `next_stage` fallback;
- no silent output loss when a terminal stage returns output objects without route.

## 9. Команды запускались

```bash
cargo test --manifest-path src-tauri/Cargo.toml --no-run
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
git diff --check
```

## 10. Результаты команд

- `cargo test --manifest-path src-tauri/Cargo.toml --no-run`: failed, `/bin/bash: line 1: cargo: command not found`.
- `cargo fmt --manifest-path src-tauri/Cargo.toml`: failed, `/bin/bash: line 1: cargo: command not found`.
- `cargo test --manifest-path src-tauri/Cargo.toml`: failed, `/bin/bash: line 1: cargo: command not found`.
- `npm run build`: passed. `tsc && vite build`, 86 modules transformed, build completed.
- `git diff --check`: passed.

## 11. Что не удалось проверить

Rust formatting, compilation, and tests could not be run in this environment because `cargo`, `rustc`, and `rustfmt` are not available in PATH or at `/home/timur/.cargo/bin/cargo` / `/usr/bin/cargo`.

No real n8n endpoint was called.

## 12. Риски

- Rust code was not compiler-verified in this environment.
- Manual formatting was applied where obvious, but `cargo fmt` still needs to run on a machine with Rust installed.
- Direct business objects that contain a top-level `payload` field are still treated as wrapper-style responses for backward compatibility.
- Directory creation uses safe normalized target dirs for B0 writes, but broader pipeline YAML path validation is still split between older config loading and the stage editor.

## 13. Решения реализации

- Added a dedicated `save_path` module instead of embedding route parsing into `file_ops`.
- Chose all-or-nothing response planning before file writes.
- Kept route failures as blocked runtime states, not retryable network-style failures.
- Kept existing `next_stage` fallback for older workflows.
- Kept local `meta.beehive` in generated artifacts because request execution is now payload-only.

## 14. Acceptance self-check

- [x] request body payload-only
- [x] no Beehive metadata sent to n8n
- [x] stage_runs still record audit
- [x] array response works by implementation/tests
- [x] direct object response works by implementation/tests
- [x] wrapper response works by retained implementation/tests
- [x] save_path route works by implementation/tests
- [x] multi-route output works by implementation/tests
- [x] unsafe save_path rejected by implementation/tests
- [x] next_stage fallback works by retained implementation/tests
- [x] source JSON not mutated by retained preflight/execution path
- [x] target JSON Beehive-wrapped
- [ ] cargo fmt done: blocked, `cargo` not found
- [ ] cargo test done: blocked, `cargo` not found
- [x] npm run build attempted and passed
- [x] docs/n8n_contract.md created
- [x] feedback created

## 15. Что передать следующему этапу

Run the Rust verification on a machine/session with Rust available:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

Then do a manual local/mock n8n smoke through the UI or backend commands with a `save_path` branching response.

Главный output следующего этапа:
working Beehive runtime that can send payload-only data to n8n and save outputs by save_path.

ТЗ перечитано на этапах: after_plan, after_request_contract_change, after_response_contract_change, after_save_path_routing, after_filesystem_safety, after_tests, before_feedback
