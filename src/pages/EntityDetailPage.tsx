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
  getWorkspaceEntity,
  openEntityFile,
  openEntityFolder,
  resetEntityStageToPending,
  resetWorkspaceEntityStageToPending,
  retryEntityStageNow,
  runEntityStage,
  saveEntityFileBusinessJson,
  skipEntityStage,
  viewWorkspaceEntityFileS3Json,
} from "../lib/runtimeApi";
import type {
  CommandErrorInfo,
  EntityDetailPayload,
  EntityFileS3JsonPayload,
  RunDueTasksSummary,
} from "../types/domain";

const S3_JSON_PREVIEW_LIMIT = 128 * 1024;

interface S3JsonModalState {
  payload: EntityFileS3JsonPayload;
  jsonText: string;
  previewText: string;
  isTruncated: boolean;
}

export function EntityDetailPage() {
  const { entityId, workspaceId: routeWorkspaceId } = useParams();
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
  const [s3JsonModal, setS3JsonModal] = useState<S3JsonModalState | null>(null);
  const [resetModal, setResetModal] = useState<{ stageId: string; reason: string } | null>(null);

  const workdirPath = state.selected_workdir_path;
  const workspaceId = routeWorkspaceId ?? state.selected_workspace_id;
  const canQueryRuntime = !!entityId && Boolean(workspaceId || (state.phase === "fully_initialized" && workdirPath));
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
      if (!canQueryRuntime || !entityId) {
        setDetail(null);
        setErrors([]);
        return;
      }

      setIsLoading(true);
      try {
        const result = workspaceId
          ? await getWorkspaceEntity(workspaceId, entityId)
          : await getEntity(workdirPath ?? "", entityId, fileId);
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
    [canQueryRuntime, entityId, workdirPath, workspaceId],
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
    reason?: string,
  ) {
    if (!entityId) return;
    if (action !== "reset" && !workdirPath) {
      setErrors([
        {
          code: "manual_action_unavailable",
          message: "This manual action is only available in local workdir mode.",
          path: null,
        },
      ]);
      return;
    }
    if (action === "reset" && !workdirPath && !workspaceId) {
      setErrors([
        {
          code: "manual_reset_unavailable",
          message: "Reset to pending requires an open workspace.",
          path: null,
        },
      ]);
      return;
    }

    setLoadingAction(`${action}:${stageId}`);
    setActionMessage(null);
    const localWorkdirPath = workdirPath ?? "";
    try {
      if (action === "run") {
        const result = await runEntityStage(localWorkdirPath, entityId, stageId);
        setRunResult(result.summary);
        setErrors([...result.errors, ...(result.summary?.errors ?? [])]);
        await loadDetail(selectedFileId);
      } else {
        const result =
          action === "retry"
            ? await retryEntityStageNow(localWorkdirPath, entityId, stageId)
            : action === "reset"
              ? workspaceId
                ? await resetWorkspaceEntityStageToPending(workspaceId, entityId, stageId, {
                    confirm: true,
                    reason: reason?.trim() || null,
                  })
                : await resetEntityStageToPending(
                    localWorkdirPath,
                    entityId,
                    stageId,
                    reason?.trim() || null,
                  )
              : await skipEntityStage(localWorkdirPath, entityId, stageId);
        await refreshAfterDetailResult(result.detail, [
          ...result.errors,
          ...(result.summary?.errors ?? []),
        ]);
        setRunResult(result.summary);
      }
      setActionMessage(
        action === "reset" ? "State reset to pending." : `${action} completed for stage ${stageId}.`,
      );
    } catch (error) {
      setErrors([
        {
          code: "manual_action_failed",
          message: error instanceof Error ? error.message : "Manual action failed.",
          path: null,
        },
      ]);
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

  async function handleViewS3Json(fileId: number) {
    if (!workspaceId) {
      setErrors([
        {
          code: "s3_json_workspace_required",
          message: "View S3 JSON requires a workspace-backed entity file.",
          path: null,
        },
      ]);
      return;
    }
    setLoadingFileAction(`s3:${fileId}`);
    setActionMessage(null);
    try {
      const result = await viewWorkspaceEntityFileS3Json(workspaceId, fileId);
      setErrors(result.errors);
      if (!result.payload) return;
      const jsonText = JSON.stringify(result.payload.json, null, 2);
      const isTruncated = jsonText.length > S3_JSON_PREVIEW_LIMIT;
      setS3JsonModal({
        payload: result.payload,
        jsonText,
        previewText: isTruncated
          ? `${jsonText.slice(0, S3_JSON_PREVIEW_LIMIT)}\n...`
          : jsonText,
        isTruncated,
      });
    } catch (error) {
      setErrors([
        {
          code: "s3_json_view_failed",
          message: error instanceof Error ? error.message : "Failed to load S3 JSON.",
          path: null,
        },
      ]);
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

  async function copyModalText(value: string, label: string) {
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error("Clipboard API is not available.");
      }
      await navigator.clipboard.writeText(value);
      setActionMessage(`${label} copied.`);
    } catch (error) {
      setErrors([
        {
          code: "clipboard_write_failed",
          message: error instanceof Error ? error.message : "Clipboard write failed.",
          path: null,
        },
      ]);
    }
  }

  async function confirmResetToPending() {
    if (!resetModal) return;
    const { stageId, reason } = resetModal;
    await handleManualAction("reset", stageId, reason);
    setResetModal(null);
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
            Open a workspace and select an entity from the Entities table.
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
          {workdirPath || workspaceId ? (
            <ManualActionsPanel
              stageStates={detail.stage_states}
              allowedActions={detail.allowed_actions}
              loadingAction={loadingAction}
              canRetry={Boolean(workdirPath)}
              canReset={Boolean(workdirPath || workspaceId)}
              canSkip={Boolean(workdirPath)}
              canRun={Boolean(workdirPath)}
              onRetry={(stageId) => void handleManualAction("retry", stageId)}
              onReset={(stageId) => setResetModal({ stageId, reason: "" })}
              onSkip={(stageId) => void handleManualAction("skip", stageId)}
              onRun={(stageId) => void handleManualAction("run", stageId)}
            />
          ) : null}
          <EntityTimeline timeline={detail.timeline} />
          <ValidationIssues
            title="Validation Issues"
            issues={detail.entity.validation_errors}
            emptyText="No validation issues recorded for this entity."
          />
          <EntityFileInstances
            files={detail.files}
            stageStates={detail.stage_states}
            fileAllowedActions={detail.file_allowed_actions}
            selectedFileId={selectedFile?.id ?? null}
            loadingFileAction={loadingFileAction}
            onSelectFile={(fileId) => void handleSelectFile(fileId)}
            onOpenFile={(fileId) => void handleOpenFile(fileId)}
            onOpenFolder={(fileId) => void handleOpenFolder(fileId)}
            onViewS3Json={(fileId) => void handleViewS3Json(fileId)}
          />
          <EntityJsonPanel
            selectedFile={selectedFile}
            selectedJson={detail.selected_file_json}
            selectedFileActions={selectedFileActions}
            isSaving={isSavingJson}
            onSave={handleSaveJson}
          />
          <StageRunsPanel runs={detail.stage_runs} workspaceId={workspaceId} />
        </>
      ) : (
        <section className="panel">
          <h2>{entityId ?? "No entity selected"}</h2>
          <p className="empty-text">Entity detail is not available for the selected ID.</p>
        </section>
      )}

      {s3JsonModal ? (
        <div className="modal-backdrop" role="presentation">
          <section className="modal-panel" role="dialog" aria-modal="true" aria-labelledby="s3-json-title">
            <div className="panel-heading">
              <div>
                <h2 id="s3-json-title">S3 JSON</h2>
                <span className="muted">{s3JsonModal.payload.s3_uri}</span>
              </div>
              <button
                type="button"
                className="button secondary"
                onClick={() => setS3JsonModal(null)}
              >
                Close
              </button>
            </div>
            {s3JsonModal.isTruncated ? (
              <p className="muted">
                Large JSON preview; showing first {S3_JSON_PREVIEW_LIMIT} characters.
              </p>
            ) : null}
            <pre className="json-preview modal-json-preview">{s3JsonModal.previewText}</pre>
            <div className="button-row">
              <button
                type="button"
                className="button primary"
                onClick={() => void copyModalText(s3JsonModal.jsonText, "JSON")}
              >
                Copy JSON
              </button>
              <button
                type="button"
                className="button secondary"
                onClick={() => void copyModalText(s3JsonModal.payload.s3_uri, "S3 URI")}
              >
                Copy S3 URI
              </button>
            </div>
          </section>
        </div>
      ) : null}

      {resetModal ? (
        <div className="modal-backdrop" role="presentation">
          <section className="modal-panel compact-modal" role="dialog" aria-modal="true" aria-labelledby="reset-title">
            <div className="panel-heading">
              <div>
                <h2 id="reset-title">Reset to pending</h2>
                <span className="muted">Stage {resetModal.stageId}</span>
              </div>
            </div>
            <p className="empty-text">
              This operation will not delete history or S3 files. It resets attempts to 0 and
              lets workers claim the task again.
            </p>
            <div className="form-row">
              <label htmlFor="reset-reason">Reason</label>
              <textarea
                id="reset-reason"
                className="compact-textarea"
                value={resetModal.reason}
                onChange={(event) =>
                  setResetModal({ ...resetModal, reason: event.target.value })
                }
                placeholder="Optional"
              />
            </div>
            <div className="button-row">
              <button
                type="button"
                className="button primary"
                disabled={loadingAction === `reset:${resetModal.stageId}`}
                onClick={() => void confirmResetToPending()}
              >
                Confirm
              </button>
              <button
                type="button"
                className="button secondary"
                disabled={loadingAction === `reset:${resetModal.stageId}`}
                onClick={() => setResetModal(null)}
              >
                Cancel
              </button>
            </div>
          </section>
        </div>
      ) : null}
    </div>
  );
}
