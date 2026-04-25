import { formatDateTime } from "../../lib/formatters";
import type { DashboardRunItem } from "../../types/domain";
import { StatusBadge } from "../StatusBadge";

export function RecentRunsPanel({ runs }: { runs: DashboardRunItem[] }) {
  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>Recent Runs</h2>
        <span className="muted">{runs.length} shown</span>
      </div>
      {runs.length === 0 ? (
        <p className="empty-text">No recent runs.</p>
      ) : (
        <div className="table-wrap">
          <table className="dashboard-table">
            <thead>
              <tr>
                <th>Run</th>
                <th>Entity</th>
                <th>Stage</th>
                <th>Result</th>
                <th>HTTP</th>
                <th>Error</th>
                <th>Duration</th>
                <th>Finished</th>
              </tr>
            </thead>
            <tbody>
              {runs.map((run) => (
                <tr key={run.run_id}>
                  <td>{run.run_id}</td>
                  <td>{run.entity_id}</td>
                  <td>{run.stage_id}</td>
                  <td>
                    <StatusBadge status={run.success ? "success" : "failed"} />
                  </td>
                  <td>{run.http_status ?? "Not available"}</td>
                  <td>{run.error_type ?? run.error_message ?? "None"}</td>
                  <td>{run.duration_ms !== null ? `${run.duration_ms} ms` : "Not available"}</td>
                  <td>{formatDateTime(run.finished_at ?? run.started_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
