import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { StatusBadge } from "../components/StatusBadge";
import { PaginationControls } from "../components/entities/PaginationControls";
import {
  listEntities,
  listWorkspaceEntities,
  reconcileS3Workspace,
  reconcileS3WorkspaceById,
  runSelectedPipelineWavesById,
  scanWorkspace,
} from "../lib/runtimeApi";
import type {
  CommandErrorInfo,
  EntityListQuery,
  EntityTableRow,
  EntityValidationStatus,
  RunSelectedPipelineWavesSummary,
  SortDirection,
} from "../types/domain";

const DEFAULT_PAGE_SIZE = 50;
const DEFAULT_MAX_WAVES = 5;
const DEFAULT_TASKS_PER_WAVE = 3;

type EntitySortBy = NonNullable<EntityListQuery["sort_by"]>;

interface ExplorerFilters {
  search: string;
  stageId: string;
  runtimeStatus: string;
  validationStatus: "" | EntityValidationStatus;
  includeArchived: boolean;
}

interface FastExplorerQuery extends ExplorerFilters {
  page: number;
  pageSize: number;
  sortBy: EntitySortBy;
  sortDirection: SortDirection;
}

const defaultQuery: FastExplorerQuery = {
  search: "",
  stageId: "",
  runtimeStatus: "",
  validationStatus: "",
  includeArchived: false,
  page: 1,
  pageSize: DEFAULT_PAGE_SIZE,
  sortBy: "updated_at",
  sortDirection: "desc",
};

function queryFromSearchParams(params: URLSearchParams): FastExplorerQuery {
  return {
    search: params.get("search") ?? defaultQuery.search,
    stageId: params.get("stage") ?? defaultQuery.stageId,
    runtimeStatus: params.get("status") ?? defaultQuery.runtimeStatus,
    validationStatus: (params.get("validation") || "") as FastExplorerQuery["validationStatus"],
    includeArchived: params.get("archived") === "true",
    page: Math.max(1, Number(params.get("page") ?? defaultQuery.page) || defaultQuery.page),
    pageSize: Math.min(
      200,
      Math.max(1, Number(params.get("page_size") ?? defaultQuery.pageSize) || defaultQuery.pageSize),
    ),
    sortBy: (params.get("sort") || defaultQuery.sortBy) as EntitySortBy,
    sortDirection: (params.get("dir") || defaultQuery.sortDirection) as SortDirection,
  };
}

function queryToSearchParams(query: FastExplorerQuery): URLSearchParams {
  const params = new URLSearchParams();
  if (query.search) params.set("search", query.search);
  if (query.stageId) params.set("stage", query.stageId);
  if (query.runtimeStatus) params.set("status", query.runtimeStatus);
  if (query.validationStatus) params.set("validation", query.validationStatus);
  if (query.includeArchived) params.set("archived", "true");
  if (query.page > 1) params.set("page", String(query.page));
  if (query.pageSize !== DEFAULT_PAGE_SIZE) params.set("page_size", String(query.pageSize));
  if (query.sortBy !== defaultQuery.sortBy) params.set("sort", query.sortBy);
  if (query.sortDirection !== defaultQuery.sortDirection) params.set("dir", query.sortDirection);
  return params;
}

function toEntityListQuery(query: FastExplorerQuery): EntityListQuery {
  return {
    search: query.search || null,
    stage_id: query.stageId || null,
    status: query.runtimeStatus || null,
    validation_status: query.validationStatus || null,
    include_archived: query.includeArchived,
    page: query.page,
    page_size: query.pageSize,
    sort_by: query.sortBy,
    sort_direction: query.sortDirection,
  };
}

