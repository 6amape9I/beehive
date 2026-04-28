import {
  flexRender,
  getCoreRowModel,
  useReactTable,
  type ColumnDef,
} from "@tanstack/react-table";

import { formatDateTime } from "../../lib/formatters";
import type { EntityListSortBy, EntityTableRow, SortDirection } from "../../types/domain";
import { StatusBadge } from "../StatusBadge";

interface EntitiesTableProps {
  rows: EntityTableRow[];
  sortBy: EntityListSortBy;
  sortDirection: SortDirection;
  onSortChange: (sortBy: EntityListSortBy, direction: SortDirection) => void;
  onRowClick: (entityId: string) => void;
}

const columns: ColumnDef<EntityTableRow>[] = [
  {
    id: "entity_id",
    header: "Entity ID",
    cell: ({ row }) => <strong>{row.original.entity_id}</strong>,
  },
  {
    id: "display_name",
    header: "Name",
    cell: ({ row }) => row.original.display_name ?? "Not available",
  },
  {
    id: "current_stage",
    header: "Current stage",
    cell: ({ row }) => row.original.current_stage_id ?? "Not available",
  },
  {
    id: "status",
    header: "Status",
    cell: ({ row }) => <StatusBadge status={row.original.current_status} />,
  },
  {
    id: "attempts",
    header: "Attempts",
    cell: ({ row }) =>
      row.original.attempts === null
        ? "Not available"
        : `${row.original.attempts}/${row.original.max_attempts ?? "?"}`,
  },
  {
    id: "last_error",
    header: "Last error",
    cell: ({ row }) => (
      <span className="truncate-cell" title={row.original.last_error ?? ""}>
        {row.original.last_error ?? "None"}
      </span>
    ),
  },
  {
    id: "last_http",
    header: "Last HTTP",
    cell: ({ row }) => row.original.last_http_status ?? "Not available",
  },
  {
    id: "next_retry",
    header: "Next retry",
    cell: ({ row }) => formatDateTime(row.original.next_retry_at),
  },
  {
    id: "file_count",
    header: "Files",
    cell: ({ row }) => row.original.file_count,
  },
  {
    id: "latest_file_path",
    header: "Latest file path",
    cell: ({ row }) => <code>{row.original.latest_file_path ?? "Not available"}</code>,
  },
  {
    id: "validation",
    header: "Validation",
    cell: ({ row }) => <StatusBadge status={row.original.validation_status} />,
  },
  {
    id: "updated_at",
    header: "Updated at",
    cell: ({ row }) => formatDateTime(row.original.updated_at),
  },
  {
    id: "last_seen_at",
    header: "Last seen",
    cell: ({ row }) => formatDateTime(row.original.last_seen_at),
  },
];

const sortableColumns = new Set<EntityListSortBy>([
  "entity_id",
  "current_stage",
  "status",
  "attempts",
  "last_error",
  "updated_at",
  "last_seen_at",
]);

function sortId(columnId: string): EntityListSortBy | null {
  if (sortableColumns.has(columnId as EntityListSortBy)) {
    return columnId as EntityListSortBy;
  }
  return null;
}

export function EntitiesTable({
  rows,
  sortBy,
  sortDirection,
  onSortChange,
  onRowClick,
}: EntitiesTableProps) {
  const table = useReactTable({
    data: rows,
    columns,
    getCoreRowModel: getCoreRowModel(),
    manualSorting: true,
  });

  return (
    <div className="table-wrap">
      <table className="entities-table">
        <thead>
          {table.getHeaderGroups().map((headerGroup) => (
            <tr key={headerGroup.id}>
              {headerGroup.headers.map((header) => {
                const mappedSort = sortId(header.column.id);
                const active = mappedSort === sortBy;
                return (
                  <th key={header.id}>
                    {mappedSort ? (
                      <button
                        type="button"
                        className="table-sort-button"
                        onClick={() =>
                          onSortChange(
                            mappedSort,
                            active && sortDirection === "asc" ? "desc" : "asc",
                          )
                        }
                      >
                        {flexRender(header.column.columnDef.header, header.getContext())}
                        <span>{active ? (sortDirection === "asc" ? " up" : " down") : ""}</span>
                      </button>
                    ) : (
                      flexRender(header.column.columnDef.header, header.getContext())
                    )}
                  </th>
                );
              })}
            </tr>
          ))}
        </thead>
        <tbody>
          {table.getRowModel().rows.map((row) => (
            <tr
              key={row.original.entity_id}
              className="clickable-row"
              onClick={() => onRowClick(row.original.entity_id)}
            >
              {row.getVisibleCells().map((cell) => (
                <td key={cell.id}>{flexRender(cell.column.columnDef.cell, cell.getContext())}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
