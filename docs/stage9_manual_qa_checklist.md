# beehive — Stage 9 Manual QA Checklist

# Full Manual Verification Before Demo / Internal Use

This checklist is mandatory for Stage 9 acceptance.

Use it after Codex has implemented Stage 9 and pushed code. Fill `docs/stage9_manual_qa_results.md` with actual results.

Do not mark a section as passed unless it was actually checked.

Recommended result values:

```text
PASS
FAIL
N/A
BLOCKED
```

For every FAIL/BLOCKED item, record:

- exact step;
- actual result;
- expected result;
- screenshot/recording reference if available;
- GitHub issue or follow-up note.

---

## 0. Test environment

Record in `docs/stage9_manual_qa_results.md`:

- Date/time:
- Tester:
- OS:
- CPU/RAM, if relevant:
- Commit SHA:
- Branch:
- Node version:
- npm version:
- Rust version:
- Tauri build environment:
- Whether real n8n endpoint was reachable:
- Notes:

---

## 1. Fresh checkout / one-action launch

### 1.1. Install dependencies

Command:

```powershell
npm.cmd install
```

Expected:

- dependencies install without fatal errors.

Result:

```text
[ ] PASS [ ] FAIL [ ] N/A
```

Notes:

### 1.2. One-action app launch

Command:

```powershell
npm.cmd run app
```

or documented equivalent.

Expected:

- Tauri desktop app opens;
- no blank screen;
- no console fatal error.

Result:

```text
[ ] PASS [ ] FAIL [ ] N/A
```

Notes:

### 1.3. Demo reset

Command:

```powershell
npm.cmd run demo:reset
```

Expected:

- demo workdir is recreated or reset;
- `demo/workdir/pipeline.yaml` exists;
- `demo/workdir/stages/incoming` contains demo JSON files;
- `demo/workdir/stages/n8n_output` is empty/clean;
- `demo/workdir/stages/review` is empty/clean;
- no stale `app.db` unless intentionally created by reset.

Result:

```text
[ ] PASS [ ] FAIL [ ] N/A
```

Notes:

### 1.4. Demo launch, if provided

Command:

```powershell
npm.cmd run demo
```

Expected:

- app starts;
- demo workdir is ready;
- if default workdir preload exists, app opens demo workdir automatically;
- if no preload exists, UI can open `demo/workdir`.

Result:

```text
[ ] PASS [ ] FAIL [ ] N/A
```

Notes:

---

## 2. Demo workdir validation

### 2.1. Demo folder structure

Check:

```text
demo/
  README.md
  workdir/
    pipeline.yaml
    stages/
      incoming/
      n8n_output/
      review/
      invalid_samples/
    logs/
```

Expected:

- required folders exist;
- README explains demo usage.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 2.2. Demo pipeline

Open `demo/workdir/pipeline.yaml`.

Expected:

- YAML parses;
- project/runtime/stages are present;
- at least `semantic_split` and `review` stages exist;
- `semantic_split.workflow_url` uses:
  `https://n8n-dev.steos.io/webhook/b0c81347-5f51-4142-b1d9-18451d8c4ecf`;
- terminal stage has no required output folder.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 2.3. Demo JSON payload shape

Open several files in `demo/workdir/stages/incoming`.

Expected:

- root has `id`, `current_stage`, `next_stage`, `status`, `payload`;
- `payload` contains business fields only;
- payload does not contain beehive service metadata;
- examples include `керамика`, `горизонт`, `замок`;
- at least 10 demo input files exist.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

---

## 3. Open workdir and bootstrap

### 3.1. Open demo workdir

In app UI:

1. Open Settings / Diagnostics or startup workdir panel.
2. Select `demo/workdir`.

Expected:

- workdir opens;
- config is valid;
- SQLite initializes;
- no fatal errors.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 3.2. Settings / Diagnostics

Expected:

- workdir path shown;
- pipeline config shown as valid;
- schema version shown;
- runtime settings shown;
- app events panel works.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

---

## 4. Scan workspace

### 4.1. First scan

Click `Scan workspace`.

Expected:

- new JSON files are registered;
- invalid files, if present in active input folder, are reported as invalid;
- summary shows registered count;
- no duplicate registration on first scan.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 4.2. Repeat scan

Click `Scan workspace` again.

Expected:

- unchanged files remain unchanged;
- no duplicate entities/files;
- counters remain stable.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 4.3. Missing file detection

Manually move one registered input JSON out of its stage folder, then scan.

Expected:

- file is marked missing, not deleted from SQLite;
- Entities/Detail/Workspace Explorer show missing state.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 4.4. Restore missing file

Move the file back, then scan.

Expected:

- file is marked restored/present;
- no duplicate row is created.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 4.5. Invalid file scenario

Copy invalid sample into active input folder and scan.

Expected:

- invalid file appears in app events and Workspace Explorer invalid section;
- invalid file does not create runnable entity;
- UI gives understandable error.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

---

## 5. Dashboard

