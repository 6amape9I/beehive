import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { StatusBadge } from "../components/StatusBadge";
import { formatDateTime, shortChecksum } from "../lib/formatters";
import {
  getWorkspaceExplorer,
  getWorkspaceExplorerById,
  listStageRunOutputs,
  openEntityFile,
  openEntityFolder,
  reconcileS3Workspace,
  reconcileS3WorkspaceById,
  registerS3SourceArtifact,
  registerS3SourceArtifactById,
  runDueTasksLimited,
  runDueTasksLimitedById,
  runPipelineWaves,
  runPipelineWavesById,
  runSelectedPipelineWavesById,
  scanWorkspace,
} from "../lib/runtimeApi";
import type {
  CommandErrorInfo,
  EntityValidationStatus,
  RunPipelineWavesSummary,
  RunSelectedPipelineWavesSummary,
  RegisterS3SourceArtifactRequest,
  S3ReconciliationSummary,
  StageRunOutputsPayload,
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

interface ManualS3RegistrationForm {
  stage_id: string;
  entity_id: string;
  artifact_id: string;
  bucket: string;
  key: string;
  version_id: string;
  etag: string;
  checksum_sha256: string;
  size: string;
}

interface PipelineWaveControls {
  max_waves: number;
  max_tasks_per_wave: number;
  stop_on_first_failure: boolean;
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

const defaultManualS3RegistrationForm: ManualS3RegistrationForm = {
  stage_id: "",
  entity_id: "",
  artifact_id: "",
  bucket: "",
  key: "",
  version_id: "",
  etag: "",
  checksum_sha256: "",
  size: "",
};

const defaultPipelineWaveControls: PipelineWaveControls = {
  max_waves: 5,
  max_tasks_per_wave: 3,
  stop_on_first_failure: true,
};

export function WorkspaceExplorerPage() {
  const { state } = useBootstrap();
  const { workspaceId: routeWorkspaceId } = useParams();
  const navigate = useNavigate();
  const [explorer, setExplorer] = useState<WorkspaceExplorerResult | null>(null);
  const [errors, setErrors] = useState<CommandErrorInfo[]>([]);
  const [filters, setFilters] = useState<ExplorerFilters>(defaultFilters);
  const [selectedFileId, setSelectedFileId] = useState<number | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [activeAction, setActiveAction] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [s3Summary, setS3Summary] = useState<S3ReconciliationSummary | null>(null);
  const [manualS3Registration, setManualS3Registration] = useState<ManualS3RegistrationForm>(
    defaultManualS3RegistrationForm,
  );
  const [batchLimit, setBatchLimit] = useState(3);
  const [pipelineWaveControls, setPipelineWaveControls] = useState<PipelineWaveControls>(
    defaultPipelineWaveControls,
  );
  const [pipelineWaveSummary, setPipelineWaveSummary] =
    useState<RunPipelineWavesSummary | null>(null);
  const [selectedRootFileIds, setSelectedRootFileIds] = useState<number[]>([]);
  const [selectedPipelineSummary, setSelectedPipelineSummary] =
    useState<RunSelectedPipelineWavesSummary | null>(null);

  const workdirPath = state.selected_workdir_path;
  const workspaceId = routeWorkspaceId ?? state.selected_workspace_id;
  const canQueryRuntime =
    (!!workspaceId && !workdirPath) ||
    (state.phase === "fully_initialized" && (!!workdirPath || !!workspaceId));

  const loadExplorer = useCallback(async () => {
    if (!canQueryRuntime || (!workdirPath && !workspaceId)) {
      setExplorer(null);
      setErrors([]);
      return;
    }

    setIsLoading(true);
    try {
      const result = workspaceId
        ? await getWorkspaceExplorerById(workspaceId)
        : await getWorkspaceExplorer(workdirPath as string);
      setExplorer(result);
      setErrors(result.errors);
      setSelectedFileId((current) =>
        current && !result.stages.some((stage) => stage.files.some((file) => file.entity_file_id === current))
          ? null
          : current,
      );
      setSelectedRootFileIds((current) =>
        current.filter((fileId) =>
          result.stages.some((stage) =>
            stage.files.some((file) => file.entity_file_id === fileId && isSelectableS3Root(file)),
          ),
        ),
      );
    } finally {
      setIsLoading(false);
    }
  }, [canQueryRuntime, workdirPath, workspaceId]);

  useEffect(() => {
    void loadExplorer();
  }, [loadExplorer]);

  useEffect(() => {
    if (!explorer) return;
    const firstS3Stage = explorer.stages.find(isS3CapableStage);
    if (!firstS3Stage) return;
    setManualS3Registration((current) => {
      if (current.stage_id || current.bucket) return current;
      return {
        ...current,
        stage_id: firstS3Stage.stage_id,
        bucket: bucketFromS3Uri(firstS3Stage.input_uri) ?? "",
      };
    });
  }, [explorer]);

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

  async function handleReconcileS3() {
    if (!workdirPath && !workspaceId) return;
    setActiveAction("s3-reconcile");
    setActionMessage(null);
    try {
      const result = workspaceId
        ? await reconcileS3WorkspaceById(workspaceId)
        : await reconcileS3Workspace(workdirPath as string);
      setErrors(result.errors);
      setS3Summary(result.summary);
      setActionMessage(
        result.summary
          ? `S3 reconciliation complete: ${result.summary.registered_file_count} registered, ${result.summary.updated_file_count} updated, ${result.summary.unmapped_object_count} unmapped.`
          : "S3 reconciliation finished with no summary.",
      );
      await loadExplorer();
    } finally {
      setActiveAction(null);
    }
  }

  async function handleManualS3Registration() {
    if (!workdirPath && !workspaceId) return;
    const size = manualS3Registration.size.trim()
      ? Number(manualS3Registration.size.trim())
      : null;
    if (size !== null && (!Number.isFinite(size) || size < 0)) {
      setActionMessage("S3 size must be an empty value or a non-negative number.");
      return;
    }
    const input: RegisterS3SourceArtifactRequest = {
      stage_id: manualS3Registration.stage_id.trim(),
      entity_id: manualS3Registration.entity_id.trim(),
      artifact_id: manualS3Registration.artifact_id.trim(),
      bucket: manualS3Registration.bucket.trim(),
      key: manualS3Registration.key.trim(),
      version_id: optionalText(manualS3Registration.version_id),
      etag: optionalText(manualS3Registration.etag),
      checksum_sha256: optionalText(manualS3Registration.checksum_sha256),
      size,
    };

    setActiveAction("s3-register");
    setActionMessage(null);
    try {
      const result = workspaceId
        ? await registerS3SourceArtifactById(workspaceId, input)
        : await registerS3SourceArtifact(workdirPath as string, input);
      setErrors(result.errors);
      setActionMessage(
        result.payload
          ? `S3 artifact registered: ${result.payload.file.entity_id} / ${result.payload.file.artifact_id ?? result.payload.file.key ?? "artifact"}.`
          : "S3 registration finished without a registered artifact.",
      );
      await loadExplorer();
    } finally {
      setActiveAction(null);
    }
  }

  async function handleRunSmallBatch() {
    if (!workdirPath && !workspaceId) return;
    setActiveAction("s3-batch");
    setActionMessage(null);
    try {
      const result = workspaceId
        ? await runDueTasksLimitedById(workspaceId, batchLimit)
        : await runDueTasksLimited(workdirPath as string, batchLimit);
      setErrors(result.errors);
      setActionMessage(
        result.summary
          ? `Small batch complete: ${result.summary.claimed} claimed, ${result.summary.succeeded} succeeded, ${result.summary.retry_scheduled} retry, ${result.summary.failed} failed, ${result.summary.blocked} blocked.`
          : "Small batch finished with no summary.",
      );
      await loadExplorer();
    } finally {
      setActiveAction(null);
    }
  }

  async function handleRunPipelineWaves() {
    if (!workdirPath && !workspaceId) return;
    setActiveAction("pipeline-waves");
    setActionMessage(null);
    try {
      const result = workspaceId
        ? await runPipelineWavesById(
            workspaceId,
            pipelineWaveControls.max_waves,
            pipelineWaveControls.max_tasks_per_wave,
            pipelineWaveControls.stop_on_first_failure,
          )
        : await runPipelineWaves(
            workdirPath as string,
            pipelineWaveControls.max_waves,
            pipelineWaveControls.max_tasks_per_wave,
            pipelineWaveControls.stop_on_first_failure,
          );
      setErrors([...(result.errors ?? []), ...(result.summary?.errors ?? [])]);
      setPipelineWaveSummary(result.summary);
      setActionMessage(
        result.summary
          ? `Pipeline waves complete: ${result.summary.waves_executed} wave(s), ${result.summary.total_claimed} claimed, stopped ${result.summary.stopped_reason}.`
          : "Pipeline waves finished with no summary.",
      );
      await loadExplorer();
    } finally {
      setActiveAction(null);
    }
  }

  async function handleRunSelectedPipelineWaves() {
    if (!workspaceId) {
      setActionMessage("Selected pipeline waves require a registered workspace route.");
      return;
    }
    if (selectedRootFileIds.length === 0) {
      setActionMessage("Select at least one pending S3 source artifact.");
      return;
    }
    setActiveAction("selected-pipeline-waves");
    setActionMessage(null);
    try {
      const result = await runSelectedPipelineWavesById(
        workspaceId,
        selectedRootFileIds,
        pipelineWaveControls.max_waves,
        pipelineWaveControls.max_tasks_per_wave,
        pipelineWaveControls.stop_on_first_failure,
      );
      setErrors([...(result.errors ?? []), ...(result.summary?.errors ?? [])]);
      setSelectedPipelineSummary(result.summary);
      setActionMessage(
        result.summary
          ? `Selected pipeline waves complete: ${result.summary.waves_executed} wave(s), ${result.summary.total_claimed} claimed, ${result.summary.output_tree.length} output(s), stopped ${result.summary.stopped_reason}.`
          : "Selected pipeline waves finished with no summary.",
      );
      await loadExplorer();
    } finally {
      setActiveAction(null);
    }
  }

  function handleToggleSelectedRoot(file: WorkspaceFileNode, checked: boolean) {
    if (!isSelectableS3Root(file)) return;
    setSelectedRootFileIds((current) => {
      if (checked) {
        if (current.includes(file.entity_file_id) || current.length >= 10) return current;
        return [...current, file.entity_file_id];
      }
      return current.filter((fileId) => fileId !== file.entity_file_id);
    });
  }

  async function handleCopyS3Uri(file: WorkspaceFileNode) {
    const uri = s3UriForFile(file);
    if (!uri) return;
    setActiveAction(`copy:${file.entity_file_id}`);
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(uri);
        setActionMessage(`Copied ${uri}`);
      } else {
        setActionMessage(uri);
      }
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
    const entityPath = workspaceId
      ? `/workspaces/${encodeURIComponent(workspaceId)}/entities/${encodeURIComponent(file.entity_id)}`
      : `/entities/${encodeURIComponent(file.entity_id)}`;
    navigate(`${entityPath}?file_id=${file.entity_file_id}`);
  }

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Filesystem</span>
          <h1>Workspace Explorer</h1>
          <span className="muted">
            {explorer?.workdir_path ?? workdirPath ?? "No workdir selected"}
            {workspaceId ? ` / workspace ${workspaceId}` : ""}
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
          <p className="empty-text">Open or initialize a valid workdir to inspect stage folders and artifacts.</p>
        </section>
      ) : isLoading && !explorer ? (
        <section className="panel">
          <p className="empty-text">Loading workspace explorer...</p>
        </section>
      ) : explorer ? (
        <>
          <ExplorerSummary explorer={explorer} />
          <S3OperatorPanel
            activeAction={activeAction}
            batchLimit={batchLimit}
            disabled={!canQueryRuntime || isLoading}
            form={manualS3Registration}
            pipelineWaveControls={pipelineWaveControls}
            pipelineWaveSummary={pipelineWaveSummary}
            selectedCount={selectedRootFileIds.length}
            selectedPipelineSummary={selectedPipelineSummary}
            stages={explorer.stages}
            summary={s3Summary}
            onBatchLimitChange={setBatchLimit}
            onClearSelection={() => {
              setSelectedRootFileIds([]);
              setSelectedPipelineSummary(null);
            }}
            onFormChange={setManualS3Registration}
            onPipelineWaveControlsChange={setPipelineWaveControls}
            onReconcile={() => void handleReconcileS3()}
            onRegister={() => void handleManualS3Registration()}
            onRunBatch={() => void handleRunSmallBatch()}
            onRunPipelineWaves={() => void handleRunPipelineWaves()}
            onRunSelectedPipelineWaves={() => void handleRunSelectedPipelineWaves()}
          />
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
                    selectedRootFileIds={selectedRootFileIds}
                    activeAction={activeAction}
                    onSelectFile={setSelectedFileId}
                    onToggleSelectedRoot={handleToggleSelectedRoot}
                    onOpenFile={(fileId) => void handleOpen("file", fileId)}
                    onOpenFolder={(fileId) => void handleOpen("folder", fileId)}
                    onCopyS3Uri={(file) => void handleCopyS3Uri(file)}
                    onGoToEntity={goToEntity}
                  />
                ))}
              </div>
              <TrailPanel
                trail={selectedTrail}
                selectedFile={selectedFile}
                activeAction={activeAction}
                workspaceId={workspaceId}
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

