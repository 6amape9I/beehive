import { useEffect, useState } from "react";
import { useParams } from "react-router-dom";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { InfoGrid } from "../components/InfoGrid";
import { StatusBadge } from "../components/StatusBadge";
import { ValidationIssues } from "../components/ValidationIssues";
import { formatDateTime, shortChecksum } from "../lib/formatters";
import { createNextStageCopy, getEntity, listStageRuns, runEntityStage } from "../lib/runtimeApi";
import type {
  CommandErrorInfo,
  EntityDetailPayload,
  FileCopyPayload,
  RunDueTasksSummary,
  StageRunRecord,
} from "../types/domain";

export function EntityDetailPage() {
  const { entityId } = useParams();
  const { state } = useBootstrap();
  const [detail, setDetail] = useState<EntityDetailPayload | null>(null);
  const [errors, setErrors] = useState<CommandErrorInfo[]>([]);
  const [copyResult, setCopyResult] = useState<FileCopyPayload | null>(null);
  const [runResult, setRunResult] = useState<RunDueTasksSummary | null>(null);
  const [stageRuns, setStageRuns] = useState<StageRunRecord[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [isCopyingStageId, setIsCopyingStageId] = useState<string | null>(null);
  const [isRunningStageId, setIsRunningStageId] = useState<string | null>(null);

  const workdirPath = state.selected_workdir_path;
  const canQueryRuntime = state.phase === "fully_initialized" && !!workdirPath && !!entityId;

  useEffect(() => {
    async function loadDetail() {
      if (!canQueryRuntime || !workdirPath || !entityId) {
        setDetail(null);
        setErrors([]);
        return;
      }

      setIsLoading(true);
      try {
        const result = await getEntity(workdirPath, entityId);
        const runsResult = await listStageRuns(workdirPath, entityId);
        setDetail(result.detail);
        setStageRuns(runsResult.runs);
        setErrors([...result.errors, ...runsResult.errors]);
      } finally {
        setIsLoading(false);
      }
    }

    void loadDetail();
  }, [canQueryRuntime, entityId, workdirPath]);

  async function handleCreateNextStageCopy(sourceStageId: string) {
    if (!workdirPath || !entityId) {
      return;
    }

    setIsCopyingStageId(sourceStageId);
    try {
      const result = await createNextStageCopy(workdirPath, entityId, sourceStageId);
      setCopyResult(result.payload);
      setErrors(result.errors);

      const refreshed = await getEntity(workdirPath, entityId);
      setDetail(refreshed.detail);
      setErrors((current) => [...current, ...refreshed.errors]);
    } finally {
      setIsCopyingStageId(null);
    }
  }

  async function handleRunStage(stageId: string) {
    if (!workdirPath || !entityId) {
      return;
    }

    setIsRunningStageId(stageId);
    try {
      const result = await runEntityStage(workdirPath, entityId, stageId);
      setRunResult(result.summary);
      setErrors([...result.errors, ...(result.summary?.errors ?? [])]);

      const [refreshed, runsResult] = await Promise.all([
        getEntity(workdirPath, entityId),
        listStageRuns(workdirPath, entityId),
      ]);
      setDetail(refreshed.detail);
      setStageRuns(runsResult.runs);
      setErrors((current) => [...current, ...refreshed.errors, ...runsResult.errors]);
    } finally {
      setIsRunningStageId(null);
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
          <section className="panel">
            <div className="panel-heading">
              <h2>{detail.entity.entity_id}</h2>
              <div className="button-row">
                <StatusBadge status={detail.entity.current_status} />
                <StatusBadge status={detail.entity.validation_status} />
              </div>
            </div>
            <InfoGrid
              items={[
                { label: "Current stage", value: detail.entity.current_stage_id },
                { label: "Latest file path", value: detail.entity.latest_file_path },
                { label: "Latest file id", value: detail.entity.latest_file_id },
                { label: "File count", value: detail.entity.file_count },
                { label: "First seen", value: formatDateTime(detail.entity.first_seen_at) },
                { label: "Last seen", value: formatDateTime(detail.entity.last_seen_at) },
                { label: "Updated at", value: formatDateTime(detail.entity.updated_at) },
              ]}
            />
          </section>
          {copyResult ? (
            <section className="panel">
              <div className="panel-heading">
                <h2>Last Managed Copy</h2>
                <StatusBadge status={copyResult.status} />
              </div>
              <InfoGrid
                items={[
                  { label: "Source stage", value: copyResult.source_stage_id },
                  { label: "Target stage", value: copyResult.target_stage_id },
                  { label: "Source file", value: copyResult.source_file_path },
                  { label: "Target file", value: copyResult.target_file_path },
                  { label: "Message", value: copyResult.message },
                ]}
              />
            </section>
          ) : null}
          {runResult ? (
            <section className="panel">
              <div className="panel-heading">
                <h2>Last Execution</h2>
                <span className="muted">Manual stage run</span>
              </div>
              <InfoGrid
                items={[
                  { label: "Claimed", value: runResult.claimed },
                  { label: "Succeeded", value: runResult.succeeded },
                  { label: "Retry scheduled", value: runResult.retry_scheduled },
                  { label: "Failed", value: runResult.failed },
                  { label: "Blocked", value: runResult.blocked },
                  { label: "Skipped", value: runResult.skipped },
                  { label: "Stuck reconciled", value: runResult.stuck_reconciled },
                ]}
              />
            </section>
          ) : null}
          <ValidationIssues
            title="Validation Issues"
            issues={detail.entity.validation_errors}
            emptyText="No validation issues recorded for this entity."
          />
          <section className="panel">
            <div className="panel-heading">
              <h2>JSON Preview</h2>
              <span className="muted">Read-only preview of the latest file instance</span>
            </div>
            <pre className="json-preview">{detail.latest_json_preview}</pre>
          </section>
          <section className="panel">
            <div className="panel-heading">
              <h2>File Instances</h2>
              <span className="muted">{detail.files.length} file record(s)</span>
            </div>
            {detail.files.length === 0 ? (
              <p className="empty-text">No file instances were recorded for this entity.</p>
            ) : (
              <div className="table-wrap">
                <table>
                  <thead>
                    <tr>
                      <th>Stage</th>
                      <th>Path</th>
                      <th>Status</th>
                      <th>Validation</th>
                      <th>Presence</th>
                      <th>Checksum</th>
                      <th>Size</th>
                      <th>Updated</th>
                      <th>Copy</th>
                    </tr>
                  </thead>
                  <tbody>
                    {detail.files.map((file) => (
                      <tr key={file.id}>
                        <td>{file.stage_id}</td>
                        <td>
                          <code>{file.file_path}</code>
                        </td>
                        <td>
                          <StatusBadge status={file.status} />
                        </td>
                        <td>
                          <StatusBadge status={file.validation_status} />
                        </td>
                        <td>{file.file_exists ? "Present" : `Missing since ${formatDateTime(file.missing_since)}`}</td>
                        <td>
                          <code>{shortChecksum(file.checksum)}</code>
                        </td>
                        <td>{file.file_size}</td>
                        <td>{formatDateTime(file.updated_at)}</td>
                        <td>
                          <button
                            type="button"
                            className="button secondary"
                            disabled={!file.file_exists || isCopyingStageId === file.stage_id}
                            onClick={() => void handleCreateNextStageCopy(file.stage_id)}
                          >
                            {isCopyingStageId === file.stage_id ? "Creating..." : "Create next-stage copy"}
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </section>
          <section className="panel">
            <div className="panel-heading">
              <h2>Stage State Rows</h2>
              <span className="muted">{detail.stage_states.length} record(s)</span>
            </div>
            {detail.stage_states.length === 0 ? (
              <p className="empty-text">No stage state rows were found for this entity.</p>
            ) : (
              <div className="table-wrap">
                <table>
                  <thead>
                    <tr>
                      <th>Stage</th>
                      <th>Status</th>
                      <th>Attempts</th>
                      <th>Max attempts</th>
                      <th>File path</th>
                      <th>Presence</th>
                      <th>Last error</th>
                      <th>Last HTTP</th>
                      <th>Next retry</th>
                      <th>Updated</th>
                      <th>Run</th>
                    </tr>
                  </thead>
                  <tbody>
                    {detail.stage_states.map((stageState) => (
                      <tr key={stageState.id}>
                        <td>{stageState.stage_id}</td>
                        <td>
                          <StatusBadge status={stageState.status} />
                        </td>
                        <td>{stageState.attempts}</td>
                        <td>{stageState.max_attempts}</td>
                        <td>
                          <code>{stageState.file_path}</code>
                        </td>
                        <td>{stageState.file_exists ? "Present" : "Missing"}</td>
                        <td>{stageState.last_error ?? "Not available"}</td>
                        <td>{stageState.last_http_status ?? "Not available"}</td>
                        <td>{formatDateTime(stageState.next_retry_at)}</td>
                        <td>{formatDateTime(stageState.updated_at)}</td>
                        <td>
                          <button
                            type="button"
                            className="button secondary"
                            disabled={
                              !stageState.file_exists ||
                              isRunningStageId === stageState.stage_id ||
                              !["pending", "retry_wait"].includes(stageState.status)
                            }
                            onClick={() => void handleRunStage(stageState.stage_id)}
                          >
                            {isRunningStageId === stageState.stage_id ? "Running..." : "Run this stage"}
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </section>
          <section className="panel">
            <div className="panel-heading">
              <h2>Stage Runs</h2>
              <span className="muted">{stageRuns.length} audit row(s)</span>
            </div>
            {stageRuns.length === 0 ? (
              <p className="empty-text">No execution attempts have been recorded for this entity.</p>
            ) : (
              <div className="table-wrap">
                <table>
                  <thead>
                    <tr>
                      <th>Run</th>
                      <th>Stage</th>
                      <th>Attempt</th>
                      <th>Success</th>
                      <th>HTTP</th>
                      <th>Error</th>
                      <th>Started</th>
                      <th>Duration</th>
                    </tr>
                  </thead>
                  <tbody>
                    {stageRuns.map((run) => (
                      <tr key={run.id}>
                        <td>
                          <code>{run.run_id}</code>
                        </td>
                        <td>{run.stage_id}</td>
                        <td>{run.attempt_no}</td>
                        <td>{run.success ? "Yes" : "No"}</td>
                        <td>{run.http_status ?? "Not available"}</td>
                        <td>{run.error_message ?? run.error_type ?? "None"}</td>
                        <td>{formatDateTime(run.started_at)}</td>
                        <td>{run.duration_ms ?? "Not available"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </section>
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