### 5.1. Dashboard overview

Open Dashboard.

Expected:

- project context shown;
- stage graph/stage list visible;
- counters per stage visible;
- due/pending tasks visible;
- recent errors panel does not break layout.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 5.2. Dashboard actions

Click Refresh, Scan workspace, Run due tasks, Reconcile stuck.

Expected:

- each action is explicit;
- no automatic n8n run on page load;
- errors/success messages are visible.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

---

## 6. Entities Table

### 6.1. Table loads

Open Entities.

Expected:

- demo entities appear;
- columns show entity id, stage, status, attempts, last error, updated time;
- no empty white screen.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 6.2. Search

Search for:

```text
керамика
```

or corresponding entity id.

Expected:

- matching entity appears;
- non-matching entities hidden.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 6.3. Filters

Test filters:

- stage;
- status;
- validation.

Expected:

- table updates correctly;
- clear filters works.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 6.4. Sorting / pagination

Test sorting and page size.

Expected:

- sorting changes order;
- pagination controls work;
- no crash.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

---

## 7. Entity Detail

### 7.1. Open entity

Click a demo entity.

Expected:

- Entity Detail opens;
- selected entity metadata visible;
- files list visible;
- stage timeline visible;
- stage runs panel visible.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 7.2. JSON viewer

Expected:

- full selected JSON is visible;
- `payload` contains business data;
- root service fields are not directly editable.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 7.3. Payload/meta edit

Edit an allowed pending file payload field.

Example:

- add or change `source_semantic_description`.

Expected:

- save succeeds;
- only payload/meta changes;
- root `id`, `current_stage`, `next_stage`, `status` remain protected;
- DB snapshot updates;
- app event is recorded.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 7.4. Edit locked statuses

Try editing a file with status:

- `queued`;
- `in_progress`;
- `done`.

Expected:

- UI disables edit or backend rejects;
- no file mutation;
- reason is shown.

Result:

```text
[ ] PASS [ ] FAIL [ ] N/A
```

Notes:

### 7.5. Open file/folder

Use Open file and Open folder.

Expected:

- backend opens registered file/folder;
- missing file open is rejected cleanly.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

---

## 8. n8n execution and multi-output

### 8.1. Real n8n endpoint availability

Endpoint:

```text
https://n8n-dev.steos.io/webhook/b0c81347-5f51-4142-b1d9-18451d8c4ecf
```

Run due tasks for demo.

Expected if endpoint works:

- HTTP request is sent;
- response is stored in stage run;
- successful response marks source stage done.

Expected if endpoint fails:

- task enters retry_wait or failed according to retry policy;
- error visible in Dashboard/Entity Detail;
- no unsafe copy is created.

Result:

```text
[ ] PASS [ ] FAIL [ ] BLOCKED
```

Notes:

### 8.2. Multi-output response

Use real endpoint or mock/local scenario that returns:

```json
[
  { "entity_name": "child one" },
  { "entity_name": "child two" },
  { "entity_name": "child three" }
]
```

Expected:

- three different target JSON files are created in next-stage folder;
- each target file has unique root `id`;
- each target file has `current_stage = target stage`;
- each target file has `status = pending`;
- each target file has business output object under `payload`;
- `meta.beehive` contains source/run/output metadata;
- Workspace Explorer shows managed copies/trail.

Result:

```text
[ ] PASS [ ] FAIL [ ] N/A
```

Notes:

### 8.3. Idempotent rerun / compatible existing outputs

Reset source if needed and repeat a run that produces same output.

Expected:

- compatible existing target files are reused or treated idempotently;
- no unsafe duplicate storm;
- no overwrite of different content.

Result:

```text
[ ] PASS [ ] FAIL [ ] N/A
```

Notes:

### 8.4. Invalid n8n response

Use mock/local or configured failing endpoint to return invalid response.

Expected:

- stage run is failed/retry;
- target files are not created;
- UI shows error.

Result:

```text
[ ] PASS [ ] FAIL [ ] N/A
```

Notes:

---

## 9. Retry / failed behavior

### 9.1. Network or HTTP failure

Use a bad workflow URL in a temporary stage or edit demo stage to invalid local URL.

Expected:

- task enters `retry_wait` after first failure;
- attempts increments;
- `next_retry_at` set;
- last_error visible.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 9.2. Attempts exhausted

Set `max_attempts = 1` or use a test stage.

Expected:

- failed task becomes `failed`;
- no next-stage copy;
- error appears in Dashboard/Entity Detail.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 9.3. Manual retry now

On `retry_wait`, click Retry now.

Expected:

- task runs immediately;
- run history records new attempt.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 9.4. Reset to pending

On `failed` or `blocked`, click Reset.

Expected:

- status becomes pending;
- attempts/error fields reset as designed;
- history remains.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 9.5. Skip

On pending/retry_wait, click Skip.

Expected:

- status becomes skipped;
- no n8n call;
- no file copy;
- app event logged.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

---

## 10. Reconciliation / restart

