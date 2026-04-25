import { useCallback, useEffect, useState } from "react";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { InfoGrid } from "../components/InfoGrid";
import { ValidationIssues } from "../components/ValidationIssues";
import { ActiveTasksPanel } from "../components/dashboard/ActiveTasksPanel";
import { DashboardActions } from "../components/dashboard/DashboardActions";
import { LastErrorsPanel } from "../components/dashboard/LastErrorsPanel";
import { RecentRunsPanel } from "../components/dashboard/RecentRunsPanel";
import { StageCountersTable } from "../components/dashboard/StageCountersTable";
import { StageGraph } from "../components/dashboard/StageGraph";
import { SummaryCards } from "../components/dashboard/SummaryCards";
import { BootstrapSummary } from "../features/bootstrap/BootstrapSummary";
import { WorkdirSetupPanel } from "../features/workdir/WorkdirSetupPanel";
import { formatDateTime } from "../lib/formatters";
import {
  getDashboardOverview,
  reconcileStuckTasks,
  runDueTasks,
  scanWorkspace,
} from "../lib/runtimeApi";
import type { CommandErrorInfo, DashboardOverview } from "../types/domain";

type DashboardAction = "refresh" | "scan" | "run" | "reconcile";

function commandError(code: string, message: string): CommandErrorInfo {
  return { code, message, path: null };
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export function DashboardPage() {
  const { state } = useBootstrap();
  const [overview, setOverview] = useState<DashboardOverview | null>(null);
  const [dashboardErrors, setDashboardErrors] = useState<CommandErrorInfo[]>([]);
  const [activeAction, setActiveAction] = useState<DashboardAction | null>(null);
  const [isLoadingOverview, setIsLoadingOverview] = useState(false);
  const [actionMessage, setActionMessage] = useState<string | null>(null);

  const workdirPath = state.selected_workdir_path;
  const canQueryDashboard = state.phase === "fully_initialized" && !!workdirPath;

  const loadOverview = useCallback(async () => {
    if (!canQueryDashboard || !workdirPath) {
      setOverview(null);
      setDashboardErrors([]);
      return;
    }

    setIsLoadingOverview(true);
    try {
      const result = await getDashboardOverview(workdirPath);
      setOverview(result.overview);
      setDashboardErrors(result.errors);
    } catch (error) {
      setOverview(null);
      setDashboardErrors([
        commandError("get_dashboard_overview_rejected", errorMessage(error)),
      ]);
    } finally {
      setIsLoadingOverview(false);
    }
  }, [canQueryDashboard, workdirPath]);

  useEffect(() => {
    void loadOverview();
  }, [loadOverview]);

  async function runDashboardAction(
    action: DashboardAction,
    label: string,
    operation: () => Promise<CommandErrorInfo[]>,
  ) {
    if (!workdirPath) {
      return;
    }

    setActiveAction(action);
    setActionMessage(null);
    try {
      const errors = await operation();
      await loadOverview();
      if (errors.length > 0) {
        setDashboardErrors(errors);
        setActionMessage(`${label} completed with ${errors.length} error(s).`);
      } else {
        setActionMessage(`${label} completed.`);
      }
    } catch (error) {
      setDashboardErrors([commandError(`${action}_rejected`, errorMessage(error))]);
      setActionMessage(`${label} failed.`);
    } finally {
      setActiveAction(null);
    }
  }

  function handleRefresh() {
    void runDashboardAction("refresh", "Refresh", async () => {
      await loadOverview();
      return [];
    });
  }

  function handleScanWorkspace() {
    void runDashboardAction("scan", "Scan workspace", async () => {
      const result = await scanWorkspace(workdirPath ?? "");
      return result.errors;
    });
  }

  function handleRunDueTasks() {
    void runDashboardAction("run", "Run due tasks", async () => {
      const result = await runDueTasks(workdirPath ?? "");
      return [...result.errors, ...(result.summary?.errors ?? [])];
    });
  }

  function handleReconcileStuck() {
    void runDashboardAction("reconcile", "Reconcile stuck", async () => {
      const result = await reconcileStuckTasks(workdirPath ?? "");
      return result.errors;
    });
  }

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Overview</span>
          <h1>Dashboard</h1>
        </div>
        <DashboardActions
          canRun={canQueryDashboard}
          activeAction={activeAction}
          onRefresh={handleRefresh}
          onScan={handleScanWorkspace}
          onRunDue={handleRunDueTasks}
          onReconcile={handleReconcileStuck}
        />
      </div>

      <WorkdirSetupPanel />
      <BootstrapSummary state={state} />

      {actionMessage ? (
        <section className="panel compact-panel">
          <p className="empty-text">{actionMessage}</p>
        </section>
      ) : null}

      <CommandErrorsPanel title="Dashboard Errors" errors={dashboardErrors} />

      {!canQueryDashboard ? (
        <>
          <section className="panel">
            <div className="panel-heading">
              <h2>Dashboard Overview</h2>
              <span className="muted">Waiting for a valid workdir</span>
            </div>
            <p className="empty-text">
              Select or initialize a workdir to load the dashboard overview.
            </p>
          </section>
          <ValidationIssues
            title="Config Validation"
            issues={state.validation.issues}
            emptyText="No config validation issues."
          />
        </>
      ) : null}

      {canQueryDashboard && !overview ? (
        <section className="panel">
          <div className="panel-heading">
            <h2>Dashboard Overview</h2>
            <span className="muted">
              {isLoadingOverview ? "Loading..." : "No overview data"}
            </span>
          </div>
          <p className="empty-text">
            {isLoadingOverview
              ? "Loading dashboard overview."
              : "Dashboard overview is not available."}
          </p>
        </section>
      ) : null}

      {overview ? (
        <>
          <section className="panel">
            <div className="panel-heading">
              <h2>{overview.project.name}</h2>
              <span className="muted">Generated {formatDateTime(overview.generated_at)}</span>
            </div>
            <InfoGrid
              items={[
                { label: "Workdir", value: overview.project.workdir_path },
                { label: "Last scan", value: formatDateTime(overview.runtime.last_scan_at) },
                { label: "Last run", value: formatDateTime(overview.runtime.last_run_at) },
                {
                  label: "Last successful run",
                  value: formatDateTime(overview.runtime.last_successful_run_at),
                },
                { label: "Last error", value: formatDateTime(overview.runtime.last_error_at) },
                { label: "Files", value: overview.totals.entity_files_total },
              ]}
            />
          </section>

          <SummaryCards overview={overview} />
          <StageGraph overview={overview} />
          <StageCountersTable counters={overview.stage_counters} />
          <div className="dashboard-split">
            <ActiveTasksPanel tasks={overview.active_tasks} />
            <LastErrorsPanel errors={overview.last_errors} />
          </div>
          <RecentRunsPanel runs={overview.recent_runs} />
        </>
      ) : null}

      {canQueryDashboard ? (
        <ValidationIssues
          title="Config Validation"
          issues={state.validation.issues}
          emptyText="No config validation issues."
        />
      ) : null}
    </div>
  );
}
