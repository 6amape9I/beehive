import { useEffect, useState } from "react";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { StatusBadge } from "../components/StatusBadge";
import { formatDateTime } from "../lib/formatters";
import { getWorkspaceExplorer } from "../lib/runtimeApi";
import type { CommandErrorInfo, WorkspaceStageGroup } from "../types/domain";

export function WorkspaceExplorerPage() {
  const { state } = useBootstrap();
  const [groups, setGroups] = useState<WorkspaceStageGroup[]>([]);
  const [errors, setErrors] = useState<CommandErrorInfo[]>([]);
  const [isLoading, setIsLoading] = useState(false);

  const workdirPath = state.selected_workdir_path;
  const canQueryRuntime = state.phase === "fully_initialized" && !!workdirPath;

  useEffect(() => {
    async function loadExplorer() {
      if (!canQueryRuntime || !workdirPath) {
        setGroups([]);
        setErrors([]);
        return;
      }

      setIsLoading(true);
      try {
        const result = await getWorkspaceExplorer(workdirPath);
        setGroups(result.groups);
        setErrors(result.errors);
      } finally {
        setIsLoading(false);
      }
    }

    void loadExplorer();
  }, [canQueryRuntime, workdirPath]);

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Filesystem</span>
          <h1>Workspace Explorer</h1>
        </div>
      </div>
      <CommandErrorsPanel title="Workspace Explorer Errors" errors={errors} />
      {!canQueryRuntime ? (
        <section className="panel">
          <p className="empty-text">Open or initialize a valid workdir to inspect stage folders and discoveries.</p>
        </section>
      ) : isLoading ? (
        <section className="panel">
          <p className="empty-text">Loading workspace explorer...</p>
        </section>
      ) : groups.length === 0 ? (
        <section className="panel">
          <p className="empty-text">No stage groups are available for this workdir.</p>
        </section>
      ) : (
        groups.map((group) => (
          <section className="panel" key={group.stage.id}>
            <div className="panel-heading">
              <div>
                <h2>{group.stage.id}</h2>
                <span className="muted">{group.stage.input_folder}</span>
              </div>
              <div className="button-row">
                <StatusBadge status={group.stage.is_active ? "active" : "inactive"} />
                <span className="muted">
                  {group.files.length} registered / {group.invalid_files.length} invalid
                </span>
              </div>
            </div>
            <div className="explorer-split">
              <div>
                <h3>Registered JSON files</h3>
                {group.files.length === 0 ? (
                  <p className="empty-text">No registered JSON files for this stage.</p>
                ) : (
                  <div className="table-wrap">
                    <table>
                      <thead>
                        <tr>
                          <th>Entity</th>
                          <th>File</th>
                          <th>Status</th>
                          <th>Validation</th>
                          <th>Updated</th>
                        </tr>
                      </thead>
                      <tbody>
                        {group.files.map((file) => (
                          <tr key={`${group.stage.id}-${file.entity_id}-${file.file_path}`}>
                            <td>{file.entity_id}</td>
                            <td>
                              <code>{file.file_path}</code>
                            </td>
                            <td>
                              <StatusBadge status={file.status} />
                            </td>
                            <td>
                              <StatusBadge status={file.validation_status} />
                            </td>
                            <td>{formatDateTime(file.updated_at)}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
              </div>
              <div>
                <h3>Invalid or unregistered files</h3>
                {group.invalid_files.length === 0 ? (
                  <p className="empty-text">No invalid discovery items were recorded for the latest scan.</p>
                ) : (
                  <div className="issue-list">
                    {group.invalid_files.map((item) => (
                      <article className="issue-row" key={`${item.file_path}-${item.code}`}>
                        <StatusBadge status="error" />
                        <div>
                          <strong>{item.code}</strong>
                          <p>{item.message}</p>
                          <code>{item.file_path}</code>
                          <p className="muted">{formatDateTime(item.created_at)}</p>
                        </div>
                      </article>
                    ))}
                  </div>
                )}
              </div>
            </div>
          </section>
        ))
      )}
    </div>
  );
}
