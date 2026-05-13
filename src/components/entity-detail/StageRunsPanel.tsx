import { Fragment, useState } from "react";

import { formatDateTime } from "../../lib/formatters";
import { listStageRunOutputs } from "../../lib/runtimeApi";
import type {
  CommandErrorInfo,
  StageRunOutputsPayload,
  StageRunRecord,
} from "../../types/domain";
import { StatusBadge } from "../StatusBadge";

interface StageRunsPanelProps {
  runs: StageRunRecord[];
  workspaceId?: string | null;
}

export function StageRunsPanel({ runs, workspaceId }: StageRunsPanelProps) {
  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>Stage Runs</h2>
        <span className="muted">{runs.length} audit row(s)</span>
      </div>
      {runs.length === 0 ? (
        <p className="empty-text">No execution attempts have been recorded for this entity.</p>
      ) : (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Run</th>
                <th>Stage</th>
                <th>Attempt</th>
                <th>Result</th>
                <th>HTTP</th>
                <th>Error</th>
                <th>Started</th>
                <th>Finished</th>
                <th>Duration</th>
                <th>Snapshots</th>
              </tr>
            </thead>
            <tbody>
              {runs.map((run) => (
                <StageRunRow key={run.id} run={run} workspaceId={workspaceId} />
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function StageRunRow({
  run,
  workspaceId,
}: {
  run: StageRunRecord;
  workspaceId?: string | null;
}) {
  const [isExpanded, setIsExpanded] = useState(false);
  const [isLoadingOutputs, setIsLoadingOutputs] = useState(false);
  const [outputs, setOutputs] = useState<StageRunOutputsPayload | null>(null);
  const [errors, setErrors] = useState<CommandErrorInfo[]>([]);

  async function toggleOutputs() {
    const nextExpanded = !isExpanded;
    setIsExpanded(nextExpanded);
    if (!nextExpanded || outputs || !workspaceId) return;
    setIsLoadingOutputs(true);
    try {
      const result = await listStageRunOutputs(workspaceId, run.run_id);
      setOutputs(result.payload);
      setErrors(result.errors);
    } finally {
      setIsLoadingOutputs(false);
    }
  }

  return (
    <Fragment>
      <tr>
        <td>
          <code>{run.run_id}</code>
        </td>
        <td>{run.stage_id}</td>
        <td>{run.attempt_no}</td>
        <td>
          <StatusBadge status={run.success ? "done" : "failed"} />
        </td>
        <td>{run.http_status ?? "Not available"}</td>
        <td>{run.error_message ?? run.error_type ?? "None"}</td>
        <td>{formatDateTime(run.started_at)}</td>
        <td>{formatDateTime(run.finished_at)}</td>
        <td>{run.duration_ms ?? "Not available"}</td>
        <td>
          <details>
            <summary>JSON</summary>
            <pre className="json-preview compact-json">{run.request_json}</pre>
            {run.response_json ? (
              <pre className="json-preview compact-json">{run.response_json}</pre>
            ) : null}
          </details>
          {workspaceId ? (
            <button
              type="button"
              className="button secondary compact-action"
              onClick={() => void toggleOutputs()}
            >
              {isLoadingOutputs ? "Loading outputs..." : isExpanded ? "Hide outputs" : "Outputs"}
            </button>
          ) : null}
        </td>
      </tr>
      {isExpanded ? (
        <tr>
          <td colSpan={10}>
            {errors.length > 0 ? (
              <div className="error-text">{errors.map((error) => error.message).join(" ")}</div>
            ) : outputs ? (
              <div className="stage-run-output-list">
                <div className="inline-meta">
                  <span>{outputs.output_count} output artifact(s)</span>
                  <span>{outputs.run_id}</span>
                </div>
                {outputs.outputs.length === 0 ? (
                  <p className="empty-text">No child artifacts were registered for this run.</p>
                ) : (
                  <table className="nested-table">
                    <thead>
                      <tr>
                        <th>Entity</th>
                        <th>Artifact</th>
                        <th>Stage</th>
                        <th>Status</th>
                        <th>Relation</th>
                        <th>S3 URI</th>
                      </tr>
                    </thead>
                    <tbody>
                      {outputs.outputs.map((output) => (
                        <tr key={output.entity_file_id}>
                          <td>{output.entity_id}</td>
                          <td>{output.artifact_id ?? "Not available"}</td>
                          <td>{output.target_stage_id}</td>
                          <td>
                            {output.runtime_status ? (
                              <StatusBadge status={output.runtime_status} />
                            ) : (
                              "Not available"
                            )}
                          </td>
                          <td>{output.relation_to_source ?? "Not available"}</td>
                          <td>
                            <code>{output.s3_uri ?? "Not available"}</code>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </div>
            ) : (
              <p className="empty-text">Loading run outputs...</p>
            )}
          </td>
        </tr>
      ) : null}
    </Fragment>
  );
}
