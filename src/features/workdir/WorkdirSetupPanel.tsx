import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useState } from "react";

import { useBootstrap } from "../../app/BootstrapContext";

export function WorkdirSetupPanel() {
  const { initializeWorkdir, openWorkdir, isBusy, lastActionError, state } = useBootstrap();
  const [path, setPath] = useState(state.selected_workdir_path ?? "");

  async function chooseDirectory() {
    const selected = await openDialog({
      directory: true,
      multiple: false,
      title: "Select beehive workdir",
    });

    if (typeof selected === "string") {
      setPath(selected);
    }
  }

  async function submit(action: "initialize" | "open") {
    const trimmed = path.trim();
    if (!trimmed) {
      return;
    }

    if (action === "initialize") {
      await initializeWorkdir(trimmed);
    } else {
      await openWorkdir(trimmed);
    }
  }

  return (
    <section className="panel setup-panel">
      <div className="panel-heading">
        <h2>Workdir Setup</h2>
        <span className="muted">Open or initialize a local pipeline workspace</span>
      </div>
      <div className="form-row">
        <label htmlFor="workdir-path">Workdir path</label>
        <div className="path-control">
          <input
            id="workdir-path"
            value={path}
            onChange={(event) => setPath(event.target.value)}
            placeholder="F:\\path\\to\\beehive-workdir"
          />
          <button type="button" className="button secondary" onClick={() => void chooseDirectory()}>
            Browse
          </button>
        </div>
      </div>
      <div className="button-row">
        <button
          type="button"
          className="button primary"
          disabled={isBusy || !path.trim()}
          onClick={() => void submit("initialize")}
        >
          Initialize New Workdir
        </button>
        <button
          type="button"
          className="button secondary"
          disabled={isBusy || !path.trim()}
          onClick={() => void submit("open")}
        >
          Open Existing Workdir
        </button>
      </div>
      {lastActionError ? <p className="error-text">{lastActionError}</p> : null}
    </section>
  );
}
