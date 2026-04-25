import { formatDateTime } from "../../lib/formatters";
import type { DashboardActiveTask } from "../../types/domain";
import { StatusBadge } from "../StatusBadge";

export function ActiveTasksPanel({ tasks }: { tasks: DashboardActiveTask[] }) {
  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>Active Tasks</h2>
        <span className="muted">{tasks.length} shown</span>
      </div>
      {tasks.length === 0 ? (
        <p className="empty-text">No active tasks.</p>
      ) : (
        <div className="table-wrap">
          <table className="dashboard-table">
            <thead>
              <tr>
                <th>Entity</th>
                <th>Stage</th>
                <th>Status</th>
                <th>Attempts</th>
                <th>Next retry</th>
                <th>Last started</th>
                <th>Reason</th>
              </tr>
            </thead>
            <tbody>
              {tasks.map((task) => (
                <tr key={`${task.entity_id}-${task.stage_id}-${task.status}`}>
                  <td>{task.entity_id}</td>
                  <td>{task.stage_id}</td>
                  <td>
                    <StatusBadge status={task.status} />
                  </td>
                  <td>
                    {task.attempts}/{task.max_attempts}
                  </td>
                  <td>{formatDateTime(task.next_retry_at)}</td>
                  <td>{formatDateTime(task.last_started_at)}</td>
                  <td>{task.reason ?? "Not available"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
