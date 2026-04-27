import { formatDateTime } from "../../lib/formatters";
import type { StageRunRecord } from "../../types/domain";
import { StatusBadge } from "../StatusBadge";

interface StageRunsPanelProps {
  runs: StageRunRecord[];
}

export function StageRunsPanel({ runs }: StageRunsPanelProps) {
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
                <tr key={run.id}>
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
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

