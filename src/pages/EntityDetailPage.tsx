import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { InfoGrid } from "../components/InfoGrid";
import { StatusBadge } from "../components/StatusBadge";
import { ValidationIssues } from "../components/ValidationIssues";
import { formatDateTime, shortChecksum } from "../lib/formatters";
import { getEntity } from "../lib/runtimeApi";
import type { CommandErrorInfo, EntityDetailPayload } from "../types/domain";

export function EntityDetailPage() {
  const { entityId } = useParams();
  const { state } = useBootstrap();
  const [detail, setDetail] = useState<EntityDetailPayload | null>(null);
  const [errors, setErrors] = useState<CommandErrorInfo[]>([]);
  const [isLoading, setIsLoading] = useState(false);

  const workdirPath = state.selected_workdir_path;
  const canQueryRuntime = state.phase === "fully_initialized" && !!workdirPath && !!entityId;

  useEffect(() => {
    async function loadDetail() {
      if (!canQueryRuntime || !workdirPath || !entityId) {
        setDetail(null);
        setErrors([]);
        return;
      }

      setIsLoading(true);
      try {
        const result = await getEntity(workdirPath, entityId);
        setDetail(result.detail);
        setErrors(result.errors);
      } finally {
        setIsLoading(false);
      }
    }

    void loadDetail();
  }, [canQueryRuntime, entityId, workdirPath]);

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Runtime</span>
          <h1>Entity Detail</h1>
        </div>
      </div>
      <CommandErrorsPanel title="Entity Detail Errors" errors={errors} />
      {!canQueryRuntime ? (
        <section className="panel">
          <h2>{entityId ?? "No entity selected"}</h2>
          <p className="empty-text">
            Open a fully initialized workdir and select an entity from the Entities table.
          </p>
        </section>
      ) : isLoading ? (
        <section className="panel">
          <p className="empty-text">Loading entity detail...</p>
        </section>
      ) : detail ? (
        <>
          <section className="panel">
            <div className="panel-heading">
              <h2>{detail.entity.entity_id}</h2>
              <div className="button-row">
                <StatusBadge status={detail.entity.status} />
                <StatusBadge status={detail.entity.validation_status} />
              </div>
            </div>
            <InfoGrid
              items={[
                { label: "File name", value: detail.entity.file_name },
                { label: "Stage", value: detail.entity.stage_id },
                { label: "File path", value: detail.entity.file_path },
                { label: "Checksum", value: shortChecksum(detail.entity.checksum) },
                { label: "File size", value: detail.entity.file_size },
                { label: "Modified at", value: formatDateTime(detail.entity.file_mtime) },
                { label: "Discovered at", value: formatDateTime(detail.entity.discovered_at) },
                { label: "Updated at", value: formatDateTime(detail.entity.updated_at) },
                { label: "Current stage", value: detail.entity.current_stage },
                { label: "Next stage", value: detail.entity.next_stage },
              ]}
            />
          </section>
          <ValidationIssues
            title="Validation Issues"
            issues={detail.entity.validation_errors}
            emptyText="No validation issues recorded for this entity."
          />
          <section className="panel">
            <div className="panel-heading">
              <h2>JSON Preview</h2>
              <span className="muted">Read-only reconstructed entity payload</span>
            </div>
            <pre className="json-preview">{detail.json_preview}</pre>
          </section>
          <section className="panel">
            <div className="panel-heading">
              <h2>Stage State Rows</h2>
              <span className="muted">{detail.stage_states.length} record(s)</span>
            </div>
            {detail.stage_states.length === 0 ? (
              <p className="empty-text">No stage state rows were found for this entity.</p>
            ) : (
              <div className="table-wrap">
                <table>
                  <thead>
                    <tr>
                      <th>Stage</th>
                      <th>Status</th>
                      <th>Attempts</th>
                      <th>Max attempts</th>
                      <th>File path</th>
                      <th>Last error</th>
                      <th>Updated</th>
                    </tr>
                  </thead>
                  <tbody>
                    {detail.stage_states.map((stageState) => (
                      <tr key={stageState.id}>
                        <td>{stageState.stage_id}</td>
                        <td>
                          <StatusBadge status={stageState.status} />
                        </td>
                        <td>{stageState.attempts}</td>
                        <td>{stageState.max_attempts}</td>
                        <td>
                          <code>{stageState.file_path}</code>
                        </td>
                        <td>{stageState.last_error ?? "Not available"}</td>
                        <td>{formatDateTime(stageState.updated_at)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </section>
        </>
      ) : (
        <section className="panel">
          <h2>{entityId ?? "No entity selected"}</h2>
          <p className="empty-text">Entity detail is not available for the selected ID.</p>
        </section>
      )}
    </div>
  );
}
