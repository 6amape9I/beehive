import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { listRegisteredWorkspaces } from "../lib/bootstrapApi";
import type { CommandErrorInfo, WorkspaceDescriptor } from "../types/domain";

export function WorkspaceSelectorPage() {
  const { state, isBusy, openRegisteredWorkspace } = useBootstrap();
  const navigate = useNavigate();
  const [workspaces, setWorkspaces] = useState<WorkspaceDescriptor[]>([]);
  const [errors, setErrors] = useState<CommandErrorInfo[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [openingWorkspaceId, setOpeningWorkspaceId] = useState<string | null>(null);
  const selectedWorkspaceId = state.selected_workspace_id;

  async function loadWorkspaces() {
    setIsLoading(true);
    try {
      const result = await listRegisteredWorkspaces();
      setWorkspaces(result.workspaces);
      setErrors(result.errors);
    } finally {
      setIsLoading(false);
    }
  }

  useEffect(() => {
    void loadWorkspaces();
  }, []);

  const sortedWorkspaces = useMemo(
    () => [...workspaces].sort((left, right) => left.name.localeCompare(right.name)),
    [workspaces],
  );

  async function selectWorkspace(workspaceId: string) {
    setOpeningWorkspaceId(workspaceId);
    try {
      await openRegisteredWorkspace(workspaceId);
      navigate("/workspace");
    } finally {
      setOpeningWorkspaceId(null);
    }
  }

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Registry</span>
          <h1>Workspaces</h1>
          <span className="muted">Choose a server-registered S3 workspace.</span>
        </div>
        <div className="button-row">
          <button
            type="button"
            className="button secondary"
            disabled={isLoading}
            onClick={() => void loadWorkspaces()}
          >
            {isLoading ? "Refreshing..." : "Refresh"}
          </button>
        </div>
      </div>

      <CommandErrorsPanel title="Workspace Registry Errors" errors={errors} />

      {sortedWorkspaces.length === 0 ? (
        <section className="panel">
          <p className="empty-text">
            {isLoading ? "Loading workspaces..." : "No registered workspaces were found."}
          </p>
        </section>
      ) : (
        <div className="workspace-card-grid">
          {sortedWorkspaces.map((workspace) => {
            const isSelected = selectedWorkspaceId === workspace.id;
            const isOpening = openingWorkspaceId === workspace.id;
            return (
              <section key={workspace.id} className="panel workspace-card">
                <div className="panel-heading">
                  <div>
                    <h2>{workspace.name}</h2>
                    <span className="muted">{workspace.id}</span>
                  </div>
                  <span className={isSelected ? "status-pill status-done" : "status-pill"}>
                    {isSelected ? "Selected" : workspace.provider}
                  </span>
                </div>
                <div className="inline-meta">
                  <span>{workspace.bucket ?? "no bucket"}</span>
                  <span>{workspace.workspace_prefix ?? "no prefix"}</span>
                  <span>{workspace.region ?? "no region"}</span>
                </div>
                <p className="muted">{workspace.endpoint ?? "Default endpoint"}</p>
                <div className="button-row">
                  <button
                    type="button"
                    className="button primary"
                    disabled={isBusy || isOpening}
                    onClick={() => void selectWorkspace(workspace.id)}
                  >
                    {isOpening ? "Opening..." : isSelected ? "Open workspace" : "Select workspace"}
                  </button>
                </div>
              </section>
            );
          })}
        </div>
      )}
    </div>
  );
}
