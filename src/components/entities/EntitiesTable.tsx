import { formatDateTime } from "../../lib/formatters";
import type { EntityListSortBy, EntityTableRow, SortDirection } from "../../types/domain";
import { StatusBadge } from "../StatusBadge";

interface EntitiesTableProps {
  rows: EntityTableRow[];
  selectedEntityIds: string[];
  sortBy: EntityListSortBy;
  sortDirection: SortDirection;
  onSortChange: (sortBy: EntityListSortBy, direction: SortDirection) => void;
  onToggleSelected: (row: EntityTableRow) => void;
  onRowClick: (entityId: string) => void;
  onArchive: (entityId: string) => void;
  onRestore: (entityId: string) => void;
}

const sortableColumns = new Set<EntityListSortBy>([
  "entity_id",
  "current_stage",
  "status",
  "updated_at",
  "last_seen_at",
]);

function sortableHeader(
  label: string,
  sortBy: EntityListSortBy,
  activeSortBy: EntityListSortBy,
  activeDirection: SortDirection,
  onSortChange: (sortBy: EntityListSortBy, direction: SortDirection) => void,
) {
  const active = sortBy === activeSortBy;
  return (
    <button
      type="button"
      className="table-sort-button"
      onClick={() => onSortChange(sortBy, active && activeDirection === "asc" ? "desc" : "asc")}
    >
      {label}
      <span>{active ? (activeDirection === "asc" ? " up" : " down") : ""}</span>
    </button>
  );
}

function shortS3Key(row: EntityTableRow) {
  const value = row.latest_file_path ?? "";
  const key = value.startsWith("s3://") ? value.split("/").slice(3).join("/") : value;
  if (!key) return "Not available";
  if (key.length <= 72) return key;
  return `${key.slice(0, 32)}...${key.slice(-32)}`;
}

export function EntitiesTable({
  rows,
  selectedEntityIds,
  sortBy,
  sortDirection,
  onSortChange,
  onToggleSelected,
  onRowClick,
  onArchive,
  onRestore,
}: EntitiesTableProps) {
  const selectedSet = new Set(selectedEntityIds);

  return (
    <div className="table-wrap">
      <table className="entities-table">
        <thead>
          <tr>
            <th>Select</th>
            <th>
              {sortableColumns.has("entity_id")
                ? sortableHeader("Entity", "entity_id", sortBy, sortDirection, onSortChange)
                : "Entity"}
            </th>
            <th>
              {sortableHeader("Stage", "current_stage", sortBy, sortDirection, onSortChange)}
            </th>
            <th>{sortableHeader("Status", "status", sortBy, sortDirection, onSortChange)}</th>
            <th>S3 key</th>
            <th>{sortableHeader("Updated", "updated_at", sortBy, sortDirection, onSortChange)}</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => {
            const selected = selectedSet.has(row.entity_id);
            return (
              <tr
                key={row.entity_id}
                className={selected ? "selected-row" : undefined}
                onClick={() => onRowClick(row.entity_id)}
              >
                <td onClick={(event) => event.stopPropagation()}>
                  <input
                    type="checkbox"
                    checked={selected}
                    disabled={!row.latest_file_id || row.is_archived}
                    aria-label={`Select ${row.entity_id}`}
                    onChange={() => onToggleSelected(row)}
                  />
                </td>
                <td>
                  <strong>{row.display_name ?? row.entity_id}</strong>
                  {row.display_name ? <span className="muted">{row.entity_id}</span> : null}
                  {row.operator_note ? <span className="muted">{row.operator_note}</span> : null}
                </td>
                <td>{row.current_stage_id ?? "Not available"}</td>
                <td>
                  <StatusBadge status={row.is_archived ? "archived" : row.current_status} />
                </td>
                <td>
                  <code title={row.latest_file_path ?? ""}>{shortS3Key(row)}</code>
                </td>
                <td>{formatDateTime(row.updated_at)}</td>
                <td onClick={(event) => event.stopPropagation()}>
                  {row.is_archived ? (
                    <button
                      type="button"
                      className="button secondary"
                      onClick={() => onRestore(row.entity_id)}
                    >
                      Restore
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="button secondary"
                      onClick={() => onArchive(row.entity_id)}
                    >
                      Archive
                    </button>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
