import { formatDateTime } from "../../lib/formatters";
import type { EntityTimelineItem } from "../../types/domain";
import { StatusBadge } from "../StatusBadge";

interface EntityTimelineProps {
  timeline: EntityTimelineItem[];
}

export function EntityTimeline({ timeline }: EntityTimelineProps) {
  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>Stage Timeline</h2>
        <span className="muted">{timeline.length} stage state(s)</span>
      </div>
      {timeline.length === 0 ? (
        <p className="empty-text">No timeline entries are available for this entity.</p>
      ) : (
        <div className="timeline-list">
          {timeline.map((item) => (
            <div key={item.stage_id} className="timeline-row">
              <div>
                <strong>{item.stage_id}</strong>
                <div className="muted">{item.file_path ?? "No file path"}</div>
              </div>
              <StatusBadge status={item.status} />
              <div className="inline-meta">
                <span>
                  Attempts {item.attempts}/{item.max_attempts}
                </span>
                <span>{item.file_exists ? "file present" : "file missing"}</span>
                <span>HTTP {item.last_http_status ?? "n/a"}</span>
                <span>Updated {formatDateTime(item.updated_at)}</span>
              </div>
              {item.last_error ? <p className="error-text">{item.last_error}</p> : null}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

