# beehive Demo Guide

## Start

```powershell
npm.cmd install
npm.cmd run demo:reset
npm.cmd run app
```

Open `demo/workdir` in the app.

## Demo Scripts

- `npm.cmd run app` starts the desktop app.
- `npm.cmd run demo:reset` restores demo workdir.
- `npm.cmd run demo` resets demo and starts the app.
- `npm.cmd run demo:generate -- --count 1000` generates volume demo JSON files.

## Workdir Structure

```text
demo/workdir/
  pipeline.yaml
  stages/incoming/
  stages/n8n_output/
  stages/review/
  stages/invalid_samples/
  logs/
```

The happy path starts with 10 JSON files in `stages/incoming`.

## n8n Endpoint

The demo pipeline uses:

```text
https://n8n-dev.steos.io/webhook/b0c81347-5f51-4142-b1d9-18451d8c4ecf
```

This endpoint is for manual demo only. Automated tests use mock HTTP.

## Happy Path

1. Open `demo/workdir`.
2. Run `Scan workspace`.
3. Confirm demo entities appear in Dashboard and Entities.
4. In Entities, check no-filter rows, stage/status/validation filters, and search by `demo-ceramic-001` or business name `керамика`.
5. Open an Entity Detail row and confirm file metadata, stage state, manual actions, and timeline are visible.
6. Run due tasks.
7. Inspect Entity Detail and Workspace Explorer for managed child files in `stages/n8n_output`.

## Invalid File Scenario

Copy one file from `stages/invalid_samples` into `stages/incoming`, then run `Scan workspace`. The file should be reported in app events and Workspace Explorer invalid-file sections.

## Load Scenario

```powershell
npm.cmd run demo:reset
npm.cmd run demo:generate -- --count 1000
```

Then open the workdir, scan, and record elapsed time/responsiveness in `docs/stage9_manual_qa_results.md`. The 5000-file run is optional and should not be part of default CI.
