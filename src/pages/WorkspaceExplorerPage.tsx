import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { StatusBadge } from "../components/StatusBadge";
import { PaginationControls } from "../components/entities/PaginationControls";
import {
  listEntities,
  listWorkspaceEntities,
  getWorkerSummary,
  resetWorkspaceFailedBlockedEntityStagesToPending,
  pauseWorkerPool,
  pauseWorkers,
  releaseWorkerLease,
  recoverExpiredWorkerLeases,
  repairWorkers,
  reconcileStuckWorkerStates,
  reconcileS3Workspace,
  reconcileS3WorkspaceById,
  resumeWorkerPool,
  resumeWorkers,
  runSelectedPipelineWavesById,
  scanWorkspace,
  startWorkers,
  stopWorkers,
  updateWorkerPool,
} from "../lib/runtimeApi";
import type {
  CommandErrorInfo,
  EntityListQuery,
  EntityTableRow,
  EntityValidationStatus,
  RunSelectedPipelineWavesSummary,
  SortDirection,
  WorkerLeaseRecord,
  WorkerPoolRuntimeSummary,
  WorkerSummary,
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
  const [workerSummary, setWorkerSummary] = useState<WorkerSummary | null>(null);
  const [workerDesiredCounts, setWorkerDesiredCounts] = useState<Record<string, number>>({
    default: 1,
    local_llm: 1,
  });

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

  async function handleBulkResetFailedBlocked() {
    if (!workspaceId) return;
    const confirmed = window.confirm(
      "Reset all failed and blocked entity stages in this workspace to pending? Active worker leases will be skipped.",
    );
    if (!confirmed) return;

    setActiveAction("bulk-reset-failed-blocked");
    setActionMessage(null);
    try {
      const result = await resetWorkspaceFailedBlockedEntityStagesToPending(workspaceId, {
        confirm: true,
        reason: "bulk reset failed/blocked from Workspace Explorer",
      });
      setErrors(result.errors);
      if (result.payload) {
        setActionMessage(
          `Bulk reset complete: ${result.payload.reset_count} reset to pending, ${result.payload.skipped_active_lease_count} skipped because of active leases. Failed before: ${result.payload.failed_before}, blocked before: ${result.payload.blocked_before}.`,
        );
        await loadEntities();
      }
    } finally {
      setActiveAction(null);
    }
  }

  async function handleLoadWorkerSummary() {
    if (!workspaceId) return;
    setActiveAction("worker-summary");
    setActionMessage(null);
    try {
      const result = await getWorkerSummary(workspaceId);
      setErrors(result.errors);
      setWorkerSummary(result.summary);
      syncWorkerDesiredCounts(result.summary);
      setActionMessage(
        result.summary
          ? `Worker summary loaded: ${result.summary.active_leases_total} active lease(s), ${result.summary.expired_leases_total} expired.`
          : "Worker summary unavailable.",
      );
    } finally {
      setActiveAction(null);
    }
  }

  async function handleRecoverExpiredLeases() {
    if (!workspaceId) return;
    setActiveAction("worker-recovery");
    setActionMessage(null);
    try {
      const result = await recoverExpiredWorkerLeases(workspaceId);
      setErrors(result.errors);
      setActionMessage(`Worker lease recovery complete: ${result.recovered} recovered.`);
      await handleLoadWorkerSummary();
      await loadEntities();
    } finally {
      setActiveAction(null);
    }
  }

  async function handleReconcileStuckWorkerStates() {
    if (!workspaceId) return;
    setActiveAction("worker-reconcile-stuck");
    setActionMessage(null);
    try {
      const result = await reconcileStuckWorkerStates(workspaceId);
      setErrors(result.errors);
      if (result.summary) {
        setWorkerSummary(result.summary);
        syncWorkerDesiredCounts(result.summary);
      }
      setActionMessage(`Worker stuck-state reconciliation complete: ${result.reconciled} repaired.`);
      await loadEntities();
    } finally {
      setActiveAction(null);
    }
  }

  async function handleRepairWorkers() {
    if (!workspaceId) return;
    setActiveAction("worker-repair");
    setActionMessage(null);
    try {
      const result = await repairWorkers(workspaceId);
      setErrors(result.errors);
      if (result.summary) {
        setWorkerSummary(result.summary);
        syncWorkerDesiredCounts(result.summary);
      }
      setActionMessage(
        result.summary
          ? `Worker repair complete: ${result.reconciled} lease/state repair(s). ${workerRuntimeSummaryText(result.summary)}.`
          : `Worker repair complete: ${result.reconciled} lease/state repair(s).`,
      );
      await loadEntities();
    } finally {
      setActiveAction(null);
    }
  }

  async function handleWorkerControl(
    action: string,
    operation: () => Promise<{ summary: WorkerSummary | null; errors: CommandErrorInfo[] }>,
    message: (summary: WorkerSummary | null) => string,
  ) {
    if (!workspaceId) return;
    setActiveAction(action);
    setActionMessage(null);
    try {
      const result = await operation();
      setErrors(result.errors);
      if (result.summary) {
        setWorkerSummary(result.summary);
        syncWorkerDesiredCounts(result.summary);
      }
      setActionMessage(message(result.summary));
      await loadEntities();
    } finally {
      setActiveAction(null);
    }
  }

  async function handleStartWorkers() {
    await handleWorkerControl(
      "worker-start",
      () =>
        startWorkers(
          workspaceId as string,
          workerDesiredCounts.default ?? 0,
          workerDesiredCounts.local_llm ?? 0,
        ),
      (summary) => `Workers started: ${workerRuntimeSummaryText(summary)}.`,
    );
  }

  async function handleStopWorkers() {
    await handleWorkerControl(
      "worker-stop",
      () => stopWorkers(workspaceId as string),
      (summary) => `Workers stopping: ${workerRuntimeSummaryText(summary)}.`,
    );
  }

  async function handleUpdatePoolDesired(resourceClass: string) {
    const desired = workerDesiredCounts[resourceClass] ?? 0;
    await handleWorkerControl(
      `worker-desired-${resourceClass}`,
      () => updateWorkerPool(workspaceId as string, resourceClass, desired),
      () => `${workerPoolLabel(resourceClass)} desired workers set to ${desired}.`,
    );
  }

  async function handlePauseAllWorkers() {
    await handleWorkerControl(
      "worker-pause-all",
      () => pauseWorkers(workspaceId as string, "manual maintenance"),
      () => "All worker pools paused. Running leases will continue.",
    );
  }

  async function handleResumeAllWorkers() {
    await handleWorkerControl(
      "worker-resume-all",
      () => resumeWorkers(workspaceId as string),
      () => "All worker pools resumed.",
    );
  }

  async function handlePausePool(resourceClass: string) {
    await handleWorkerControl(
      `worker-pause-${resourceClass}`,
      () => pauseWorkerPool(workspaceId as string, resourceClass, "manual maintenance"),
      () => `${workerPoolLabel(resourceClass)} paused. Running leases will continue.`,
    );
  }

  async function handleResumePool(resourceClass: string) {
    await handleWorkerControl(
      `worker-resume-${resourceClass}`,
      () => resumeWorkerPool(workspaceId as string, resourceClass),
      () => `${workerPoolLabel(resourceClass)} resumed.`,
    );
  }

  async function handleReleaseWorkerLease(lease: WorkerLeaseRecord) {
    if (!workspaceId) return;
    setActiveAction(`worker-release-${lease.lease_id}`);
    setActionMessage(null);
    try {
      const result = await releaseWorkerLease(
        workspaceId,
        lease.lease_id,
        "manual_release_after_finished_run",
      );
      setErrors(result.errors);
      setActionMessage(
        result.released
          ? `Worker lease ${shortId(lease.lease_id)} released.`
          : `Worker lease ${shortId(lease.lease_id)} was not released.`,
      );
      await handleLoadWorkerSummary();
      await loadEntities();
    } finally {
      setActiveAction(null);
    }
  }

  function handleWorkerDesiredCountChange(resourceClass: string, value: number) {
    setWorkerDesiredCounts((current) => ({
      ...current,
      [resourceClass]: boundedInteger(value, 0, 999),
    }));
  }

  function syncWorkerDesiredCounts(summary: WorkerSummary | null) {
    if (!summary) return;
    setWorkerDesiredCounts((current) => {
      const next = { ...current };
      for (const pool of summary.pools) {
        next[pool.resource_class] = pool.desired_concurrency;
      }
      return next;
    });
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

  function goToEntityById(entityId: string) {
    const path = workspaceId
      ? `/workspaces/${encodeURIComponent(workspaceId)}/entities/${encodeURIComponent(entityId)}`
      : `/entities/${encodeURIComponent(entityId)}`;
    navigate(path);
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
                  disabled={!canUseWorkspaceActions || activeAction !== null}
                  onClick={() => void handleBulkResetFailedBlocked()}
                >
                  {activeAction === "bulk-reset-failed-blocked" ? "Resetting..." : "Reset failed/blocked to pending"}
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
            {workerSummary?.broad_runs_disabled ? (
              <p className="empty-text">
                Broad manual runs are disabled while workers are enabled. Use selected run for debug or let workers process the queue.
              </p>
            ) : null}
            {workerSummary?.workers_enabled ? (
              <p className="empty-text">
                Selected runs are for operator debug only while workers are enabled. Roots with active worker leases will be rejected.
              </p>
            ) : null}
            {selectedPipelineSummary ? <SelectedPipelineSummary summary={selectedPipelineSummary} /> : null}
          </section>

          {workspaceId ? (
            <section className="panel">
              <div className="panel-heading">
                <div>
                  <h2>Workers & Queue</h2>
                  <span className="muted">
                    DB-backed pool controls, queue counts, and recent leases.
                  </span>
                </div>
                <div className="worker-runtime-controls">
                  <label className="inline-field" htmlFor="default-worker-count">
                    Default workers
                    <input
                      id="default-worker-count"
                      type="number"
                      min={0}
                      max={999}
                      value={workerDesiredCounts.default ?? 0}
                      disabled={activeAction !== null}
                      onChange={(event) =>
                        handleWorkerDesiredCountChange("default", Number(event.target.value))
                      }
                    />
                  </label>
                  <label className="inline-field" htmlFor="local-llm-worker-count">
                    Local LLM workers
                    <input
                      id="local-llm-worker-count"
                      type="number"
                      min={0}
                      max={999}
                      value={workerDesiredCounts.local_llm ?? 0}
                      disabled={activeAction !== null}
                      onChange={(event) =>
                        handleWorkerDesiredCountChange("local_llm", Number(event.target.value))
                      }
                    />
                  </label>
                  <button
                    type="button"
                    className="button primary"
                    disabled={activeAction !== null}
                    onClick={() => void handleStartWorkers()}
                  >
                    {activeAction === "worker-start" ? "Starting..." : "Start workers"}
                  </button>
                  <button
                    type="button"
                    className="button secondary"
                    disabled={activeAction !== null}
                    onClick={() => void handleStopWorkers()}
                  >
                    {activeAction === "worker-stop" ? "Stopping..." : "Stop workers"}
                  </button>
                  <button
                    type="button"
                    className="button secondary"
                    disabled={activeAction === "worker-summary"}
                    onClick={() => void handleLoadWorkerSummary()}
                  >
                    {activeAction === "worker-summary" ? "Loading..." : "Load summary"}
                  </button>
                </div>
                <div className="button-row">
                  <button
                    type="button"
                    className="button primary"
                    disabled={activeAction !== null}
                    onClick={() => void handleRepairWorkers()}
                  >
                    {activeAction === "worker-repair" ? "Repairing..." : "Repair workers"}
                  </button>
                  <button
                    type="button"
                    className="button secondary"
                    disabled={activeAction === "worker-recovery"}
                    onClick={() => void handleRecoverExpiredLeases()}
                  >
                    {activeAction === "worker-recovery" ? "Recovering..." : "Recover expired"}
                  </button>
                  <button
                    type="button"
                    className="button secondary"
                    disabled={activeAction === "worker-reconcile-stuck"}
                    onClick={() => void handleReconcileStuckWorkerStates()}
                  >
                    {activeAction === "worker-reconcile-stuck" ? "Reconciling..." : "Reconcile stuck"}
                  </button>
                  <button
                    type="button"
                    className="button secondary"
                    disabled={activeAction !== null}
                    onClick={() => void handlePauseAllWorkers()}
                  >
                    Pause all
                  </button>
                  <button
                    type="button"
                    className="button secondary"
                    disabled={activeAction !== null}
                    onClick={() => void handleResumeAllWorkers()}
                  >
                    Resume all
                  </button>
                </div>
              </div>
              {workerSummary ? (
                <WorkerPoolsSummary
                  summary={workerSummary}
                  activeAction={activeAction}
                  desiredCounts={workerDesiredCounts}
                  onDesiredCountChange={handleWorkerDesiredCountChange}
                  onApplyDesiredPool={handleUpdatePoolDesired}
                  onPausePool={handlePausePool}
                  onResumePool={handleResumePool}
                  onReleaseLease={handleReleaseWorkerLease}
                  onReconcileStuck={handleRepairWorkers}
                  onOpenEntity={goToEntityById}
                />
              ) : null}
            </section>
          ) : null}

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

function WorkerPoolsSummary({
  summary,
  activeAction,
  desiredCounts,
  onDesiredCountChange,
  onApplyDesiredPool,
  onPausePool,
  onResumePool,
  onReleaseLease,
  onReconcileStuck,
  onOpenEntity,
}: {
  summary: WorkerSummary;
  activeAction: string | null;
  desiredCounts: Record<string, number>;
  onDesiredCountChange: (resourceClass: string, value: number) => void;
  onApplyDesiredPool: (resourceClass: string) => void;
  onPausePool: (resourceClass: string) => void;
  onResumePool: (resourceClass: string) => void;
  onReleaseLease: (lease: WorkerLeaseRecord) => void;
  onReconcileStuck: () => void;
  onOpenEntity: (entityId: string) => void;
}) {
  const localLlmPool = summary.pools.find((pool) => pool.resource_class === "local_llm");
  const localLlmFull =
    localLlmPool &&
    localLlmPool.configured_concurrency > 0 &&
    localLlmPool.active_leases >= localLlmPool.configured_concurrency;
  const anomalyTotal = summary.worker_state_anomaly_counts.reduce(
    (total, item) => total + item.count,
    0,
  );

  return (
    <div className="workspace-wave-summary">
      {!summary.workers_enabled ? (
        <p className="empty-text">
          Worker supervisor is disabled on this server. Start/Stop state will be saved, but background claiming needs the server worker guard enabled.
        </p>
      ) : null}
      {summary.broad_runs_disabled ? (
        <p className="empty-text">
          Broad manual runs are disabled while workers are enabled. Use selected run for debug or let workers process the queue.
        </p>
      ) : null}
      {localLlmFull ? (
        <p className="empty-text">
          Local LLM pool is full. New local LLM tasks will wait in Beehive.
        </p>
      ) : null}
      {anomalyTotal > 0 ? (
        <div className="issue-list">
          <article className="issue-row">
            <StatusBadge status="warning" />
            <div>
              <strong>Worker state needs attention</strong>
              <p>
                {summary.worker_state_anomaly_counts
                  .map((item) => `${item.count} ${workerAnomalyLabel(item.diagnosis)}`)
                  .join(", ")}
              </p>
              <div className="button-row">
                <button
                  type="button"
                  className="button secondary"
                  disabled={activeAction !== null}
                  onClick={onReconcileStuck}
                >
                  {activeAction === "worker-repair" ? "Repairing..." : "Repair workers"}
                </button>
              </div>
            </div>
          </article>
        </div>
      ) : null}
      <div className="summary-card-grid">
        <SummaryCard label="Runtime status" value={summary.runtime_status} />
        <SummaryCard label="Scheduling" value={summary.scheduling_policy} />
        <SummaryCard label="Active leases" value={summary.active_leases_total} />
        <SummaryCard label="Expired leases" value={summary.expired_leases_total} />
        <SummaryCard label="Lease sec" value={summary.worker_lease_sec} />
        <SummaryCard label="Heartbeat sec" value={summary.worker_heartbeat_sec} />
        <SummaryCard label="Last recovery" value={summary.last_recovery_at ?? "never"} />
      </div>
      {summary.scheduling_policy === "depth_first" ? (
        <p className="empty-text">
          Depth-first prioritizes fresh child artifacts so a subset of entities moves deeper through the pipeline sooner.
        </p>
      ) : null}
      <div className="worker-pool-grid">
        {summary.pools.map((pool) => (
          <WorkerPoolCard
            key={pool.resource_class}
            pool={pool}
            activeAction={activeAction}
            desiredCount={desiredCounts[pool.resource_class] ?? pool.desired_concurrency}
            onDesiredCountChange={onDesiredCountChange}
            onApplyDesiredPool={onApplyDesiredPool}
            onPausePool={onPausePool}
            onResumePool={onResumePool}
          />
        ))}
      </div>
      {summary.recent_leases.length > 0 ? (
        <details className="diagnostics-block">
          <summary>
            <strong>Recent leases</strong>
            <span className="muted">Latest worker lease records and safe actions</span>
          </summary>
          <div className="table-wrap">
            <table className="workspace-file-table">
              <thead>
                <tr>
                  <th>Lease</th>
                  <th>Worker</th>
                  <th>Entity</th>
                  <th>Stage</th>
                  <th>Pool</th>
                  <th>Status</th>
                  <th>Leased</th>
                  <th>Until</th>
                  <th>Heartbeat</th>
                  <th>Run</th>
                  <th>Release reason</th>
                  <th>Actions</th>
                </tr>
              </thead>
              <tbody>
                {summary.recent_leases.map((lease) => (
                  <tr key={lease.lease_id}>
                    <td>
                      <code>{shortId(lease.lease_id)}</code>
                    </td>
                    <td>{shortId(lease.worker_id)}</td>
                    <td>{lease.entity_id}</td>
                    <td>{lease.stage_id}</td>
                    <td>{lease.resource_class}</td>
                    <td>
                      <StatusBadge status={lease.status} />
                    </td>
                    <td>{lease.leased_at}</td>
                    <td>{lease.lease_until}</td>
                    <td>{lease.heartbeat_at}</td>
                    <td>{lease.run_id ? <code>{shortId(lease.run_id)}</code> : "none"}</td>
                    <td>{lease.release_reason ?? "none"}</td>
                    <td>
                      <div className="button-row">
                        <button
                          type="button"
                          className="button secondary"
                          onClick={() => onOpenEntity(lease.entity_id)}
                        >
                          Open entity
                        </button>
                        {lease.run_id ? (
                          <button
                            type="button"
                            className="button secondary"
                            onClick={() => onOpenEntity(lease.entity_id)}
                          >
                            Open outputs
                          </button>
                        ) : null}
                        <button
                          type="button"
                          className="button secondary"
                          disabled={lease.status !== "active" || activeAction !== null}
                          onClick={() => onReleaseLease(lease)}
                        >
                          {activeAction === `worker-release-${lease.lease_id}` ? "Releasing..." : "Release"}
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </details>
      ) : (
        <p className="empty-text">No worker leases recorded yet.</p>
      )}
      {summary.recent_worker_state_anomalies.length > 0 ? (
        <details className="diagnostics-block">
          <summary>
            <strong>Worker state anomalies</strong>
            <span className="muted">Recent records that can block worker claiming</span>
          </summary>
          <div className="table-wrap">
            <table className="workspace-file-table">
              <thead>
                <tr>
                  <th>Diagnosis</th>
                  <th>Entity</th>
                  <th>Stage</th>
                  <th>Pool</th>
                  <th>State</th>
                  <th>Lease</th>
                  <th>Worker</th>
                  <th>Run</th>
                  <th>Action</th>
                </tr>
              </thead>
              <tbody>
                {summary.recent_worker_state_anomalies.map((anomaly) => (
                  <tr key={`${anomaly.diagnosis}-${anomaly.state_id}-${anomaly.lease_id ?? "none"}`}>
                    <td>{workerAnomalyLabel(anomaly.diagnosis)}</td>
                    <td>{anomaly.entity_id}</td>
                    <td>{anomaly.stage_id}</td>
                    <td>{anomaly.resource_class}</td>
                    <td>{anomaly.state_status ?? "missing"}</td>
                    <td>{anomaly.lease_status ?? "none"}</td>
                    <td>{anomaly.worker_id ? shortId(anomaly.worker_id) : "none"}</td>
                    <td>{anomaly.run_id ? <code>{shortId(anomaly.run_id)}</code> : "none"}</td>
                    <td>{anomaly.recommended_action}</td>
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

function WorkerPoolCard({
  pool,
  activeAction,
  desiredCount,
  onDesiredCountChange,
  onApplyDesiredPool,
  onPausePool,
  onResumePool,
}: {
  pool: WorkerPoolRuntimeSummary;
  activeAction: string | null;
  desiredCount: number;
  onDesiredCountChange: (resourceClass: string, value: number) => void;
  onApplyDesiredPool: (resourceClass: string) => void;
  onPausePool: (resourceClass: string) => void;
  onResumePool: (resourceClass: string) => void;
}) {
  const pendingTotal = pool.pending_count + pool.retry_wait_due_count;
  const requested = pool.requested_desired_concurrency ?? desiredCount;
  const requestWasCapped =
    pool.requested_desired_concurrency !== null &&
    pool.requested_desired_concurrency > pool.desired_concurrency;
  return (
    <div className="summary-card">
      <span>{workerPoolLabel(pool.resource_class)}</span>
      <strong>
        {pool.active_leases}/{pool.effective_concurrency} active
      </strong>
      <span>{pool.is_started ? "started" : "stopped"}</span>
      <span>{pool.is_paused ? "paused" : "resumed"}</span>
      <span>Requested: {requested}</span>
      <span>Applied desired: {pool.desired_concurrency}</span>
      <span>{pool.configured_concurrency} YAML limit</span>
      <span>Env limit: {pool.env_concurrency_limit ?? "not set"}</span>
      <span>Effective: {pool.effective_concurrency}</span>
      {requestWasCapped ? (
        <p className="empty-text">
          Requested {pool.requested_desired_concurrency} {pool.resource_class} workers, but Beehive applied {pool.desired_concurrency}. Increase runtime.worker_pools.{pool.resource_class}.concurrency in pipeline.yaml or Runtime settings.
        </p>
      ) : null}
      <span>{pendingTotal} pending</span>
      <span>{pool.retry_wait_not_due_count} retry wait</span>
      <span>{pool.queued_count} queued</span>
      <span>{pool.in_progress_count} in progress</span>
      <span>{pool.blocked_count} blocked</span>
      <span>{pool.failed_count} failed</span>
      <span>{pool.expired_leases} expired leases</span>
      <span>{pool.oldest_pending_age_sec !== null ? `${pool.oldest_pending_age_sec}s oldest` : "no pending age"}</span>
      <span>{pool.average_duration_ms !== null ? `${pool.average_duration_ms}ms avg` : "no duration"}</span>
      {pool.pause_reason ? <span>{pool.pause_reason}</span> : null}
      {pool.last_error ? <span>{pool.last_error}</span> : null}
      <label className="inline-field" htmlFor={`desired-${pool.resource_class}`}>
        Desired
        <input
          id={`desired-${pool.resource_class}`}
          type="number"
          min={0}
          max={999}
          value={desiredCount}
          disabled={activeAction !== null}
          onChange={(event) => onDesiredCountChange(pool.resource_class, Number(event.target.value))}
        />
      </label>
      <div className="button-row">
        <button
          type="button"
          className="button secondary"
          disabled={activeAction !== null}
          onClick={() => onApplyDesiredPool(pool.resource_class)}
        >
          {activeAction === `worker-desired-${pool.resource_class}` ? "Applying..." : "Apply desired"}
        </button>
        <button
          type="button"
          className="button secondary"
          disabled={pool.is_paused || activeAction !== null}
          onClick={() => onPausePool(pool.resource_class)}
        >
          Pause
        </button>
        <button
          type="button"
          className="button secondary"
          disabled={!pool.is_paused || activeAction !== null}
          onClick={() => onResumePool(pool.resource_class)}
        >
          Resume
        </button>
      </div>
    </div>
  );
}

function workerPoolLabel(resourceClass: string) {
  return resourceClass === "local_llm" ? "Local LLM pool" : "Default pool";
}

function workerRuntimeSummaryText(summary: WorkerSummary | null) {
  if (!summary) return "summary unavailable";
  const pools = summary.pools
    .map(
      (pool) =>
        `${pool.resource_class} ${pool.desired_concurrency} desired/${pool.effective_concurrency} effective/${pool.active_leases} active`,
    )
    .join(", ");
  return `${summary.runtime_status}, ${pools}`;
}

function workerAnomalyLabel(diagnosis: string) {
  switch (diagnosis) {
    case "in_progress_without_active_lease":
      return "in_progress task has no active lease";
    case "queued_without_active_lease":
      return "queued task has no active lease";
    case "active_lease_with_finished_run":
      return "active lease has a finished run";
    case "active_lease_expired":
      return "active lease is expired";
    case "active_lease_without_recent_heartbeat":
      return "active lease has no recent heartbeat";
    case "active_lease_for_missing_state":
      return "active lease points to a missing state";
    case "active_lease_for_state_not_running":
      return "active lease points to a non-running state";
    case "recent_unleased_in_progress":
      return "recent in_progress task has no active lease";
    default:
      return diagnosis.replaceAll("_", " ");
  }
}

function boundedInteger(value: number, min: number, max: number) {
  if (!Number.isFinite(value)) return min;
  return Math.min(max, Math.max(min, Math.floor(value)));
}

function shortId(value: string) {
  return value.length > 12 ? `${value.slice(0, 12)}...` : value;
}

function SummaryCard({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="summary-card">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}
