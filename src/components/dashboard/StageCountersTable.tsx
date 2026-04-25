import { formatDateTime } from "../../lib/formatters";
import type { DashboardStageCounters } from "../../types/domain";
import { StatusBadge } from "../StatusBadge";

export function StageCountersTable({ counters }: { counters: DashboardStageCounters[] }) {
  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>Stage Counters</h2>
        <span className="muted">SQLite execution state</span>
      </div>
      {counters.length === 0 ? (
        <p className="empty-text">No stage counters are available.</p>
      ) : (
        <div className="table-wrap">
          <table className="dashboard-table">
            <thead>
              <tr>
                <th>Stage</th>
                <th>Active</th>
                <th>Total</th>
                <th>Pending</th>
                <th>Queued</th>
                <th>In progress</th>
                <th>Retry</th>
                <th>Done</th>
                <th>Failed</th>
                <th>Blocked</th>
                <th>Skipped</th>
                <th>Unknown</th>
                <th>Existing files</th>
                <th>Missing</th>
                <th>Last activity</th>
              </tr>
            </thead>
            <tbody>
              {counters.map((counter) => (
                <tr key={counter.stage_id}>
                  <td>{counter.stage_label}</td>
                  <td>
                    <StatusBadge status={counter.is_active ? "active" : "inactive"} />
                  </td>
                  <td>{counter.total}</td>
                  <td>{counter.pending}</td>
                  <td>{counter.queued}</td>
                  <td>{counter.in_progress}</td>
                  <td>{counter.retry_wait}</td>
                  <td>{counter.done}</td>
                  <td>{counter.failed}</td>
                  <td>{counter.blocked}</td>
                  <td>{counter.skipped}</td>
                  <td>{counter.unknown}</td>
                  <td>{counter.existing_files}</td>
                  <td>{counter.missing_files}</td>
                  <td>
                    {formatDateTime(counter.last_finished_at ?? counter.last_started_at)}
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
