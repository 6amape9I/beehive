import { useCallback, useEffect, useState } from "react";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { InfoGrid } from "../components/InfoGrid";
import { StatusBadge } from "../components/StatusBadge";
import { ValidationIssues } from "../components/ValidationIssues";
import { BootstrapSummary } from "../features/bootstrap/BootstrapSummary";
import { WorkdirSetupPanel } from "../features/workdir/WorkdirSetupPanel";
import { formatDateTime } from "../lib/formatters";
import { getRuntimeSummary, listAppEvents, listStages } from "../lib/runtimeApi";
import type { AppEventRecord, CommandErrorInfo, RuntimeSummary, StageRecord } from "../types/domain";

export function SettingsDiagnosticsPage() {
  const { state } = useBootstrap();
  const [summary, setSummary] = useState<RuntimeSummary | null>(null);
  const [stages, setStages] = useState<StageRecord[]>([]);
  const [events, setEvents] = useState<AppEventRecord[]>([]);
  const [errors, setErrors] = useState<CommandErrorInfo[]>([]);

  const workdirPath = state.selected_workdir_path;
  const canQueryRuntime = state.phase === "fully_initialized" && !!workdirPath;

  const loadDiagnostics = useCallback(async () => {
    if (!canQueryRuntime || !workdirPath) {
      setSummary(null);
      setStages([]);
      setEvents([]);
      setErrors([]);
      return;
    }

    const [summaryResult, stagesResult, eventsResult] = await Promise.all([
      getRuntimeSummary(workdirPath),
      listStages(workdirPath),
      listAppEvents(workdirPath, 20),
    ]);

    setSummary(summaryResult.summary);
    setStages(stagesResult.stages);
    setEvents(eventsResult.events);
    setErrors([...summaryResult.errors, ...stagesResult.errors, ...eventsResult.errors]);
  }, [canQueryRuntime, workdirPath]);

  useEffect(() => {
    void loadDiagnostics();
  }, [loadDiagnostics]);

  const activeStages = stages.filter((stage) => stage.is_active);
  const inactiveStages = stages.filter((stage) => !stage.is_active);

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Technical state</span>
          <h1>Settings / Diagnostics</h1>
        </div>
      </div>
      <WorkdirSetupPanel />
      <BootstrapSummary state={state} />
      <CommandErrorsPanel title="Diagnostics Query Errors" errors={errors} />
      <section className="panel">
        <div className="panel-heading">
          <h2>Workdir Health</h2>
          <span className="muted">Filesystem and runtime checks</span>
        </div>
        <InfoGrid
          items={[
            { label: "Workdir exists", value: state.workdir_state?.exists ? "Yes" : "No" },
            {
              label: "pipeline.yaml exists",
              value: state.workdir_state?.pipeline_config_exists ? "Yes" : "No",
            },
            { label: "app.db exists", value: state.workdir_state?.database_exists ? "Yes" : "No" },
            {
              label: "stages/ exists",
              value: state.workdir_state?.stages_dir_exists ? "Yes" : "No",
            },
            { label: "logs/ exists", value: state.workdir_state?.logs_dir_exists ? "Yes" : "No" },
            {
              label: "SQLite schema version",
              value: summary?.schema_version ?? state.database_state?.schema_version,
            },
            { label: "Active stages", value: summary?.active_stage_count },
            { label: "Inactive stages", value: summary?.inactive_stage_count },
            { label: "Present files", value: summary?.present_file_count },
            { label: "Missing files", value: summary?.missing_file_count },
            { label: "Managed copies", value: summary?.managed_copy_count },
            { label: "Invalid files", value: summary?.invalid_file_count },
            { label: "Last reconciliation", value: formatDateTime(summary?.last_reconciliation_at) },
          ]}
        />
      </section>
      <section className="panel">
        <div className="panel-heading">
          <h2>Stage Lifecycle</h2>
          <span className="muted">
            {activeStages.length} active / {inactiveStages.length} inactive
          </span>
        </div>
        <div className="stage-chip-row">
          {stages.length === 0 ? (
            <span className="muted">No stages available for this workdir.</span>
          ) : (
            stages.map((stage) => (
              <span className="stage-chip" key={stage.id}>
                {stage.id} ({stage.is_active ? "active" : "inactive"})
              </span>
            ))
          )}
        </div>
      </section>
      <section className="panel">
        <div className="panel-heading">
          <h2>Recent App Events</h2>
          <span className="muted">{events.length} event(s)</span>
        </div>
        {events.length === 0 ? (
          <p className="empty-text">No app events have been recorded yet.</p>
        ) : (
          <div className="issue-list">
            {events.map((event) => (
              <article className="issue-row" key={event.id}>
                <StatusBadge status={event.level} />
                <div>
                  <strong>{event.code}</strong>
                  <p>{event.message}</p>
                  <p className="muted">{formatDateTime(event.created_at)}</p>
                  {event.context ? (
                    <pre className="json-preview compact-json">
                      {JSON.stringify(event.context, null, 2)}
                    </pre>
                  ) : null}
                </div>
              </article>
            ))}
          </div>
        )}
      </section>
      <ValidationIssues
        title="Workdir Issues"
        issues={state.workdir_state?.health_issues ?? []}
        emptyText="No workdir health issues."
      />
      <ValidationIssues
        title="Config Validation"
        issues={state.validation.issues}
        emptyText="No config validation issues."
      />
    </div>
  );
}
