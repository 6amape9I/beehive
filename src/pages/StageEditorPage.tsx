import { useEffect, useState } from "react";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { StatusBadge } from "../components/StatusBadge";
import { formatDateTime } from "../lib/formatters";
import { listStages } from "../lib/runtimeApi";
import type { CommandErrorInfo, StageRecord } from "../types/domain";

export function StageEditorPage() {
  const { state } = useBootstrap();
  const [stages, setStages] = useState<StageRecord[]>([]);
  const [errors, setErrors] = useState<CommandErrorInfo[]>([]);
  const [isLoading, setIsLoading] = useState(false);

  const workdirPath = state.selected_workdir_path;
  const canQueryRuntime = state.phase === "fully_initialized" && !!workdirPath;

  useEffect(() => {
    async function loadStages() {
      if (!canQueryRuntime || !workdirPath) {
        setStages([]);
        setErrors([]);
        return;
      }

      setIsLoading(true);
      try {
        const result = await listStages(workdirPath);
        setStages(result.stages);
        setErrors(result.errors);
      } finally {
        setIsLoading(false);
      }
    }

    void loadStages();
  }, [canQueryRuntime, workdirPath]);

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Configuration</span>
          <h1>Stage Editor</h1>
        </div>
      </div>
      <CommandErrorsPanel title="Stage Query Errors" errors={errors} />
      <section className="panel">
        <div className="panel-heading">
          <h2>Stage Definitions</h2>
          <span className="muted">
            {isLoading ? "Loading..." : `${stages.length} stage(s)`}
          </span>
        </div>
        {!canQueryRuntime ? (
          <p className="empty-text">Open or initialize a valid workdir to view runtime stage data.</p>
        ) : stages.length === 0 ? (
          <p className="empty-text">No stages were found for this workdir.</p>
        ) : (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Stage ID</th>
                  <th>Lifecycle</th>
                  <th>Input</th>
                  <th>Output</th>
                  <th>Workflow URL</th>
                  <th>Retry</th>
                  <th>Next</th>
                  <th>Entities</th>
                  <th>Last seen in config</th>
                </tr>
              </thead>
              <tbody>
                {stages.map((stage) => (
                  <tr key={stage.id}>
                    <td>
                      <strong>{stage.id}</strong>
                    </td>
                    <td>
                      <div className="stacked-cell">
                        <StatusBadge status={stage.is_active ? "active" : "inactive"} />
                        {stage.archived_at ? (
                          <span className="muted">Archived {formatDateTime(stage.archived_at)}</span>
                        ) : null}
                      </div>
                    </td>
                    <td>{stage.input_folder}</td>
                    <td>{stage.output_folder.trim() ? stage.output_folder : "Not required"}</td>
                    <td>
                      <code>{stage.workflow_url}</code>
                    </td>
                    <td>
                      {stage.max_attempts} attempts / {stage.retry_delay_sec}s
                    </td>
                    <td>{stage.next_stage ?? "End"}</td>
                    <td>{stage.entity_count}</td>
                    <td>{formatDateTime(stage.last_seen_in_config_at)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  );
}
