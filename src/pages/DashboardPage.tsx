import { useBootstrap } from "../app/BootstrapContext";
import { ValidationIssues } from "../components/ValidationIssues";
import { BootstrapSummary } from "../features/bootstrap/BootstrapSummary";
import { WorkdirSetupPanel } from "../features/workdir/WorkdirSetupPanel";

export function DashboardPage() {
  const { state } = useBootstrap();

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Overview</span>
          <h1>Dashboard</h1>
        </div>
      </div>
      <WorkdirSetupPanel />
      <BootstrapSummary state={state} />
      <ValidationIssues
        title="Config Validation"
        issues={state.validation.issues}
        emptyText="No config validation issues."
      />
    </div>
  );
}
