import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { StatusBadge } from "../components/StatusBadge";
import { formatDateTime, shortChecksum } from "../lib/formatters";
import {
  getWorkspaceExplorer,
  openEntityFile,
  openEntityFolder,
  scanWorkspace,
} from "../lib/runtimeApi";
import type {
  CommandErrorInfo,
  EntityValidationStatus,
  WorkspaceEntityTrail,
  WorkspaceExplorerResult,
  WorkspaceFileNode,
  WorkspaceStageTree,
} from "../types/domain";

interface ExplorerFilters {
  search: string;
  stageId: string;
  runtimeStatus: string;
  validationStatus: "" | EntityValidationStatus;
  showMissing: boolean;
  showInvalid: boolean;
  showInactive: boolean;
  showManaged: boolean;
}

const defaultFilters: ExplorerFilters = {
  search: "",
  stageId: "",
  runtimeStatus: "",
  validationStatus: "",
  showMissing: true,
  showInvalid: true,
  showInactive: true,
  showManaged: true,
};

export function WorkspaceExplorerPage() {
  const { state } = useBootstrap();
  const navigate = useNavigate();
  const [explorer, setExplorer] = useState<WorkspaceExplorerResult | null>(null);
  const [errors, setErrors] = useState<CommandErrorInfo[]>([]);
  const [filters, setFilters] = useState<ExplorerFilters>(defaultFilters);
  const [selectedFileId, setSelectedFileId] = useState<number | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [activeAction, setActiveAction] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);

  const workdirPath = state.selected_workdir_path;
  const canQueryRuntime = state.phase === "fully_initialized" && !!workdirPath;

  const loadExplorer = useCallback(async () => {
    if (!canQueryRuntime || !workdirPath) {
      setExplorer(null);
      setErrors([]);
      return;
    }

    setIsLoading(true);
    try {
      const result = await getWorkspaceExplorer(workdirPath);
      setExplorer(result);
      setErrors(result.errors);
      if (selectedFileId && !result.stages.some((stage) => stage.files.some((file) => file.entity_file_id === selectedFileId))) {
        setSelectedFileId(null);
      }
    } finally {
      setIsLoading(false);
    }
  }, [canQueryRuntime, selectedFileId, workdirPath]);

  useEffect(() => {
    void loadExplorer();
  }, [loadExplorer]);

  const stageOptions = useMemo(
    () => explorer?.stages.map((stage) => stage.stage_id) ?? [],
    [explorer],
  );
  const runtimeStatuses = useMemo(() => {
    const statuses = new Set<string>();
    explorer?.stages.forEach((stage) => {
      stage.files.forEach((file) => {
        if (file.runtime_status) statuses.add(file.runtime_status);
      });
    });
    return Array.from(statuses).sort();
  }, [explorer]);

  const filteredStages = useMemo(() => {
    if (!explorer) return [];
    return explorer.stages
      .filter((stage) => filters.showInactive || stage.is_active)
      .filter((stage) => !filters.stageId || stage.stage_id === filters.stageId)
      .map((stage) => ({
        ...stage,
        files: stage.files.filter((file) => fileMatchesFilters(file, filters)),
        invalid_files: filters.showInvalid
          ? stage.invalid_files.filter((item) => invalidItemMatchesSearch(item, filters.search))
          : [],
      }));
  }, [explorer, filters]);

  const selectedFile = useMemo(() => {
    if (!explorer || !selectedFileId) return null;
    for (const stage of explorer.stages) {
      const file = stage.files.find((item) => item.entity_file_id === selectedFileId);
      if (file) return file;
    }
    return null;
  }, [explorer, selectedFileId]);

  const selectedTrail = useMemo(() => {
    if (!explorer || !selectedFile) return null;
    return (
      explorer.entity_trails.find((trail) => trail.entity_id === selectedFile.entity_id) ??
      null
    );
  }, [explorer, selectedFile]);

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
          : "Scan finished with no summary.",
      );
      await loadExplorer();
    } finally {
      setActiveAction(null);
    }
  }

  async function handleOpen(kind: "file" | "folder", fileId: number) {
    if (!workdirPath) return;
    setActiveAction(`${kind}:${fileId}`);
    setActionMessage(null);
    try {
      const result =
        kind === "file"
          ? await openEntityFile(workdirPath, fileId)
          : await openEntityFolder(workdirPath, fileId);
      setErrors(result.errors);
      setActionMessage(result.payload ? `Opened ${result.payload.opened_path}` : null);
    } finally {
      setActiveAction(null);
    }
  }

  function goToEntity(file: WorkspaceFileNode) {
    navigate(`/entities/${encodeURIComponent(file.entity_id)}?file_id=${file.entity_file_id}`);
  }

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Filesystem</span>
          <h1>Workspace Explorer</h1>
          <span className="muted">
            {explorer?.workdir_path ?? workdirPath ?? "No workdir selected"}
          </span>
        </div>
        <div className="button-row">
          <button
            type="button"
            className="button secondary"
            disabled={!canQueryRuntime || isLoading}
            onClick={() => void loadExplorer()}
          >
            {isLoading ? "Refreshing..." : "Refresh"}
          </button>
          <button
            type="button"
            className="button primary"
            disabled={!canQueryRuntime || activeAction === "scan"}
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
          <p className="empty-text">Open or initialize a valid workdir to inspect stage folders and artifacts.</p>
        </section>
      ) : isLoading && !explorer ? (
        <section className="panel">
          <p className="empty-text">Loading workspace explorer...</p>
        </section>
      ) : explorer ? (
        <>
          <ExplorerSummary explorer={explorer} />
          <ExplorerFiltersPanel
            filters={filters}
            stageOptions={stageOptions}
            runtimeStatuses={runtimeStatuses}
            onChange={setFilters}
          />
          {filteredStages.length === 0 ? (
            <section className="panel">
              <p className="empty-text">No stage folders match the current filters.</p>
            </section>
          ) : (
            <div className="workspace-layout">
              <div className="workspace-stage-tree">
                {filteredStages.map((stage) => (
                  <StageTreePanel
                    key={stage.stage_id}
                    stage={stage}
                    selectedFileId={selectedFileId}
                    activeAction={activeAction}
                    onSelectFile={setSelectedFileId}
                    onOpenFile={(fileId) => void handleOpen("file", fileId)}
                    onOpenFolder={(fileId) => void handleOpen("folder", fileId)}
                    onGoToEntity={goToEntity}
                  />
                ))}
              </div>
              <TrailPanel
                trail={selectedTrail}
                selectedFile={selectedFile}
                activeAction={activeAction}
                onOpenFile={(fileId) => void handleOpen("file", fileId)}
                onOpenFolder={(fileId) => void handleOpen("folder", fileId)}
                onGoToEntity={goToEntity}
              />
            </div>
          )}
        </>
      ) : (
        <section className="panel">
          <p className="empty-text">Workspace explorer data is not available.</p>
        </section>
      )}
    </div>
  );
}

