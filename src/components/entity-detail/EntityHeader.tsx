import { formatDateTime } from "../../lib/formatters";
import type { EntityRecord } from "../../types/domain";
import { InfoGrid } from "../InfoGrid";
import { StatusBadge } from "../StatusBadge";

interface EntityHeaderProps {
  entity: EntityRecord;
  onRefresh: () => void;
  isRefreshing: boolean;
}

export function EntityHeader({ entity, onRefresh, isRefreshing }: EntityHeaderProps) {
  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h2>{entity.entity_id}</h2>
          <div className="button-row">
            <StatusBadge status={entity.current_status} />
            <StatusBadge status={entity.validation_status} />
          </div>
        </div>
        <button type="button" className="button secondary" disabled={isRefreshing} onClick={onRefresh}>
          Refresh
        </button>
      </div>
      <InfoGrid
        items={[
          { label: "Current stage", value: entity.current_stage_id },
          { label: "Latest file path", value: entity.latest_file_path },
          { label: "Latest file id", value: entity.latest_file_id },
          { label: "File count", value: entity.file_count },
          { label: "First seen", value: formatDateTime(entity.first_seen_at) },
          { label: "Last seen", value: formatDateTime(entity.last_seen_at) },
          { label: "Updated at", value: formatDateTime(entity.updated_at) },
        ]}
      />
    </section>
  );
}

