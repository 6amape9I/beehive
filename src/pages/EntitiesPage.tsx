import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { EntitiesTable } from "../components/entities/EntitiesTable";
import { EntityFilters } from "../components/entities/EntityFilters";
import { PaginationControls } from "../components/entities/PaginationControls";
import { listEntities, scanWorkspace } from "../lib/runtimeApi";
import type {
  CommandErrorInfo,
  EntityListQuery,
  EntityListSortBy,
  EntityTableRow,
  EntityValidationStatus,
  SortDirection,
} from "../types/domain";

const defaultSortBy: EntityListSortBy = "updated_at";
const defaultSortDirection: SortDirection = "desc";
const defaultPageSize = 50;

function queryFromSearchParams(params: URLSearchParams): EntityListQuery {
  return {
    search: params.get("search") ?? "",
    stage_id: params.get("stage") ?? "",
    status: params.get("status") ?? "",
    validation_status: (params.get("validation") || null) as EntityValidationStatus | null,
    sort_by: (params.get("sort") || defaultSortBy) as EntityListSortBy,
    sort_direction: (params.get("dir") || defaultSortDirection) as SortDirection,
    page: Math.max(1, Number(params.get("page") ?? "1") || 1),
    page_size: Math.min(200, Math.max(1, Number(params.get("page_size") ?? defaultPageSize))),
  };
}

function writeQueryToParams(query: EntityListQuery) {
  const params = new URLSearchParams();
  if (query.search) params.set("search", query.search);
  if (query.stage_id) params.set("stage", query.stage_id);
  if (query.status) params.set("status", query.status);
  if (query.validation_status) params.set("validation", query.validation_status);
  if (query.sort_by && query.sort_by !== defaultSortBy) params.set("sort", query.sort_by);
  if (query.sort_direction && query.sort_direction !== defaultSortDirection) {
    params.set("dir", query.sort_direction);
  }
  if (query.page && query.page > 1) params.set("page", String(query.page));
  if (query.page_size && query.page_size !== defaultPageSize) {
    params.set("page_size", String(query.page_size));
  }
  return params;
}

export function EntitiesPage() {
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const { state } = useBootstrap();
  const [entities, setEntities] = useState<EntityTableRow[]>([]);
  const [availableStages, setAvailableStages] = useState<string[]>([]);
  const [availableStatuses, setAvailableStatuses] = useState<string[]>([]);
  const [total, setTotal] = useState(0);
  const [errors, setErrors] = useState<CommandErrorInfo[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [isScanning, setIsScanning] = useState(false);
  const [actionMessage, setActionMessage] = useState<string | null>(null);

  const query = useMemo(() => queryFromSearchParams(searchParams), [searchParams]);
  const workdirPath = state.selected_workdir_path;
  const canQueryRuntime = state.phase === "fully_initialized" && !!workdirPath;

  const setQuery = useCallback(
    (patch: Partial<EntityListQuery>) => {
      const next = { ...query, ...patch };
      setSearchParams(writeQueryToParams(next), { replace: true });
    },
    [query, setSearchParams],
  );

  const clearFilters = useCallback(() => {
    setSearchParams(new URLSearchParams(), { replace: true });
  }, [setSearchParams]);

  const loadEntities = useCallback(async () => {
    if (!canQueryRuntime || !workdirPath) {
      setEntities([]);
      setAvailableStages([]);
      setAvailableStatuses([]);
      setTotal(0);
      setErrors([]);
      return;
    }

    setIsLoading(true);
    try {
      const result = await listEntities(workdirPath, query);
      setEntities(result.entities);
      setAvailableStages(result.available_stages);
      setAvailableStatuses(result.available_statuses);
      setTotal(result.total);
      setErrors(result.errors);
    } finally {
      setIsLoading(false);
    }
  }, [canQueryRuntime, query, workdirPath]);

  const hasActiveFilters = Boolean(
    query.search || query.stage_id || query.status || query.validation_status,
  );

  const handleScanWorkspace = useCallback(async () => {
    if (!canQueryRuntime || !workdirPath) {
      return;
    }

    setIsScanning(true);
    setActionMessage(null);
    try {
      const result = await scanWorkspace(workdirPath);
      if (result.summary) {
        setActionMessage(
          `Scan finished: ${result.summary.registered_entity_count} entities, ${result.summary.registered_file_count} files, ${result.summary.invalid_count} invalid.`,
        );
      } else {
        setActionMessage("Scan finished without a summary.");
      }
      setErrors(result.errors);
      await loadEntities();
    } finally {
      setIsScanning(false);
    }
  }, [canQueryRuntime, loadEntities, workdirPath]);

  useEffect(() => {
    void loadEntities();
  }, [loadEntities]);

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Runtime</span>
          <h1>Entities</h1>
        </div>
        <div className="button-row">
          <button
            type="button"
            className="button secondary"
            disabled={!canQueryRuntime || isLoading || isScanning}
            onClick={() => void loadEntities()}
          >
            Refresh
          </button>
          <button
            type="button"
            className="button"
            disabled={!canQueryRuntime || isLoading || isScanning}
            onClick={() => void handleScanWorkspace()}
          >
            {isScanning ? "Scanning..." : "Scan workspace"}
          </button>
        </div>
      </div>

      <EntityFilters
        query={query}
        availableStages={availableStages}
        availableStatuses={availableStatuses}
        disabled={!canQueryRuntime || isLoading || isScanning}
        onChange={setQuery}
        onClear={clearFilters}
      />

      <CommandErrorsPanel title="Entity Query Errors" errors={errors} />
      {actionMessage ? <p className="empty-text">{actionMessage}</p> : null}

      <section className="panel">
        <div className="panel-heading">
          <h2>Registered Entities</h2>
          <span className="muted">
            {isLoading
              ? "Loading..."
              : canQueryRuntime
                ? `${total} matching entity row(s)`
                : "Open a workdir to query runtime data"}
          </span>
        </div>
        {!canQueryRuntime ? (
          <p className="empty-text">Open or initialize a valid workdir to view registered entities.</p>
        ) : entities.length === 0 ? (
          <div className="empty-text">
            <p>
              {hasActiveFilters
                ? "No entities match the current filters."
                : "No entities are registered yet. Run Scan workspace to register JSON files from active stage folders."}
            </p>
            {!hasActiveFilters ? (
              <button
                type="button"
                className="button"
                disabled={isScanning}
                onClick={() => void handleScanWorkspace()}
              >
                {isScanning ? "Scanning..." : "Scan workspace"}
              </button>
            ) : null}
          </div>
        ) : (
          <>
            <EntitiesTable
              rows={entities}
              sortBy={query.sort_by ?? defaultSortBy}
              sortDirection={query.sort_direction ?? defaultSortDirection}
              onSortChange={(sortBy, sortDirection) =>
                setQuery({ sort_by: sortBy, sort_direction: sortDirection, page: 1 })
              }
              onRowClick={(entityId) => void navigate(`/entities/${entityId}`)}
            />
            <PaginationControls
              page={query.page ?? 1}
              pageSize={query.page_size ?? defaultPageSize}
              total={total}
              disabled={isLoading}
              onPageChange={(page) => setQuery({ page })}
              onPageSizeChange={(pageSize) => setQuery({ page_size: pageSize, page: 1 })}
            />
          </>
        )}
      </section>
    </div>
  );
}
