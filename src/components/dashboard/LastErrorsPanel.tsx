import { formatDateTime } from "../../lib/formatters";
import type { DashboardErrorItem } from "../../types/domain";
import { StatusBadge } from "../StatusBadge";

export function LastErrorsPanel({ errors }: { errors: DashboardErrorItem[] }) {
  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>Last Errors</h2>
        <span className="muted">{errors.length} shown</span>
      </div>
      {errors.length === 0 ? (
        <p className="empty-text">No recent errors.</p>
      ) : (
        <div className="issue-list">
          {errors.map((error) => (
            <article className="issue-row" key={error.id}>
              <StatusBadge status={error.level} />
              <div>
                <strong>{error.event_type}</strong>
                <p>{error.message}</p>
                <div className="inline-meta">
                  {error.entity_id ? <span>entity: {error.entity_id}</span> : null}
                  {error.stage_id ? <span>stage: {error.stage_id}</span> : null}
                  {error.run_id ? <span>run: {error.run_id}</span> : null}
                  <span>{formatDateTime(error.created_at)}</span>
                </div>
              </div>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}
