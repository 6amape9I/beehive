import { useCallback, useEffect, useMemo, useState } from "react";
import { useParams, useSearchParams } from "react-router-dom";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { EntityFileInstances } from "../components/entity-detail/EntityFileInstances";
import { EntityHeader } from "../components/entity-detail/EntityHeader";
import { EntityJsonPanel } from "../components/entity-detail/EntityJsonPanel";
import { EntityTimeline } from "../components/entity-detail/EntityTimeline";
import { ManualActionsPanel } from "../components/entity-detail/ManualActionsPanel";
import { StageRunsPanel } from "../components/entity-detail/StageRunsPanel";
import { ValidationIssues } from "../components/ValidationIssues";
import {
  getEntity,
  openEntityFile,
  openEntityFolder,
  resetEntityStageToPending,
  retryEntityStageNow,
  runEntityStage,
  saveEntityFileBusinessJson,
  skipEntityStage,
} from "../lib/runtimeApi";
import type {
  CommandErrorInfo,
  EntityDetailPayload,
  RunDueTasksSummary,
} from "../types/domain";

export function EntityDetailPage() {
  const { entityId } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const { state } = useBootstrap();
  const [detail, setDetail] = useState<EntityDetailPayload | null>(null);
  const [selectedFileId, setSelectedFileId] = useState<number | null>(null);
  const [errors, setErrors] = useState<CommandErrorInfo[]>([]);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [runResult, setRunResult] = useState<RunDueTasksSummary | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [loadingAction, setLoadingAction] = useState<string | null>(null);
  const [loadingFileAction, setLoadingFileAction] = useState<string | null>(null);
  const [isSavingJson, setIsSavingJson] = useState(false);

  const workdirPath = state.selected_workdir_path;
  const canQueryRuntime = state.phase === "fully_initialized" && !!workdirPath && !!entityId;
  const queryFileId = useMemo(() => {
    const value = searchParams.get("file_id");
    if (!value) return null;
    const parsed = Number(value);
    return Number.isInteger(parsed) && parsed > 0 ? parsed : null;
  }, [searchParams]);

  const selectedFile = useMemo(() => {
    if (!detail) return null;
    return (
      detail.files.find((file) => file.id === selectedFileId) ??
      detail.files.find((file) => file.id === detail.entity.latest_file_id) ??
      detail.files[0] ??
      null
    );
  }, [detail, selectedFileId]);
  const selectedFileActions = useMemo(() => {
    if (!detail || !selectedFile) return null;
    return (
      detail.file_allowed_actions.find(
        (actions) => actions.entity_file_id === selectedFile.id,
      ) ?? null
    );
  }, [detail, selectedFile]);

  const loadDetail = useCallback(
    async (fileId: number | null = null) => {
      if (!canQueryRuntime || !workdirPath || !entityId) {
        setDetail(null);
        setErrors([]);
        return;
      }

      setIsLoading(true);
      try {
        const result = await getEntity(workdirPath, entityId, fileId);
        setDetail(result.detail);
        setErrors(result.errors);
        if (result.detail) {
          const requestedFileIsValid =
            fileId !== null && result.detail.files.some((file) => file.id === fileId);
          setSelectedFileId(
            requestedFileIsValid
              ? fileId
              : result.detail.entity.latest_file_id ?? result.detail.files[0]?.id ?? null,
          );
        }
      } finally {
        setIsLoading(false);
      }
    },
    [canQueryRuntime, entityId, workdirPath],
  );

  useEffect(() => {
    void loadDetail(queryFileId);
  }, [loadDetail, queryFileId]);

  async function refreshAfterDetailResult(nextDetail: EntityDetailPayload | null, nextErrors: CommandErrorInfo[]) {
    setDetail(nextDetail);
    setErrors(nextErrors);
    if (nextDetail && selectedFileId === null) {
      setSelectedFileId(nextDetail.entity.latest_file_id ?? nextDetail.files[0]?.id ?? null);
    }
  }

  async function handleManualAction(
    action: "retry" | "reset" | "skip" | "run",
    stageId: string,
  ) {
    if (!workdirPath || !entityId) return;

    setLoadingAction(`${action}:${stageId}`);
    setActionMessage(null);
    try {
      if (action === "run") {
        const result = await runEntityStage(workdirPath, entityId, stageId);
        setRunResult(result.summary);
        setErrors([...result.errors, ...(result.summary?.errors ?? [])]);
        await loadDetail(selectedFileId);
      } else {
        const result =
          action === "retry"
            ? await retryEntityStageNow(workdirPath, entityId, stageId)
            : action === "reset"
              ? await resetEntityStageToPending(workdirPath, entityId, stageId)
              : await skipEntityStage(workdirPath, entityId, stageId);
        await refreshAfterDetailResult(result.detail, [
          ...result.errors,
          ...(result.summary?.errors ?? []),
        ]);
        setRunResult(result.summary);
      }
      setActionMessage(`${action} completed for stage ${stageId}.`);
    } finally {
      setLoadingAction(null);
    }
  }

  async function handleSelectFile(fileId: number) {
    setSelectedFileId(fileId);
    setSearchParams((current) => {
      const next = new URLSearchParams(current);
      next.set("file_id", String(fileId));
      return next;
    });
    await loadDetail(fileId);
  }

  async function handleOpenFile(fileId: number) {
    if (!workdirPath) return;
    setLoadingFileAction(`file:${fileId}`);
    try {
      const result = await openEntityFile(workdirPath, fileId);
      setErrors(result.errors);
      setActionMessage(result.payload ? `Opened ${result.payload.opened_path}` : null);
    } finally {
      setLoadingFileAction(null);
    }
  }

  async function handleOpenFolder(fileId: number) {
    if (!workdirPath) return;
    setLoadingFileAction(`folder:${fileId}`);
    try {
      const result = await openEntityFolder(workdirPath, fileId);
      setErrors(result.errors);
      setActionMessage(result.payload ? `Opened ${result.payload.opened_path}` : null);
    } finally {
      setLoadingFileAction(null);
    }
  }

  async function handleSaveJson(payloadJson: string, metaJson: string, comment: string) {
    if (!workdirPath || !selectedFile) return;
    setIsSavingJson(true);
    try {
      const result = await saveEntityFileBusinessJson(
        workdirPath,
        selectedFile.id,
        payloadJson,
        metaJson,
        comment || null,
      );
      setDetail(result.detail);
      setErrors(result.errors);
      setActionMessage(result.errors.length === 0 ? "JSON payload/meta saved." : null);
    } finally {
      setIsSavingJson(false);
    }
  }

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Runtime</span>
          <h1>Entity Detail</h1>
        </div>
      </div>

      <CommandErrorsPanel title="Entity Detail Errors" errors={errors} />
      {actionMessage ? <section className="compact-panel panel">{actionMessage}</section> : null}

      {!canQueryRuntime ? (
        <section className="panel">
          <h2>{entityId ?? "No entity selected"}</h2>
          <p className="empty-text">
            Open a fully initialized workdir and select an entity from the Entities table.
          </p>
        </section>
      ) : isLoading ? (
        <section className="panel">
          <p className="empty-text">Loading entity detail...</p>
        </section>
      ) : detail ? (
        <>
          <EntityHeader
            entity={detail.entity}
            isRefreshing={isLoading}
            onRefresh={() => void loadDetail(selectedFileId)}
          />
          {runResult ? (
            <section className="panel">
              <div className="panel-heading">
                <h2>Last Manual Execution</h2>
                <span className="muted">Summary from backend command</span>
              </div>
              <div className="inline-meta">
                <span>claimed {runResult.claimed}</span>
                <span>succeeded {runResult.succeeded}</span>
                <span>retry {runResult.retry_scheduled}</span>
                <span>failed {runResult.failed}</span>
                <span>blocked {runResult.blocked}</span>
                <span>skipped {runResult.skipped}</span>
              </div>
            </section>
          ) : null}
          <ManualActionsPanel
            stageStates={detail.stage_states}
            allowedActions={detail.allowed_actions}
            loadingAction={loadingAction}
            onRetry={(stageId) => void handleManualAction("retry", stageId)}
            onReset={(stageId) => void handleManualAction("reset", stageId)}
            onSkip={(stageId) => void handleManualAction("skip", stageId)}
            onRun={(stageId) => void handleManualAction("run", stageId)}
          />
          <EntityTimeline timeline={detail.timeline} />
          <ValidationIssues
            title="Validation Issues"
            issues={detail.entity.validation_errors}
            emptyText="No validation issues recorded for this entity."
          />
          <EntityFileInstances
            files={detail.files}
            fileAllowedActions={detail.file_allowed_actions}
            selectedFileId={selectedFile?.id ?? null}
            loadingFileAction={loadingFileAction}
            onSelectFile={(fileId) => void handleSelectFile(fileId)}
            onOpenFile={(fileId) => void handleOpenFile(fileId)}
            onOpenFolder={(fileId) => void handleOpenFolder(fileId)}
          />
          <EntityJsonPanel
            selectedFile={selectedFile}
            selectedJson={detail.selected_file_json}
            selectedFileActions={selectedFileActions}
            isSaving={isSavingJson}
            onSave={handleSaveJson}
          />
          <StageRunsPanel runs={detail.stage_runs} />
        </>
      ) : (
        <section className="panel">
          <h2>{entityId ?? "No entity selected"}</h2>
          <p className="empty-text">Entity detail is not available for the selected ID.</p>
        </section>
      )}
    </div>
  );
}
