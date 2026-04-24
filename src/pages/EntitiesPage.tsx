import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { StatusBadge } from "../components/StatusBadge";
import { formatDateTime, shortChecksum } from "../lib/formatters";
import { listEntities } from "../lib/runtimeApi";
import type {
  CommandErrorInfo,
  EntityFilters,
  EntityRecord,
  EntityValidationStatus,
  StageStatus,
} from "../types/domain";

const stageStatuses: StageStatus[] = [
  "pending",
  "queued",
  "in_progress",
  "retry_wait",
  "done",
  "failed",
  "blocked",
  "skipped",
];

const validationStatuses: EntityValidationStatus[] = ["valid", "warning", "invalid"];

export function EntitiesPage() {
  const navigate = useNavigate();
  const { state } = useBootstrap();
  const [entities, setEntities] = useState<EntityRecord[]>([]);
  const [availableStages, setAvailableStages] = useState<string[]>([]);
  const [errors, setErrors] = useState<CommandErrorInfo[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [filters, setFilters] = useState<EntityFilters>({
    stage_id: "",
    status: "",
    validation_status: null,
    search: "",
  });

  const workdirPath = state.selected_workdir_path;
  const canQueryRuntime = state.phase === "fully_initialized" && !!workdirPath;

  const loadEntities = useCallback(async () => {
    if (!canQueryRuntime || !workdirPath) {
      setEntities([]);
      setAvailableStages([]);
      setErrors([]);
      return;
    }

    setIsLoading(true);
    try {
      const result = await listEntities(workdirPath, filters);
      setEntities(result.entities);
      setAvailableStages(result.available_stages);
      setErrors(result.errors);
    } finally {
      setIsLoading(false);
    }
  }, [canQueryRuntime, filters, workdirPath]);

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
        <button
          type="button"
          className="button secondary"
          disabled={!canQueryRuntime || isLoading}
          onClick={() => void loadEntities()}
        >
          Refresh
        </button>
      </div>
      <section className="panel">
        <div className="panel-heading">
          <h2>Filters</h2>
          <span className="muted">{entities.length} entity(s)</span>
        </div>
        <div className="filter-grid">
          <div className="form-row">
            <label htmlFor="entity-search">Search</label>
            <input
              id="entity-search"
              value={filters.search ?? ""}
              onChange={(event) =>
                setFilters((current) => ({ ...current, search: event.target.value }))
              }
              placeholder="Entity ID or filename"
            />
          </div>
          <div className="form-row">
            <label htmlFor="entity-stage-filter">Stage</label>
            <select
              id="entity-stage-filter"
              value={filters.stage_id ?? ""}
              onChange={(event) =>
                setFilters((current) => ({ ...current, stage_id: event.target.value }))
              }
            >
              <option value="">All stages</option>
              {availableStages.map((stageId) => (
                <option key={stageId} value={stageId}>
                  {stageId}
                </option>
              ))}
            </select>
          </div>
          <div className="form-row">
            <label htmlFor="entity-status-filter">Status</label>
            <select
              id="entity-status-filter"
              value={filters.status ?? ""}
              onChange={(event) =>
                setFilters((current) => ({ ...current, status: event.target.value }))
              }
            >
              <option value="">All statuses</option>
              {stageStatuses.map((status) => (
                <option key={status} value={status}>
                  {status}
                </option>
              ))}
            </select>
          </div>
          <div className="form-row">
            <label htmlFor="entity-validation-filter">Validation</label>
            <select
              id="entity-validation-filter"
              value={filters.validation_status ?? ""}
              onChange={(event) =>
                setFilters((current) => ({
                  ...current,
                  validation_status: (event.target.value || null) as EntityValidationStatus | null,
                }))
              }
            >
              <option value="">All validation states</option>
              {validationStatuses.map((status) => (
                <option key={status} value={status}>
                  {status}
                </option>
              ))}
            </select>
          </div>
        </div>
      </section>
      <CommandErrorsPanel title="Entity Query Errors" errors={errors} />
      <section className="panel">
        <div className="panel-heading">
          <h2>Registered Entities</h2>
          <span className="muted">
            {isLoading ? "Loading..." : canQueryRuntime ? "Click a row for details" : "Open a workdir to query runtime data"}
          </span>
        </div>
        {!canQueryRuntime ? (
          <p className="empty-text">Open or initialize a valid workdir to view registered entities.</p>
        ) : entities.length === 0 ? (
          <p className="empty-text">No entities match the current filters.</p>
        ) : (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Entity ID</th>
                  <th>File</th>
                  <th>Stage</th>
                  <th>Status</th>
                  <th>Validation</th>
                  <th>File path</th>
                  <th>Checksum</th>
                  <th>Updated at</th>
                  <th>Errors</th>
                </tr>
              </thead>
              <tbody>
                {entities.map((entity) => (
                  <tr
                    key={`${entity.entity_id}-${entity.file_path}`}
                    className="clickable-row"
                    onClick={() => void navigate(`/entities/${entity.entity_id}`)}
                  >
                    <td>
                      <strong>{entity.entity_id}</strong>
                    </td>
                    <td>{entity.file_name}</td>
                    <td>{entity.stage_id}</td>
                    <td>
                      <StatusBadge status={entity.status} />
                    </td>
                    <td>
                      <StatusBadge status={entity.validation_status} />
                    </td>
                    <td>
                      <code>{entity.file_path}</code>
                    </td>
                    <td>
                      <code>{shortChecksum(entity.checksum)}</code>
                    </td>
                    <td>{formatDateTime(entity.updated_at)}</td>
                    <td>{entity.validation_errors.length}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  );
}