export function WorkspaceExplorerPage() {
  const navigate = useNavigate();
  const { state } = useBootstrap();
  const { workspaceId: routeWorkspaceId } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();

  const query = useMemo(() => queryFromSearchParams(searchParams), [searchParams]);
  const workdirPath = state.selected_workdir_path;
  const workspaceId = routeWorkspaceId ?? state.selected_workspace_id;
  const canQueryRuntime = Boolean(workspaceId || (state.phase === "fully_initialized" && workdirPath));
  const canUseWorkspaceActions = Boolean(workspaceId);

  const [entities, setEntities] = useState<EntityTableRow[]>([]);
  const [availableStages, setAvailableStages] = useState<string[]>([]);
  const [availableStatuses, setAvailableStatuses] = useState<string[]>([]);
  const [total, setTotal] = useState(0);
  const [errors, setErrors] = useState<CommandErrorInfo[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [activeAction, setActiveAction] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [selectedEntityIds, setSelectedEntityIds] = useState<string[]>([]);
  const [selectedPipelineSummary, setSelectedPipelineSummary] =
    useState<RunSelectedPipelineWavesSummary | null>(null);

  const selectedRows = useMemo(
    () => entities.filter((entity) => selectedEntityIds.includes(entity.entity_id)),
    [entities, selectedEntityIds],
  );
  const selectedFileIds = selectedRows
    .map((entity) => entity.latest_file_id)
    .filter((id): id is number => typeof id === "number");

  const setQuery = useCallback(
    (patch: Partial<FastExplorerQuery>) => {
      const next = { ...query, ...patch };
      setSearchParams(queryToSearchParams(next), { replace: true });
    },
    [query, setSearchParams],
  );

  const resetFilters = useCallback(() => {
    setSearchParams(new URLSearchParams(), { replace: true });
  }, [setSearchParams]);

  const loadEntities = useCallback(async () => {
    if (!canQueryRuntime) {
      setEntities([]);
      setAvailableStages([]);
      setAvailableStatuses([]);
      setTotal(0);
      setErrors([]);
      return;
    }

    setIsLoading(true);
    try {
      const result = workspaceId
        ? await listWorkspaceEntities(workspaceId, toEntityListQuery(query))
        : await listEntities(workdirPath ?? "", toEntityListQuery(query));
      setEntities(result.entities);
      setAvailableStages(result.available_stages);
      setAvailableStatuses(result.available_statuses);
      setTotal(result.total);
      setErrors(result.errors);
      setSelectedEntityIds((current) =>
        current.filter((entityId) => result.entities.some((entity) => entity.entity_id === entityId)),
      );
    } finally {
      setIsLoading(false);
    }
  }, [canQueryRuntime, query, workdirPath, workspaceId]);

  useEffect(() => {
    void loadEntities();
  }, [loadEntities]);

  async function handleReconcileS3() {
    if (!workdirPath && !workspaceId) return;
    setActiveAction("s3-reconcile");
    setActionMessage(null);
    try {
      const result = workspaceId
        ? await reconcileS3WorkspaceById(workspaceId)
        : await reconcileS3Workspace(workdirPath as string);
      setErrors(result.errors);
      setActionMessage(
        result.summary
          ? `S3 reconciliation complete: ${result.summary.registered_file_count} registered, ${result.summary.updated_file_count} updated, ${result.summary.unmapped_object_count} unmapped.`
          : "S3 reconciliation finished.",
      );
      await loadEntities();
    } finally {
      setActiveAction(null);
    }
  }

  async function handleScanWorkspace() {
    if (!workdirPath) return;
    setActiveAction("scan");
    setActionMessage(null);
    try {
      const result = await scanWorkspace(workdirPath);
      setErrors(result.errors);
      setActionMessage(
        result.summary
          ? `Scan complete: ${result.summary.registered_file_count} registered, ${result.summary.invalid_count} invalid.`
          : "Scan finished.",
      );
      await loadEntities();
    } finally {
      setActiveAction(null);
    }
  }

  async function handleRunSelectedPipelineWaves() {
    if (!workspaceId) {
      setActionMessage("Selected pipeline waves require a registered workspace route.");
      return;
    }
    if (selectedFileIds.length === 0) {
      setActionMessage("Select at least one visible pending S3 source artifact.");
      return;
    }

    setActiveAction("selected-pipeline-waves");
    setActionMessage(null);
    setSelectedPipelineSummary(null);
    try {
      const result = await runSelectedPipelineWavesById(
        workspaceId,
        selectedFileIds,
        DEFAULT_MAX_WAVES,
        DEFAULT_TASKS_PER_WAVE,
        true,
      );
      setErrors([...(result.errors ?? []), ...(result.summary?.errors ?? [])]);
      setSelectedPipelineSummary(result.summary);
      setActionMessage(
        result.summary
          ? `Selected pipeline waves complete: ${result.summary.waves_executed} wave(s), ${result.summary.total_claimed} claimed, ${result.summary.output_tree.length} output(s), stopped ${result.summary.stopped_reason}.`
          : "Selected pipeline waves finished.",
      );
      setSelectedEntityIds([]);
      await loadEntities();
    } finally {
      setActiveAction(null);
    }
  }

  function toggleEntity(entity: EntityTableRow, checked: boolean) {
    if (!entity.latest_file_id || !isSelectableEntity(entity)) return;
    setSelectedEntityIds((current) => {
      if (checked) {
        if (current.includes(entity.entity_id) || current.length >= 10) return current;
        return [...current, entity.entity_id];
      }
      return current.filter((entityId) => entityId !== entity.entity_id);
    });
  }

  function goToEntity(entity: EntityTableRow) {
    const path = workspaceId
      ? `/workspaces/${encodeURIComponent(workspaceId)}/entities/${encodeURIComponent(entity.entity_id)}`
      : `/entities/${encodeURIComponent(entity.entity_id)}`;
    navigate(entity.latest_file_id ? `${path}?file_id=${entity.latest_file_id}` : path);
  }

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Fast Workspace Explorer</span>
          <h1>{workspaceId ? `Workspace ${workspaceId}` : "Workspace Explorer"}</h1>
          <span className="muted">
            Paginated entity view. Only {query.pageSize} rows are requested from backend per page.
          </span>
        </div>
        <div className="button-row">
          <button
            type="button"
            className="button primary"
            disabled={!workspaceId}
            onClick={() =>
              workspaceId
                ? navigate(`/workspaces/${encodeURIComponent(workspaceId)}/entities`)
                : navigate("/entities")
            }
          >
            Upload entities
          </button>
          <button
            type="button"
            className="button secondary"
            disabled={!canQueryRuntime || isLoading}
            onClick={() => void loadEntities()}
          >
            {isLoading ? "Refreshing..." : "Refresh"}
          </button>
          <button
            type="button"
            className="button secondary"
            disabled={!canQueryRuntime || activeAction === "s3-reconcile"}
            onClick={() => void handleReconcileS3()}
          >
            {activeAction === "s3-reconcile" ? "Reconciling..." : "Reconcile S3"}
          </button>
          <button
            type="button"
            className="button primary"
            disabled={!workdirPath || !canQueryRuntime || activeAction === "scan"}
            onClick={() => void handleScanWorkspace()}
          >
            {activeAction === "scan" ? "Scanning..." : "Scan workspace"}
          </button>
        </div>
      </div>

      <CommandErrorsPanel title="Workspace Explorer Errors" errors={errors} />
      {actionMessage ? <section className="compact-panel panel">{actionMessage}</section> : null}

      {!canQueryRuntime ? (
        <section className="panel">
          <p className="empty-text">Open or select a workspace to inspect entities.</p>
        </section>
      ) : (
        <>
          <section className="panel">
            <div className="panel-heading">
              <div>
                <h2>Operator Actions</h2>
                <span className="muted">
                  {selectedFileIds.length} selected source(s) / {total} matching entities
                </span>
              </div>
              <div className="button-row">
                <button
                  type="button"
                  className="button primary"
                  disabled={!canUseWorkspaceActions || selectedFileIds.length === 0 || activeAction === "selected-pipeline-waves"}
                  onClick={() => void handleRunSelectedPipelineWaves()}
                >
                  {activeAction === "selected-pipeline-waves"
                    ? "Running selected..."
                    : `Run selected pipeline waves (${selectedFileIds.length})`}
                </button>
                <button
                  type="button"
                  className="button secondary"
                  disabled={selectedEntityIds.length === 0}
                  onClick={() => setSelectedEntityIds([])}
                >
                  Clear selection
                </button>
              </div>
            </div>
            {selectedPipelineSummary ? <SelectedPipelineSummary summary={selectedPipelineSummary} /> : null}
          </section>

          <section className="panel">
            <div className="panel-heading">
              <h2>Filters</h2>
              <button type="button" className="button secondary" onClick={resetFilters}>
                Clear
              </button>
            </div>
            <div className="filter-grid">
              <label>
                Search
                <input
                  value={query.search}
                  placeholder="Entity or path"
                  onChange={(event) => setQuery({ search: event.target.value, page: 1 })}
                />
              </label>
              <label>
                Stage
                <select value={query.stageId} onChange={(event) => setQuery({ stageId: event.target.value, page: 1 })}>
                  <option value="">All stages</option>
                  {availableStages.map((stageId) => (
                    <option key={stageId} value={stageId}>
                      {stageId}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Runtime status
                <select
                  value={query.runtimeStatus}
                  onChange={(event) => setQuery({ runtimeStatus: event.target.value, page: 1 })}
                >
                  <option value="">All statuses</option>
                  {availableStatuses.map((status) => (
                    <option key={status} value={status}>
                      {status}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Validation
                <select
                  value={query.validationStatus}
                  onChange={(event) =>
                    setQuery({
                      validationStatus: event.target.value as FastExplorerQuery["validationStatus"],
                      page: 1,
                    })
                  }
                >
                  <option value="">All validation</option>
                  <option value="valid">valid</option>
                  <option value="warning">warning</option>
                  <option value="invalid">invalid</option>
                </select>
              </label>
            </div>
            <div className="workspace-toggle-row">
              <label>
                <input
                  type="checkbox"
                  checked={query.includeArchived}
                  onChange={(event) => setQuery({ includeArchived: event.target.checked, page: 1 })}
                />
                Show archived
              </label>
            </div>
          </section>

          <section className="panel">
            <div className="panel-heading">
              <div>
                <h2>Entities</h2>
                <span className="muted">
                  Showing {entities.length} row(s); backend total {total}.
                </span>
              </div>
              <PaginationControls
                page={query.page}
                pageSize={query.pageSize}
                total={total}
                disabled={isLoading}
                onPageChange={(page) => setQuery({ page })}
                onPageSizeChange={(pageSize) => setQuery({ pageSize, page: 1 })}
              />
            </div>

            {isLoading ? (
              <p className="empty-text">Loading entities...</p>
            ) : entities.length === 0 ? (
              <p className="empty-text">No entities match the current filters.</p>
            ) : (
              <div className="table-wrap">
                <table className="workspace-file-table">
                  <thead>
                    <tr>
                      <th>Select</th>
                      <th>Entity</th>
                      <th>Stage</th>
                      <th>Status</th>
                      <th>Validation</th>
                      <th>Latest file</th>
                      <th>Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {entities.map((entity) => {
                      const selectable = isSelectableEntity(entity);
                      const selected = selectedEntityIds.includes(entity.entity_id);
                      return (
                        <tr key={entity.entity_id} className={selected ? "selected-row" : ""}>
                          <td>
                            <input
                              type="checkbox"
                              checked={selected}
                              disabled={!selectable || (selectedEntityIds.length >= 10 && !selected)}
                              onChange={(event) => toggleEntity(entity, event.target.checked)}
                              aria-label={`Select entity ${entity.entity_id}`}
                            />
                          </td>
                          <td>
                            <div className="stacked-cell">
                              <strong>{entity.display_name || entity.entity_id}</strong>
                              {entity.display_name ? <span className="muted">{entity.entity_id}</span> : null}
                              {entity.is_archived ? <span className="muted">archived</span> : null}
                            </div>
                          </td>
                          <td>{entity.current_stage_id ?? "not available"}</td>
                          <td>
                            <StatusBadge status={entity.current_status || "unknown"} />
                          </td>
                          <td>
                            <StatusBadge status={entity.validation_status} />
                          </td>
                          <td>
                            <code>{entity.latest_file_path ?? "not available"}</code>
                          </td>
                          <td>
                            <div className="button-row">
                              <button type="button" className="button secondary" onClick={() => goToEntity(entity)}>
                                Entity
                              </button>
                            </div>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}

            <PaginationControls
              page={query.page}
              pageSize={query.pageSize}
              total={total}
              disabled={isLoading}
              onPageChange={(page) => setQuery({ page })}
              onPageSizeChange={(pageSize) => setQuery({ pageSize, page: 1 })}
            />
          </section>
        </>
      )}
    </div>
  );
}

function isSelectableEntity(entity: EntityTableRow): boolean {
  return (
    !entity.is_archived &&
    typeof entity.latest_file_id === "number" &&
    (entity.current_status === "pending" || entity.current_status === "retry_wait")
  );
}

function SelectedPipelineSummary({ summary }: { summary: RunSelectedPipelineWavesSummary }) {
  return (
    <div className="workspace-wave-summary">
      <div className="summary-card-grid">
        <SummaryCard label="Selected roots" value={summary.root_entity_file_ids.length} />
        <SummaryCard label="Waves" value={summary.waves_executed} />
        <SummaryCard label="Claimed" value={summary.total_claimed} />
        <SummaryCard label="Succeeded" value={summary.total_succeeded} />
        <SummaryCard label="Retry" value={summary.total_retry_scheduled} />
        <SummaryCard label="Failed" value={summary.total_failed} />
        <SummaryCard label="Blocked" value={summary.total_blocked} />
        <SummaryCard label="Outputs" value={summary.output_tree.length} />
        <SummaryCard label="Stopped" value={String(summary.stopped_reason)} />
      </div>
      {summary.output_tree.length > 0 ? (
        <details className="diagnostics-block" open>
          <summary>
            <strong>Outputs</strong>
            <span className="muted">Child artifacts created by selected run</span>
          </summary>
          <div className="table-wrap">
            <table className="workspace-file-table">
              <thead>
                <tr>
                  <th>Output</th>
                  <th>Target</th>
                  <th>Status</th>
                  <th>Relation</th>
                  <th>S3</th>
                </tr>
              </thead>
              <tbody>
                {summary.output_tree.map((output) => (
                  <tr key={`${output.producer_run_id}-${output.entity_file_id}`}>
                    <td>{output.entity_id}</td>
                    <td>{output.target_stage_id}</td>
                    <td>
                      <StatusBadge status={output.runtime_status ?? "pending"} />
                    </td>
                    <td>{output.relation_to_source ?? "not available"}</td>
                    <td>
                      <code>{output.s3_uri ?? output.key ?? "not available"}</code>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </details>
      ) : null}
    </div>
  );
}

function SummaryCard({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="summary-card">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}
