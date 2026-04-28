# beehive demo workdir

This folder is a resettable Stage 9 demo workspace.

Run:

```powershell
npm.cmd run demo:reset
npm.cmd run app
```

Then open `demo/workdir` in the app.

The demo pipeline has two stages:

- `semantic_split` reads `stages/incoming` and calls the configured n8n webhook.
- `review` reads managed copies created in `stages/n8n_output` and is terminal.

Invalid examples are kept in `demo/workdir/stages/invalid_samples` so the happy path starts clean. Copy one invalid file into `stages/incoming` and run `Scan workspace` to verify invalid-file UX.

The real n8n endpoint is part of the demo pipeline only. Automated tests use local mock HTTP servers and must not call it.