### 10.1. Stale queued

Create or use test helper to seed stale `queued`.

Expected:

- reconciliation releases stale queued safely;
- no duplicate stage run;
- app event visible.

Result:

```text
[ ] PASS [ ] FAIL [ ] N/A
```

Notes:

### 10.2. Stale in_progress with attempts remaining

Create or use test helper to seed stale `in_progress`.

Expected:

- reconciliation moves state to due `retry_wait`;
- app event visible.

Result:

```text
[ ] PASS [ ] FAIL [ ] N/A
```

Notes:

### 10.3. Stale in_progress exhausted

Expected:

- reconciliation moves state to `failed`.

Result:

```text
[ ] PASS [ ] FAIL [ ] N/A
```

Notes:

### 10.4. App restart

Close app and restart.

Expected:

- workdir can reopen;
- SQLite state preserved;
- no entities lost;
- Dashboard/Entities/Workspace Explorer still work.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

---

## 11. Stage Editor

### 11.1. Load editor

Open Stage Editor.

Expected:

- current pipeline loads into draft;
- YAML preview visible;
- validation panel visible;
- no automatic save.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 11.2. Validate

Make a harmless valid edit, then Validate.

Expected:

- valid draft reports valid;
- YAML preview updates.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 11.3. Invalid draft

Create invalid stage id or bad next_stage.

Expected:

- validation error appears;
- save rejected;
- pipeline.yaml not overwritten.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 11.4. Save valid draft

Add a new stage or change retry delay.

Expected:

- pipeline.yaml backup is created;
- new YAML saved;
- SQLite stages synced;
- directories provisioned;
- app reload reflects changes.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 11.5. Remove stage from config

Remove a stage with/without history.

Expected:

- removal is blocked if referenced by next_stage;
- historical SQLite data is not deleted;
- inactive stage remains visible where appropriate.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

---

## 12. Workspace Explorer

### 12.1. Explorer loads

Open Workspace Explorer.

Expected:

- workdir summary visible;
- stage tree visible;
- active/inactive stages visible;
- no automatic scan.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 12.2. Registered files

Expected:

- files appear under correct stage;
- runtime status comes from SQLite;
- validation status visible;
- present/missing visible.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 12.3. Invalid files

After invalid scan scenario:

Expected:

- invalid files appear under stage;
- code/message/path visible.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 12.4. Managed copies and trails

After successful n8n/multi-output run:

Expected:

- target files visible;
- managed copy marker visible;
- artifact trail shows source/target relationship;
- inferred relationships are marked as inferred if not exact.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 12.5. Deep link to Entity Detail

Click Entity from Workspace Explorer.

Expected:

- Entity Detail opens;
- exact `file_id` is selected.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

---

## 13. Load test

### 13.1. Generate 1000 files

Command:

```powershell
npm.cmd run demo:generate -- --count 1000
```

or documented equivalent.

Expected:

- 1000 valid files generated;
- no committed large data required.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 13.2. Scan 1000 files

Run scan.

Record:

- scanned count:
- registered count:
- elapsed ms:
- app responsiveness:

Expected:

- scan completes;
- no crash;
- UI remains usable.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 13.3. Optional 5000-file test

Command:

```powershell
npm.cmd run demo:generate -- --count 5000
```

Expected:

- test is allowed to be marked N/A if machine/time insufficient;
- if run, record elapsed time and UI behavior.

Result:

```text
[ ] PASS [ ] FAIL [ ] N/A
```

Notes:

---

## 14. Release readiness

### 14.1. Formatting

Command:

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml
```

Expected:

- passes.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 14.2. Rust tests

Command:

```powershell
cmd.exe /c 'call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat" >nul && cargo test --manifest-path src-tauri\Cargo.toml'
```

Expected:

- all tests pass.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 14.3. Frontend build

Command:

```powershell
npm.cmd run build
```

Expected:

- TypeScript and Vite build pass.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

### 14.4. Release build

Command:

```powershell
npm.cmd run release
```

Expected:

- release build passes, or exact platform blocker is documented;
- no claim of release readiness if failed.

Result:

```text
[ ] PASS [ ] FAIL [ ] BLOCKED
```

Notes:

---

## 15. Documentation check

Verify docs exist and are readable:

```text
docs/user_guide.md
docs/demo_guide.md
docs/release_checklist.md
docs/stage9_manual_qa_checklist.md
docs/stage9_manual_qa_results.md
```

Expected:

- each doc exists;
- demo guide matches actual scripts and demo folders;
- user guide describes main screens and workflows;
- release checklist has commands and artifacts.

Result:

```text
[ ] PASS [ ] FAIL
```

Notes:

---

## 16. Final acceptance decision

Overall manual QA result:

```text
[ ] PASS
[ ] PASS WITH KNOWN NON-BLOCKING ISSUES
[ ] FAIL
[ ] BLOCKED
```

Blocking issues:

1.
2.
3.

Non-blocking issues:

1.
2.
3.

Final notes:

