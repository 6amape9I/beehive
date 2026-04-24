import { useBootstrap } from "../app/BootstrapContext";
import { InfoGrid } from "../components/InfoGrid";
import { ValidationIssues } from "../components/ValidationIssues";
import { BootstrapSummary } from "../features/bootstrap/BootstrapSummary";
import { WorkdirSetupPanel } from "../features/workdir/WorkdirSetupPanel";

export function SettingsDiagnosticsPage() {
  const { state } = useBootstrap();

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
      <section className="panel">
        <div className="panel-heading">
          <h2>Workdir Health</h2>
          <span className="muted">Filesystem checks</span>
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
              value: state.database_state?.schema_version,
            },
          ]}
        />
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