function ExplorerSummary({ explorer }: { explorer: WorkspaceExplorerResult }) {
  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h2>Workdir Tree</h2>
          <span className="muted">
            Generated {formatDateTime(explorer.generated_at)} / last scan{" "}
            {formatDateTime(explorer.last_scan_at)}
          </span>
        </div>
      </div>
      <div className="summary-card-grid">
        <SummaryCard label="Stages" value={`${explorer.totals.active_stages_total} active / ${explorer.totals.inactive_stages_total} inactive`} />
        <SummaryCard label="Entities" value={explorer.totals.entities_total} />
        <SummaryCard label="Registered files" value={explorer.totals.registered_files_total} />
        <SummaryCard label="Present / missing" value={`${explorer.totals.present_files_total} / ${explorer.totals.missing_files_total}`} />
        <SummaryCard label="Invalid last scan" value={explorer.totals.invalid_files_total} />
        <SummaryCard label="Managed copies" value={explorer.totals.managed_copies_total} />
      </div>
    </section>
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

interface ExplorerFiltersPanelProps {
  filters: ExplorerFilters;
  stageOptions: string[];
  runtimeStatuses: string[];
  onChange: (filters: ExplorerFilters) => void;
}

function ExplorerFiltersPanel({
  filters,
  stageOptions,
  runtimeStatuses,
  onChange,
}: ExplorerFiltersPanelProps) {
  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>Filters</h2>
        <button type="button" className="button secondary" onClick={() => onChange(defaultFilters)}>
          Clear
        </button>
      </div>
      <div className="filter-grid">
        <label>
          Search
          <input
            value={filters.search}
            onChange={(event) => onChange({ ...filters, search: event.target.value })}
            placeholder="Entity, file, or path"
          />
        </label>
        <label>
          Stage
          <select
            value={filters.stageId}
            onChange={(event) => onChange({ ...filters, stageId: event.target.value })}
          >
            <option value="">All stages</option>
            {stageOptions.map((stageId) => (
              <option key={stageId} value={stageId}>
                {stageId}
              </option>
            ))}
          </select>
        </label>
        <label>
          Runtime status
          <select
            value={filters.runtimeStatus}
            onChange={(event) => onChange({ ...filters, runtimeStatus: event.target.value })}
          >
            <option value="">All statuses</option>
            {runtimeStatuses.map((status) => (
              <option key={status} value={status}>
                {status}
              </option>
            ))}
          </select>
        </label>
        <label>
          Validation
          <select
            value={filters.validationStatus}
            onChange={(event) =>
              onChange({
                ...filters,
                validationStatus: event.target.value as ExplorerFilters["validationStatus"],
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
            checked={filters.showMissing}
            onChange={(event) => onChange({ ...filters, showMissing: event.target.checked })}
          />
          Show missing
        </label>
        <label>
          <input
            type="checkbox"
            checked={filters.showInvalid}
            onChange={(event) => onChange({ ...filters, showInvalid: event.target.checked })}
          />
          Show invalid
        </label>
        <label>
          <input
            type="checkbox"
            checked={filters.showInactive}
            onChange={(event) => onChange({ ...filters, showInactive: event.target.checked })}
          />
          Show inactive
        </label>
        <label>
          <input
            type="checkbox"
            checked={filters.showManaged}
            onChange={(event) => onChange({ ...filters, showManaged: event.target.checked })}
          />
          Show managed copies
        </label>
      </div>
    </section>
  );
}

interface StageTreePanelProps {
  stage: WorkspaceStageTree;
  selectedFileId: number | null;
  activeAction: string | null;
  onSelectFile: (fileId: number) => void;
  onOpenFile: (fileId: number) => void;
  onOpenFolder: (fileId: number) => void;
  onGoToEntity: (file: WorkspaceFileNode) => void;
}

function StageTreePanel({
  stage,
  selectedFileId,
  activeAction,
  onSelectFile,
  onOpenFile,
  onOpenFolder,
  onGoToEntity,
}: StageTreePanelProps) {
  return (
    <details className="panel workspace-stage-panel" open>
      <summary>
        <div>
          <strong>{stage.stage_id}</strong>
          <span className="muted">{stage.input_folder}</span>
        </div>
        <div className="button-row">
          <StatusBadge status={stage.is_active ? "active" : "inactive"} />
          <StatusBadge status={stage.folder_exists ? "folder_ready" : "folder_missing"} />
        </div>
      </summary>
      {!stage.is_active ? (
        <p className="muted">Inactive stage: historical files remain visible, but new files are not scanned here.</p>
      ) : null}
      <div className="inline-meta">
        <span>folder {stage.folder_path}</span>
        <span>output {stage.output_folder ?? "not required"}</span>
        <span>next {stage.next_stage ?? "terminal"}</span>
        <span>registered {stage.counters.registered_files}</span>
        <span>missing {stage.counters.missing_files}</span>
        <span>invalid {stage.counters.invalid_files}</span>
        <span>managed {stage.counters.managed_copies}</span>
      </div>
      <div className="inline-meta">
        <span>pending {stage.counters.pending}</span>
        <span>queued {stage.counters.queued}</span>
        <span>in progress {stage.counters.in_progress}</span>
        <span>retry {stage.counters.retry_wait}</span>
        <span>done {stage.counters.done}</span>
        <span>failed {stage.counters.failed}</span>
        <span>blocked {stage.counters.blocked}</span>
        <span>skipped {stage.counters.skipped}</span>
      </div>
      <div className="workspace-stage-content">
        <div>
          <h3>Registered JSON files</h3>
          {stage.files.length === 0 ? (
            <p className="empty-text">No registered files match the current filters for this stage.</p>
          ) : (
            <div className="table-wrap">
              <table className="workspace-file-table">
                <thead>
                  <tr>
                    <th>Entity / file</th>
                    <th>Path</th>
                    <th>Runtime</th>
                    <th>File status</th>
                    <th>Validation</th>
                    <th>Presence</th>
                    <th>Copy</th>
                    <th>Checksum</th>
                    <th>Updated</th>
                    <th>Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {stage.files.map((file) => {
                    const busy =
                      activeAction === `file:${file.entity_file_id}` ||
                      activeAction === `folder:${file.entity_file_id}`;
                    return (
                      <tr
                        key={file.entity_file_id}
                        className={selectedFileId === file.entity_file_id ? "selected-row" : ""}
                      >
                        <td>
                          <div className="stacked-cell">
                            <strong>{file.entity_id}</strong>
                            <span className="muted">file #{file.entity_file_id}</span>
                          </div>
                        </td>
                        <td>
                          <code>{file.file_path}</code>
                        </td>
                        <td>
                          {file.runtime_status ? (
                            <StatusBadge status={file.runtime_status} />
                          ) : (
                            <span className="muted">No state</span>
                          )}
                        </td>
                        <td>
                          <StatusBadge status={file.file_status} />
                        </td>
                        <td>
                          <StatusBadge status={file.validation_status} />
                        </td>
                        <td>
                          {file.file_exists
                            ? "Present"
                            : `Missing since ${formatDateTime(file.missing_since)}`}
                        </td>
                        <td>
                          {file.is_managed_copy ? (
                            <div className="stacked-cell">
                              <StatusBadge status="managed_copy" />
                              <span className="muted">
                                from {file.copy_source_stage_id ?? "unknown"} #{file.copy_source_file_id ?? "?"}
                              </span>
                            </div>
                          ) : (
                            "Original/observed"
                          )}
                        </td>
                        <td>
                          <code>{shortChecksum(file.checksum)}</code>
                        </td>
                        <td>{formatDateTime(file.updated_at)}</td>
                        <td>
                          <div className="button-row">
                            <button type="button" className="button secondary" onClick={() => onSelectFile(file.entity_file_id)}>
                              Trail
                            </button>
                            <button
                              type="button"
                              className="button secondary"
                              disabled={busy || !file.can_open_file}
                              onClick={() => onOpenFile(file.entity_file_id)}
                            >
                              File
                            </button>
                            <button
                              type="button"
                              className="button secondary"
                              disabled={busy || !file.can_open_folder}
                              onClick={() => onOpenFolder(file.entity_file_id)}
                            >
                              Folder
                            </button>
                            <button type="button" className="button secondary" onClick={() => onGoToEntity(file)}>
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
        </div>
        <div>
          <h3>Invalid files from last scan</h3>
          {stage.invalid_files.length === 0 ? (
            <p className="empty-text">No invalid last-scan items match this stage.</p>
          ) : (
            <div className="issue-list">
              {stage.invalid_files.map((item) => (
                <article className="issue-row" key={`${item.file_path}-${item.code}-${item.created_at}`}>
                  <StatusBadge status="error" />
                  <div>
                    <strong>{item.file_name || item.code}</strong>
                    <p>{item.message}</p>
                    <code>{item.file_path}</code>
                    <p className="muted">
                      {item.code} / {formatDateTime(item.created_at)}
                    </p>
                  </div>
                </article>
              ))}
            </div>
          )}
        </div>
      </div>
    </details>
  );
}

interface TrailPanelProps {
  trail: WorkspaceEntityTrail | null;
  selectedFile: WorkspaceFileNode | null;
  activeAction: string | null;
  onOpenFile: (fileId: number) => void;
  onOpenFolder: (fileId: number) => void;
  onGoToEntity: (file: WorkspaceFileNode) => void;
}

function TrailPanel({
  trail,
  selectedFile,
  activeAction,
  onOpenFile,
  onOpenFolder,
  onGoToEntity,
}: TrailPanelProps) {
  if (!selectedFile || !trail) {
    return (
      <section className="panel workspace-trail-panel">
        <h2>Artifact Trail</h2>
        <p className="empty-text">Select a registered file to inspect its entity trail.</p>
      </section>
    );
  }

  return (
    <section className="panel workspace-trail-panel">
      <div className="panel-heading">
        <div>
          <h2>Artifact Trail</h2>
          <span className="muted">{trail.entity_id} / {trail.file_count} file instance(s)</span>
        </div>
        <button type="button" className="button secondary" onClick={() => onGoToEntity(selectedFile)}>
          Go to Entity Detail
        </button>
      </div>
      <div className="timeline-list">
        {trail.stages.map((node) => {
          const busy =
            activeAction === `file:${node.entity_file_id}` ||
            activeAction === `folder:${node.entity_file_id}`;
          return (
            <article
              className={`timeline-row ${node.entity_file_id === selectedFile.entity_file_id ? "selected-row" : ""}`}
              key={node.entity_file_id}
            >
              <div>
                <strong>{node.stage_id}</strong>
                <p className="muted">file #{node.entity_file_id}</p>
              </div>
              <div>
                {node.runtime_status ? <StatusBadge status={node.runtime_status} /> : <span className="muted">No state</span>}
              </div>
              <div className="stacked-cell">
                <code>{node.file_path}</code>
                <span>{node.file_exists ? "Present" : "Missing"} / {node.is_managed_copy ? "managed copy" : "observed file"}</span>
                <div className="button-row">
                  <button
                    type="button"
                    className="button secondary"
                    disabled={busy || !node.file_exists}
                    onClick={() => onOpenFile(node.entity_file_id)}
                  >
                    File
                  </button>
                  <button
                    type="button"
                    className="button secondary"
                    disabled={busy}
                    onClick={() => onOpenFolder(node.entity_file_id)}
                  >
                    Folder
                  </button>
                </div>
              </div>
            </article>
          );
        })}
      </div>
      <h3>Relations</h3>
      {trail.edges.length === 0 ? (
        <p className="empty-text">No copy or inferred stage-sequence relations are available.</p>
      ) : (
        <div className="issue-list">
          {trail.edges.map((edge) => (
            <article className="issue-row" key={`${edge.from_entity_file_id}-${edge.to_entity_file_id}-${edge.relation}`}>
              <StatusBadge status={edge.relation.includes("inferred") ? "warning" : "ok"} />
              <div>
                <strong>
                  #{edge.from_entity_file_id} {"->"} #{edge.to_entity_file_id}
                </strong>
                <p>{edge.relation.replaceAll("_", " ")}</p>
                {edge.created_child_path ? <code>{edge.created_child_path}</code> : null}
              </div>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}

function fileMatchesFilters(file: WorkspaceFileNode, filters: ExplorerFilters) {
  if (!filters.showMissing && !file.file_exists) return false;
  if (!filters.showManaged && file.is_managed_copy) return false;
  if (filters.runtimeStatus && file.runtime_status !== filters.runtimeStatus) return false;
  if (filters.validationStatus && file.validation_status !== filters.validationStatus) return false;
  const search = filters.search.trim().toLowerCase();
  if (!search) return true;
  return [file.entity_id, file.file_name, file.file_path, file.stage_id, file.current_stage, file.next_stage]
    .filter(Boolean)
    .some((value) => value!.toLowerCase().includes(search));
}

function invalidItemMatchesSearch(
  item: WorkspaceStageTree["invalid_files"][number],
  searchValue: string,
) {
  const search = searchValue.trim().toLowerCase();
  if (!search) return true;
  return [item.file_name, item.file_path, item.code, item.message]
    .filter(Boolean)
    .some((value) => value.toLowerCase().includes(search));
}
