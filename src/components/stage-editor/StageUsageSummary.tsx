import { formatDateTime } from "../../lib/formatters";
import type { StageUsageSummary as StageUsageSummaryRecord } from "../../types/domain";
import { StatusBadge } from "../StatusBadge";

interface StageUsageSummaryProps {
  usage: StageUsageSummaryRecord | null;
}

export function StageUsageSummary({ usage }: StageUsageSummaryProps) {
  if (!usage) {
    return <p className="empty-text">No SQLite usage has been recorded for this draft stage.</p>;
  }

  return (
    <div className="stage-usage-box">
      <div className="inline-meta">
        <StatusBadge status={usage.is_active ? "active" : "inactive"} />
        <span>entities {usage.entity_count}</span>
        <span>files {usage.entity_file_count}</span>
        <span>states {usage.stage_state_count}</span>
        <span>runs {usage.run_count}</span>
      </div>
      <div className="inline-meta">
        <span>last seen {formatDateTime(usage.last_seen_in_config_at)}</span>
        {usage.archived_at ? <span>archived {formatDateTime(usage.archived_at)}</span> : null}
        <span>{usage.can_rename ? "rename allowed" : "rename locked"}</span>
      </div>
      {usage.warnings.length > 0 ? (
        <ul className="stage-editor-warning-list">
          {usage.warnings.map((warning) => (
            <li key={warning}>{warning}</li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
