# beehive — Stage 1 Polish Review Task

You are not starting Stage 2 yet.

Your task is to perform a focused **Stage 1 polish pass** for the desktop application **beehive** and close the remaining foundation gaps.

## Main goals

1. Make stage synchronization between `pipeline.yaml` and SQLite fully consistent.
2. Make the initialization state model honest and internally consistent.
3. Improve Stage 1 verification through better tests and real manual validation.
4. Re-check the program yourself and report only what you actually verified.

## Required fixes

### 1) Fix stage sync consistency

The current stage sync appears incomplete. It must not silently leave stale stage records in SQLite when stages are removed from `pipeline.yaml`.

You must:

* ensure stage definitions in SQLite reflect the actual YAML state
* handle removed stages deliberately and consistently
* document the chosen behavior clearly

Hard delete or soft delete/deactivation is acceptable, but the behavior must be explicit, deterministic, and justified.

### 2) Fix initialization state model

The current initialization phases and actual bootstrap flow appear inconsistent.

Choose one of these approaches:

* either implement the intermediate states properly in the real bootstrap flow and UI
* or simplify the model and remove dead / unused states

Do not keep dead state variants “for the future” if they are not truly part of the app lifecycle.

### 3) Strengthen Stage 1 tests

Add or improve tests for at least:

* new workdir initialization
* opening existing workdir
* valid config loading
* invalid config handling
* duplicate stage ids
* stage sync updates
* stage sync removal behavior
* app.db creation
* SQLite schema bootstrap
* bootstrap state behavior

## Mandatory requirement: re-check the app yourself

You must personally re-verify that the application works.

You must manually check and report the result of:

1. fresh app launch
2. new workdir initialization
3. creation of `pipeline.yaml`, `app.db`, `stages/`, `logs/`
4. valid config loading
5. stage visibility in UI
6. stage sync into SQLite
7. opening an existing workdir
8. invalid config scenario
9. stage update/removal sync behavior

Do not write things like:

* “should work”
* “likely works”
* “covered by architecture”

Use only:

* “verified manually”
* “reproduced”
* “failed to reproduce because …”
* “fixed and re-tested”

## Documentation updates

Update the relevant Stage 1 docs so they reflect the real post-polish state:

* delivery report
* checklist
* progress log
* README if behavior changed

## Small `.gitignore` review

Perform a small `.gitignore` review.
Consider whether to add ignores for:

* `.vscode/`
* `.env`
* `.env.*`
* `*.db`
* `*.db-shm`
* `*.db-wal`
* `*.sqlite`
* `*.sqlite3`
* `*.tsbuildinfo`

Do not remove useful tracked files like:

* `README.md`
* `docs/`
* `instructions/`
* `package-lock.json`
* `Cargo.lock`

## Out of scope

Do not implement Stage 2 features:

* n8n runtime execution
* retry runtime engine
* file scanning runtime
* entity processing
* stage graph execution
* advanced CRUD/editor work
* orchestration scheduler

## Success criteria

This polish pass is successful only if:

* stage sync is now consistent
* initialization state model is clean and honest
* tests are improved
* the app was manually re-verified by you
* docs reflect reality
* `.gitignore` was reviewed and adjusted if needed
* Stage 1 can be cleanly closed or one final blocker is explicitly identified

## Response format

Return your result in this exact structure:

A. What was fixed
B. Files changed
C. Stage sync behavior after the fix
D. Initialization state model after the fix
E. Tests added/updated
F. What was manually verified by you
G. `.gitignore` changes
H. Remaining blockers for Stage 1
