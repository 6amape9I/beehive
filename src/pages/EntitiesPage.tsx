import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { EntitiesTable } from "../components/entities/EntitiesTable";
import { EntityFilters } from "../components/entities/EntityFilters";
import { PaginationControls } from "../components/entities/PaginationControls";
import {
  archiveWorkspaceEntity,
  importWorkspaceEntitiesJsonBatch,
  listEntities,
  listWorkspaceEntities,
  restoreWorkspaceEntity,
  runSelectedPipelineWavesById,
  scanWorkspace,
} from "../lib/runtimeApi";
import type {
  CommandErrorInfo,
  EntityListQuery,
  EntityListSortBy,
  EntityTableRow,
  EntityValidationStatus,
  ImportJsonFileResult,
  SortDirection,
} from "../types/domain";

const defaultSortBy: EntityListSortBy = "updated_at";
const defaultSortDirection: SortDirection = "desc";
const defaultPageSize = 50;
const importBatchSize = 25;

interface UploadSummary {
  imported: number;
  failed: number;
  invalid: number;
  files: ImportJsonFileResult[];
}

function queryFromSearchParams(params: URLSearchParams): EntityListQuery {
  return {
    search: params.get("search") ?? "",
    stage_id: params.get("stage") ?? "",
    status: params.get("status") ?? "",
    validation_status: (params.get("validation") || null) as EntityValidationStatus | null,
    include_archived: params.get("archived") === "true",
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
  if (query.include_archived) params.set("archived", "true");
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
  const { workspaceId: routeWorkspaceId } = useParams();
  const { state } = useBootstrap();
  const [entities, setEntities] = useState<EntityTableRow[]>([]);
  const [availableStages, setAvailableStages] = useState<string[]>([]);
  const [availableStatuses, setAvailableStatuses] = useState<string[]>([]);
  const [selectedEntityIds, setSelectedEntityIds] = useState<string[]>([]);
  const [uploadStageId, setUploadStageId] = useState("");
  const [total, setTotal] = useState(0);
  const [errors, setErrors] = useState<CommandErrorInfo[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [isScanning, setIsScanning] = useState(false);
  const [isUploading, setIsUploading] = useState(false);
  const [isRunningSelected, setIsRunningSelected] = useState(false);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [uploadSummary, setUploadSummary] = useState<UploadSummary | null>(null);

  const query = useMemo(() => queryFromSearchParams(searchParams), [searchParams]);
  const workdirPath = state.selected_workdir_path;
  const workspaceId = routeWorkspaceId ?? state.selected_workspace_id;
  const canQueryRuntime = Boolean(workspaceId || (state.phase === "fully_initialized" && workdirPath));
  const canUseWorkspaceActions = Boolean(workspaceId);

  const selectedRows = useMemo(
    () => entities.filter((entity) => selectedEntityIds.includes(entity.entity_id)),
    [entities, selectedEntityIds],
  );
  const selectedFileIds = selectedRows
    .map((entity) => entity.latest_file_id)
    .filter((id): id is number => typeof id === "number");

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
      const result =
        workspaceId !== null && workspaceId !== undefined
          ? await listWorkspaceEntities(workspaceId, query)
          : await listEntities(workdirPath ?? "", query);
      setEntities(result.entities);
      setAvailableStages(result.available_stages);
      setAvailableStatuses(result.available_statuses);
      setTotal(result.total);
      setErrors(result.errors);
      setSelectedEntityIds((current) =>
        current.filter((entityId) => result.entities.some((entity) => entity.entity_id === entityId)),
      );
      if (!uploadStageId && result.available_stages[0]) {
        setUploadStageId(result.available_stages[0]);
      }
    } finally {
      setIsLoading(false);
    }
  }, [canQueryRuntime, query, uploadStageId, workdirPath, workspaceId]);

  const hasActiveFilters = Boolean(
    query.search || query.stage_id || query.status || query.validation_status || query.include_archived,
  );

  const handleScanWorkspace = useCallback(async () => {
    if (!workdirPath) {
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
  }, [loadEntities, workdirPath]);

  async function handleUploadFiles(fileList: FileList | null) {
    if (!workspaceId || !uploadStageId || !fileList?.length) return;
    setIsUploading(true);
    setActionMessage(null);
    setUploadSummary(null);
    const files = Array.from(fileList).filter((file) => file.name.toLowerCase().endsWith(".json"));
    const parsed: Array<{ relative_path: string; file_name: string; content: Record<string, unknown> }> = [];
    let invalid = 0;
    const localFailures: ImportJsonFileResult[] = [];

    for (const file of files) {
      const relativePath = (file as File & { webkitRelativePath?: string }).webkitRelativePath || file.name;
      try {
        const content = JSON.parse(await file.text()) as unknown;
        if (!content || typeof content !== "object" || Array.isArray(content)) {
          invalid += 1;
          localFailures.push({
            file_name: file.name,
            status: "invalid",
            entity_id: null,
            artifact_id: null,
            bucket: null,
            key: null,
            object_key: null,
            error: "JSON content must be an object.",
          });
          continue;
        }
        parsed.push({
          relative_path: relativePath,
          file_name: file.name,
          content: content as Record<string, unknown>,
        });
      } catch (error) {
        invalid += 1;
        localFailures.push({
          file_name: file.name,
          status: "invalid",
          entity_id: null,
          artifact_id: null,
          bucket: null,
          key: null,
          object_key: null,
          error: error instanceof Error ? error.message : "Invalid JSON.",
        });
      }
    }

    const remoteResults: ImportJsonFileResult[] = [];
    try {
      for (let index = 0; index < parsed.length; index += importBatchSize) {
        const batch = parsed.slice(index, index + importBatchSize);
        const result = await importWorkspaceEntitiesJsonBatch(workspaceId, {
          stage_id: uploadStageId,
          files: batch,
          options: { overwrite_existing: false },
        });
        setErrors(result.errors);
        if (result.payload) {
          remoteResults.push(...result.payload.files);
        }
      }
      const imported = remoteResults.filter((file) => file.status === "imported").length;
      const failed = remoteResults.filter((file) => file.status === "failed").length;
      setUploadSummary({
        imported,
        failed,
        invalid,
        files: [...localFailures, ...remoteResults],
      });
      setActionMessage(`Upload finished: ${imported} imported, ${invalid + failed} issue(s).`);
      await loadEntities();
    } finally {
      setIsUploading(false);
    }
  }

  async function handleArchiveEntity(entityId: string) {
    if (!workspaceId) return;
    const result = await archiveWorkspaceEntity(workspaceId, entityId);
    setErrors(result.errors);
    if (result.payload) {
      setActionMessage(`Entity ${entityId} archived.`);
      await loadEntities();
    }
  }

  async function handleRestoreEntity(entityId: string) {
    if (!workspaceId) return;
    const result = await restoreWorkspaceEntity(workspaceId, entityId);
    setErrors(result.errors);
    if (result.payload) {
      setActionMessage(`Entity ${entityId} restored.`);
      await loadEntities();
    }
  }

  async function handleRunSelected() {
    if (!workspaceId || selectedFileIds.length === 0) return;
    setIsRunningSelected(true);
    setActionMessage(null);
    try {
      const result = await runSelectedPipelineWavesById(workspaceId, selectedFileIds, 5, 3, true);
      setErrors(result.errors);
      if (result.summary) {
        setActionMessage(
          `Selected run finished: ${result.summary.total_succeeded} success, ${result.summary.total_failed} failed.`,
        );
      }
      await loadEntities();
    } finally {
      setIsRunningSelected(false);
    }
  }

  function toggleSelected(row: EntityTableRow) {
    setSelectedEntityIds((current) =>
      current.includes(row.entity_id)
        ? current.filter((entityId) => entityId !== row.entity_id)
        : [...current, row.entity_id],
    );
  }

  useEffect(() => {
    void loadEntities();
  }, [loadEntities]);

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Runtime</span>
          <h1>Entities</h1>
          {workspaceId ? <span className="muted">Workspace {workspaceId}</span> : null}
        </div>
        <div className="button-row">
          <button
            type="button"
            className="button primary"
            disabled={!canUseWorkspaceActions || selectedFileIds.length === 0 || isRunningSelected}
            onClick={() => void handleRunSelected()}
          >
            {isRunningSelected ? "Running..." : `Run selected pipeline waves (${selectedFileIds.length})`}
          </button>
          <button
            type="button"
            className="button secondary"
            disabled={!canQueryRuntime || isLoading || isScanning || isUploading}
            onClick={() => void loadEntities()}
          >
            Refresh
          </button>
          <button
            type="button"
            className="button"
            disabled={!workdirPath || isLoading || isScanning}
            onClick={() => void handleScanWorkspace()}
          >
            {isScanning ? "Scanning..." : "Scan workspace"}
          </button>
        </div>
      </div>

      {canUseWorkspaceActions ? (
        <section className="panel">
          <div className="panel-heading">
            <div>
              <h2>Upload entities</h2>
              <span className="muted">JSON object files are uploaded to the selected stage.</span>
            </div>
          </div>
          <div className="stage-editor-form-grid">
            <div className="form-row">
              <label htmlFor="entity-upload-stage">Target stage</label>
              <select
                id="entity-upload-stage"
                value={uploadStageId}
                disabled={isUploading || availableStages.length === 0}
                onChange={(event) => setUploadStageId(event.target.value)}
              >
                <option value="">Select stage</option>
                {availableStages.map((stageId) => (
                  <option key={stageId} value={stageId}>
                    {stageId}
                  </option>
                ))}
              </select>
            </div>
            <div className="form-row">
              <label htmlFor="entity-folder-upload">Folder</label>
              <input
                id="entity-folder-upload"
                type="file"
                multiple
                accept="application/json,.json"
                disabled={!uploadStageId || isUploading}
                {...{ webkitdirectory: "", directory: "" }}
                onChange={(event) => void handleUploadFiles(event.target.files)}
              />
            </div>
          </div>
          {uploadSummary ? (
            <p className="field-hint">
              Imported {uploadSummary.imported}, invalid {uploadSummary.invalid}, failed{" "}
              {uploadSummary.failed}.
            </p>
          ) : null}
        </section>
      ) : null}

      <EntityFilters
        query={query}
        availableStages={availableStages}
        availableStatuses={availableStatuses}
        disabled={!canQueryRuntime || isLoading || isScanning || isUploading}
        onChange={setQuery}
        onClear={clearFilters}
      />

      <CommandErrorsPanel title="Entity Errors" errors={errors} />
      {actionMessage ? <p className="empty-text">{actionMessage}</p> : null}

      <section className="panel">
        <div className="panel-heading">
          <h2>Registered Entities</h2>
          <span className="muted">
            {isLoading
              ? "Loading..."
              : canQueryRuntime
                ? `${total} matching entity row(s)`
                : "Open a workspace to query runtime data"}
          </span>
        </div>
        {!canQueryRuntime ? (
          <p className="empty-text">Open a workspace to view registered entities.</p>
        ) : entities.length === 0 ? (
          <div className="empty-text">
            <p>
              {hasActiveFilters
                ? "No entities match the current filters."
                : "No entities are registered yet."}
            </p>
          </div>
        ) : (
          <>
            <EntitiesTable
              rows={entities}
              selectedEntityIds={selectedEntityIds}
              sortBy={query.sort_by ?? defaultSortBy}
              sortDirection={query.sort_direction ?? defaultSortDirection}
              onSortChange={(sortBy, sortDirection) =>
                setQuery({ sort_by: sortBy, sort_direction: sortDirection, page: 1 })
              }
              onToggleSelected={toggleSelected}
              onArchive={(entityId) => void handleArchiveEntity(entityId)}
              onRestore={(entityId) => void handleRestoreEntity(entityId)}
              onRowClick={(entityId) =>
                void navigate(
                  workspaceId
                    ? `/workspaces/${encodeURIComponent(workspaceId)}/entities/${encodeURIComponent(entityId)}`
                    : `/entities/${encodeURIComponent(entityId)}`,
                )
              }
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