interface S3OperatorPanelProps {
  activeAction: string | null;
  batchLimit: number;
  disabled: boolean;
  form: ManualS3RegistrationForm;
  pipelineWaveControls: PipelineWaveControls;
  pipelineWaveSummary: RunPipelineWavesSummary | null;
  selectedCount: number;
  selectedPipelineSummary: RunSelectedPipelineWavesSummary | null;
  stages: WorkspaceStageTree[];
  summary: S3ReconciliationSummary | null;
  onBatchLimitChange: (value: number) => void;
  onClearSelection: () => void;
  onFormChange: (form: ManualS3RegistrationForm) => void;
  onPipelineWaveControlsChange: (controls: PipelineWaveControls) => void;
  onReconcile: () => void;
  onRegister: () => void;
  onRunBatch: () => void;
  onRunPipelineWaves: () => void;
  onRunSelectedPipelineWaves: () => void;
}

function S3OperatorPanel({
  activeAction,
  batchLimit,
  disabled,
  form,
  pipelineWaveControls,
  pipelineWaveSummary,
  selectedCount,
  selectedPipelineSummary,
  stages,
  summary,
  onBatchLimitChange,
  onClearSelection,
  onFormChange,
  onPipelineWaveControlsChange,
  onReconcile,
  onRegister,
  onRunBatch,
  onRunPipelineWaves,
  onRunSelectedPipelineWaves,
}: S3OperatorPanelProps) {
  const s3Stages = stages.filter(isS3CapableStage);
  const canRegister =
    !disabled &&
    !!form.stage_id.trim() &&
    !!form.entity_id.trim() &&
    !!form.artifact_id.trim() &&
    !!form.bucket.trim() &&
    !!form.key.trim();

  function updateField(key: keyof ManualS3RegistrationForm, value: string) {
    onFormChange({ ...form, [key]: value });
  }

  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h2>S3 Operator Console</h2>
          <span className="muted">{s3Stages.length} S3-capable stage(s)</span>
        </div>
        <div className="button-row">
          <button
            type="button"
            className="button secondary"
            disabled={disabled || activeAction === "s3-reconcile"}
            onClick={onReconcile}
          >
            {activeAction === "s3-reconcile" ? "Reconciling..." : "Reconcile S3"}
          </button>
          <label className="inline-field">
            Batch
            <input
              type="number"
              min={1}
              max={5}
              value={batchLimit}
              disabled={disabled || activeAction === "s3-batch"}
              onChange={(event) => onBatchLimitChange(Number(event.target.value))}
            />
          </label>
          <button
            type="button"
            className="button primary"
            disabled={disabled || activeAction === "s3-batch"}
            onClick={onRunBatch}
          >
            {activeAction === "s3-batch" ? "Running..." : "Run small batch"}
          </button>
        </div>
      </div>

      {summary ? (
        <div className="summary-card-grid">
          <SummaryCard label="Stages" value={summary.stage_count} />
          <SummaryCard label="Listed" value={summary.listed_object_count} />
          <SummaryCard label="Tagged" value={summary.metadata_tagged_count} />
          <SummaryCard label="Registered" value={summary.registered_file_count} />
          <SummaryCard label="Updated" value={summary.updated_file_count} />
          <SummaryCard label="Unchanged" value={summary.unchanged_file_count} />
          <SummaryCard label="Missing" value={summary.missing_file_count} />
          <SummaryCard label="Restored" value={summary.restored_file_count} />
          <SummaryCard label="Unmapped" value={summary.unmapped_object_count} />
          <SummaryCard label="Elapsed ms" value={summary.elapsed_ms} />
          <SummaryCard label="Latest" value={formatDateTime(summary.latest_reconciliation_at)} />
        </div>
      ) : null}

      <div className="stage-editor-form-grid">
        <div className="form-row">
          <label htmlFor="s3-register-stage">Stage</label>
          <select
            id="s3-register-stage"
            value={form.stage_id}
            disabled={disabled || activeAction === "s3-register"}
            onChange={(event) => {
              const stage = s3Stages.find((item) => item.stage_id === event.target.value);
              onFormChange({
                ...form,
                stage_id: event.target.value,
                bucket: form.bucket || bucketFromS3Uri(stage?.input_uri) || "",
              });
            }}
          >
            <option value="">Select stage</option>
            {s3Stages.map((stage) => (
              <option key={stage.stage_id} value={stage.stage_id}>
                {stage.stage_id}
              </option>
            ))}
          </select>
        </div>
        <S3RegistrationInput
          disabled={disabled || activeAction === "s3-register"}
          id="s3-register-entity"
          label="Entity ID"
          value={form.entity_id}
          onChange={(value) => updateField("entity_id", value)}
        />
        <S3RegistrationInput
          disabled={disabled || activeAction === "s3-register"}
          id="s3-register-artifact"
          label="Artifact ID"
          value={form.artifact_id}
          onChange={(value) => updateField("artifact_id", value)}
        />
        <S3RegistrationInput
          disabled={disabled || activeAction === "s3-register"}
          id="s3-register-bucket"
          label="Bucket"
          value={form.bucket}
          onChange={(value) => updateField("bucket", value)}
        />
        <S3RegistrationInput
          disabled={disabled || activeAction === "s3-register"}
          id="s3-register-key"
          label="Key"
          value={form.key}
          onChange={(value) => updateField("key", value)}
        />
        <S3RegistrationInput
          disabled={disabled || activeAction === "s3-register"}
          id="s3-register-version"
          label="Version ID"
          value={form.version_id}
          onChange={(value) => updateField("version_id", value)}
        />
        <S3RegistrationInput
          disabled={disabled || activeAction === "s3-register"}
          id="s3-register-etag"
          label="ETag"
          value={form.etag}
          onChange={(value) => updateField("etag", value)}
        />
        <S3RegistrationInput
          disabled={disabled || activeAction === "s3-register"}
          id="s3-register-checksum"
          label="SHA-256"
          value={form.checksum_sha256}
          onChange={(value) => updateField("checksum_sha256", value)}
        />
        <S3RegistrationInput
          disabled={disabled || activeAction === "s3-register"}
          id="s3-register-size"
          label="Size"
          type="number"
          value={form.size}
          onChange={(value) => updateField("size", value)}
        />
      </div>
      <div className="button-row">
        <button
          type="button"
          className="button secondary"
          disabled={!canRegister || activeAction === "s3-register"}
          onClick={onRegister}
        >
          {activeAction === "s3-register" ? "Registering..." : "Register S3 source"}
        </button>
      </div>

      <h3>Pipeline Waves</h3>
      <p className="muted">
        Selected pipeline waves run only selected roots and descendants. Small batch and pipeline waves can claim the broader due queue.
      </p>
      <div className="stage-editor-form-grid">
        <div className="form-row">
          <label htmlFor="pipeline-wave-max-waves">Max waves</label>
          <input
            id="pipeline-wave-max-waves"
            type="number"
            min={1}
            max={10}
            value={pipelineWaveControls.max_waves}
            disabled={disabled || activeAction === "pipeline-waves"}
            onChange={(event) =>
              onPipelineWaveControlsChange({
                ...pipelineWaveControls,
                max_waves: boundedNumber(event.target.value, 1, 10, 5),
              })
            }
          />
        </div>
        <div className="form-row">
          <label htmlFor="pipeline-wave-tasks">Tasks per wave</label>
          <input
            id="pipeline-wave-tasks"
            type="number"
            min={1}
            max={5}
            value={pipelineWaveControls.max_tasks_per_wave}
            disabled={disabled || activeAction === "pipeline-waves"}
            onChange={(event) =>
              onPipelineWaveControlsChange({
                ...pipelineWaveControls,
                max_tasks_per_wave: boundedNumber(event.target.value, 1, 5, 3),
              })
            }
          />
        </div>
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={pipelineWaveControls.stop_on_first_failure}
            disabled={disabled || activeAction === "pipeline-waves"}
            onChange={(event) =>
              onPipelineWaveControlsChange({
                ...pipelineWaveControls,
                stop_on_first_failure: event.target.checked,
              })
            }
          />
          Stop on first failure or blocked task
        </label>
      </div>
      <div className="button-row">
        <button
          type="button"
          className="button primary"
          disabled={disabled || selectedCount === 0 || activeAction === "selected-pipeline-waves"}
          onClick={onRunSelectedPipelineWaves}
        >
          {activeAction === "selected-pipeline-waves"
            ? "Running selected..."
            : `Run selected pipeline waves (${selectedCount})`}
        </button>
        <button
          type="button"
          className="button secondary"
          disabled={disabled || selectedCount === 0 || activeAction === "selected-pipeline-waves"}
          onClick={onClearSelection}
        >
          Clear selection
        </button>
        <button
          type="button"
          className="button secondary"
          disabled={disabled || activeAction === "pipeline-waves"}
          onClick={onRunPipelineWaves}
        >
          {activeAction === "pipeline-waves" ? "Running waves..." : "Run pipeline waves"}
        </button>
      </div>
      {selectedPipelineSummary ? <SelectedPipelineSummaryPanel summary={selectedPipelineSummary} /> : null}
      {pipelineWaveSummary ? (
        <div className="workspace-wave-summary">
          <div className="summary-card-grid">
            <SummaryCard label="Waves" value={pipelineWaveSummary.waves_executed} />
            <SummaryCard label="Claimed" value={pipelineWaveSummary.total_claimed} />
            <SummaryCard label="Succeeded" value={pipelineWaveSummary.total_succeeded} />
            <SummaryCard label="Retry" value={pipelineWaveSummary.total_retry_scheduled} />
            <SummaryCard label="Failed" value={pipelineWaveSummary.total_failed} />
            <SummaryCard label="Blocked" value={pipelineWaveSummary.total_blocked} />
            <SummaryCard label="Skipped" value={pipelineWaveSummary.total_skipped} />
            <SummaryCard label="Errors" value={pipelineWaveSummary.total_errors} />
            <SummaryCard label="Stopped" value={formatStoppedReason(pipelineWaveSummary.stopped_reason)} />
          </div>
          <div className="table-wrap">
            <table className="workspace-file-table">
              <thead>
                <tr>
                  <th>Wave</th>
                  <th>Claimed</th>
                  <th>Succeeded</th>
                  <th>Retry</th>
                  <th>Failed</th>
                  <th>Blocked</th>
                  <th>Skipped</th>
                  <th>Errors</th>
                </tr>
              </thead>
              <tbody>
                {pipelineWaveSummary.wave_summaries.map((wave) => (
                  <tr key={wave.wave_index}>
                    <td>{wave.wave_index}</td>
                    <td>{wave.summary.claimed}</td>
                    <td>{wave.summary.succeeded}</td>
                    <td>{wave.summary.retry_scheduled}</td>
                    <td>{wave.summary.failed}</td>
                    <td>{wave.summary.blocked}</td>
                    <td>{wave.summary.skipped}</td>
                    <td>{wave.summary.errors.length}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      ) : null}
    </section>
  );
}

function SelectedPipelineSummaryPanel({ summary }: { summary: RunSelectedPipelineWavesSummary }) {
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
        <SummaryCard label="Stopped" value={formatStoppedReason(summary.stopped_reason)} />
      </div>
      <div className="table-wrap">
        <table className="workspace-file-table">
          <thead>
            <tr>
              <th>Root</th>
              <th>Stage</th>
              <th>Status</th>
              <th>Runs</th>
              <th>Outputs</th>
              <th>S3</th>
            </tr>
          </thead>
          <tbody>
            {summary.root_results.map((root) => (
              <tr key={root.root_entity_file_id}>
                <td>
                  <div className="stacked-cell">
                    <strong>{root.entity_id}</strong>
                    <span className="muted">file #{root.root_entity_file_id}</span>
                    {root.artifact_id ? <span className="muted">artifact {root.artifact_id}</span> : null}
                  </div>
                </td>
                <td>{root.stage_id}</td>
                <td>
                  <div className="stacked-cell">
                    <StatusBadge status={root.status_before} />
                    {root.status_after ? <StatusBadge status={root.status_after} /> : null}
                  </div>
                </td>
                <td>{root.run_ids.length ? root.run_ids.join(", ") : "none"}</td>
                <td>{root.output_count}</td>
                <td>
                  <code>{root.s3_uri ?? root.key ?? "not available"}</code>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {summary.output_tree.length > 0 ? (
        <div className="table-wrap">
          <table className="workspace-file-table">
            <thead>
              <tr>
                <th>Output</th>
                <th>Target</th>
                <th>Status</th>
                <th>Relation</th>
                <th>S3</th>
                <th>Producer</th>
              </tr>
            </thead>
            <tbody>
              {summary.output_tree.map((output) => (
                <tr key={`${output.producer_run_id}-${output.entity_file_id}`}>
                  <td>
                    <div className="stacked-cell">
                      <strong>{output.entity_id}</strong>
                      <span className="muted">file #{output.entity_file_id}</span>
                      {output.artifact_id ? <span className="muted">artifact {output.artifact_id}</span> : null}
                    </div>
                  </td>
                  <td>{output.target_stage_id}</td>
                  <td>
                    <StatusBadge status={output.runtime_status ?? "pending"} />
                  </td>
                  <td>{output.relation_to_source ?? "not available"}</td>
                  <td>
                    <code>{output.s3_uri ?? output.key ?? "not available"}</code>
                  </td>
                  <td>
                    <div className="stacked-cell">
                      <span className="muted">root #{output.root_entity_file_id}</span>
                      <span className="muted">run {output.producer_run_id}</span>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : null}
    </div>
  );
}

function S3RegistrationInput({
  disabled,
  id,
  label,
  onChange,
  type = "text",
  value,
}: {
  disabled: boolean;
  id: string;
  label: string;
  onChange: (value: string) => void;
  type?: "text" | "number";
  value: string;
}) {
  return (
    <div className="form-row">
      <label htmlFor={id}>{label}</label>
      <input
        id={id}
        type={type}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
      />
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
  selectedRootFileIds: number[];
  activeAction: string | null;
  onSelectFile: (fileId: number) => void;
  onToggleSelectedRoot: (file: WorkspaceFileNode, checked: boolean) => void;
  onOpenFile: (fileId: number) => void;
  onOpenFolder: (fileId: number) => void;
  onCopyS3Uri: (file: WorkspaceFileNode) => void;
  onGoToEntity: (file: WorkspaceFileNode) => void;
}

function StageTreePanel({
  stage,
  selectedFileId,
  selectedRootFileIds,
  activeAction,
  onSelectFile,
  onToggleSelectedRoot,
  onOpenFile,
  onOpenFolder,
  onCopyS3Uri,
  onGoToEntity,
}: StageTreePanelProps) {
  return (
    <details className="panel workspace-stage-panel" open>
      <summary>
        <div>
          <strong>{stage.stage_id}</strong>
          <span className="muted">{stage.input_uri ?? stage.input_folder}</span>
        </div>
        <div className="button-row">
          <StatusBadge status={stage.is_active ? "active" : "inactive"} />
          <StatusBadge
            status={
              stage.storage_provider === "s3"
                ? "s3_input"
                : stage.folder_exists
                  ? "folder_ready"
                  : "folder_missing"
            }
          />
        </div>
      </summary>
      {!stage.is_active ? (
        <p className="muted">Inactive stage: historical files remain visible, but new files are not scanned here.</p>
      ) : null}
      <div className="inline-meta">
        <span>input {stage.input_uri ?? stage.folder_path}</span>
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
                    <th>Select</th>
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
                    const isS3 = file.storage_provider === "s3";
                    const s3Uri = s3UriForFile(file);
                    const selectable = isSelectableS3Root(file);
                    const selectedForRun = selectedRootFileIds.includes(file.entity_file_id);
                    return (
                      <tr
                        key={file.entity_file_id}
                        className={selectedFileId === file.entity_file_id ? "selected-row" : ""}
                      >
                        <td>
                          <input
                            type="checkbox"
                            checked={selectedForRun}
                            disabled={!selectable || (selectedRootFileIds.length >= 10 && !selectedForRun)}
                            onChange={(event) => onToggleSelectedRoot(file, event.target.checked)}
                            aria-label={`Select file ${file.entity_file_id} for selected pipeline run`}
                          />
                        </td>
                        <td>
                          <div className="stacked-cell">
                            <strong>{file.entity_id}</strong>
                            <span className="muted">file #{file.entity_file_id}</span>
                            <span className="muted">{isS3 ? "s3 pointer" : file.storage_provider}</span>
                            {file.artifact_id ? <span className="muted">artifact {file.artifact_id}</span> : null}
                            {file.relation_to_source ? <span className="muted">{file.relation_to_source}</span> : null}
                            {file.producer_run_id ? <span className="muted">run {file.producer_run_id}</span> : null}
                          </div>
                        </td>
                        <td>
                          <div className="stacked-cell">
                            <code>{file.file_path}</code>
                            {isS3 ? (
                              <>
                                <span className="muted">bucket {file.bucket ?? "unknown"}</span>
                                <span className="muted">key {file.key ?? "unknown"}</span>
                              </>
                            ) : null}
                          </div>
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
                          {isS3 ? (
                            <div className="stacked-cell">
                              <span>S3 pointer</span>
                              <span className="muted">
                                {file.file_exists ? "registered" : `missing since ${formatDateTime(file.missing_since)}`}
                              </span>
                            </div>
                          ) : file.file_exists ? (
                            "Present"
                          ) : (
                            `Missing since ${formatDateTime(file.missing_since)}`
                          )}
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
                          <div className="stacked-cell">
                            <code>{shortChecksum(file.checksum)}</code>
                            <span className="muted">{file.file_size} bytes</span>
                          </div>
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
                            {isS3 ? (
                              <button
                                type="button"
                                className="button secondary"
                                disabled={!s3Uri || activeAction === `copy:${file.entity_file_id}`}
                                onClick={() => onCopyS3Uri(file)}
                              >
                                Copy S3 URI
                              </button>
                            ) : null}
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
  workspaceId?: string | null;
  onOpenFile: (fileId: number) => void;
  onOpenFolder: (fileId: number) => void;
  onGoToEntity: (file: WorkspaceFileNode) => void;
}

function TrailPanel({
  trail,
  selectedFile,
  activeAction,
  workspaceId,
  onOpenFile,
  onOpenFolder,
  onGoToEntity,
}: TrailPanelProps) {
  const [outputs, setOutputs] = useState<StageRunOutputsPayload | null>(null);
  const [outputErrors, setOutputErrors] = useState<CommandErrorInfo[]>([]);
  const [isLoadingOutputs, setIsLoadingOutputs] = useState(false);

  useEffect(() => {
    setOutputs(null);
    setOutputErrors([]);
  }, [selectedFile?.entity_file_id]);

  async function loadProducerOutputs() {
    if (!workspaceId || !selectedFile?.producer_run_id) return;
    setIsLoadingOutputs(true);
    setOutputErrors([]);
    try {
      const result = await listStageRunOutputs(workspaceId, selectedFile.producer_run_id);
      setOutputs(result.payload);
      setOutputErrors(result.errors);
    } finally {
      setIsLoadingOutputs(false);
    }
  }

  useEffect(() => {
    if (!workspaceId || !selectedFile?.producer_run_id) return;
    void loadProducerOutputs();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspaceId, selectedFile?.producer_run_id]);

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
      {workspaceId && selectedFile.producer_run_id ? (
        <div className="lineage-output-panel">
          <div className="panel-heading">
            <div>
              <h3>Run Outputs</h3>
              <span className="muted">{selectedFile.producer_run_id}</span>
            </div>
            <button
              type="button"
              className="button secondary"
              disabled={isLoadingOutputs}
              onClick={() => void loadProducerOutputs()}
            >
              {isLoadingOutputs ? "Loading..." : "Load outputs"}
            </button>
          </div>
          {outputErrors.length > 0 ? (
            <p className="error-text">{outputErrors.map((error) => error.message).join(" ")}</p>
          ) : outputs ? (
            <div className="stage-run-output-list">
              <div className="inline-meta">
                <span>{outputs.output_count} output artifact(s)</span>
                <span>{outputs.run_id}</span>
              </div>
              {outputs.outputs.map((output) => (
                <article className="issue-row" key={output.entity_file_id}>
                  <StatusBadge status={output.runtime_status ?? "pending"} />
                  <div>
                    <strong>
                      {output.entity_id} / {output.target_stage_id}
                    </strong>
                    <p>{output.relation_to_source ?? "relation not available"}</p>
                    <code>{output.s3_uri ?? output.key ?? "S3 URI not available"}</code>
                    <p className="muted">
                      artifact {output.artifact_id ?? "not available"} / size {output.size ?? "?"}
                    </p>
                  </div>
                </article>
              ))}
            </div>
          ) : (
            <p className="empty-text">Load outputs to inspect all artifacts from this producer run.</p>
          )}
        </div>
      ) : null}
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
                    disabled={busy || !node.can_open_file}
                    onClick={() => onOpenFile(node.entity_file_id)}
                  >
                    File
                  </button>
                  <button
                    type="button"
                    className="button secondary"
                    disabled={busy || !node.can_open_folder}
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
  return [
    file.entity_id,
    file.file_name,
    file.file_path,
    file.stage_id,
    file.current_stage,
    file.next_stage,
    file.storage_provider,
    file.bucket,
    file.key,
    file.artifact_id,
    file.relation_to_source,
    file.producer_run_id,
  ]
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

function optionalText(value: string): string | null {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function boundedNumber(value: string, min: number, max: number, fallback: number): number {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return fallback;
  return Math.min(max, Math.max(min, parsed));
}

function formatStoppedReason(reason: string): string {
  return reason.replaceAll("_", " ");
}

function isS3CapableStage(stage: WorkspaceStageTree): boolean {
  return stage.storage_provider === "s3" || stage.input_uri?.startsWith("s3://") === true;
}

function isSelectableS3Root(file: WorkspaceFileNode): boolean {
  return (
    file.storage_provider === "s3" &&
    file.file_exists &&
    (file.runtime_status === "pending" || file.runtime_status === "retry_wait")
  );
}

function bucketFromS3Uri(inputUri?: string | null): string | null {
  if (!inputUri?.startsWith("s3://")) return null;
  const withoutScheme = inputUri.slice("s3://".length);
  return withoutScheme.split("/")[0] || null;
}

function s3UriForFile(file: WorkspaceFileNode): string | null {
  if (file.storage_provider !== "s3" || !file.bucket || !file.key) return null;
  return `s3://${file.bucket}/${file.key}`;
}
