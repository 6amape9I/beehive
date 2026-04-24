import { useCallback, useEffect, useState } from "react";

import { useBootstrap } from "../app/BootstrapContext";
import { CommandErrorsPanel } from "../components/CommandErrorsPanel";
import { InfoGrid } from "../components/InfoGrid";
import { ValidationIssues } from "../components/ValidationIssues";
import { BootstrapSummary } from "../features/bootstrap/BootstrapSummary";
import { WorkdirSetupPanel } from "../features/workdir/WorkdirSetupPanel";
import { formatDateTime } from "../lib/formatters";
import { getRuntimeSummary, scanWorkspace } from "../lib/runtimeApi";
import type { CommandErrorInfo, RuntimeSummary, ScanSummary } from "../types/domain";

export function DashboardPage() {
  const { state } = useBootstrap();
  const [runtimeSummary, setRuntimeSummary] = useState<RuntimeSummary | null>(null);
  const [lastScan, setLastScan] = useState<ScanSummary | null>(null);
  const [runtimeErrors, setRuntimeErrors] = useState<CommandErrorInfo[]>([]);
  const [isLoadingSummary, setIsLoadingSummary] = useState(false);
  const [isScanning, setIsScanning] = useState(false);

  const workdirPath = state.selected_workdir_path;
  const canQueryRuntime = state.phase === "fully_initialized" && !!workdirPath;

  const loadRuntimeSummary = useCallback(async () => {
    if (!canQueryRuntime || !workdirPath) {
      setRuntimeSummary(null);
      setRuntimeErrors([]);
      return;
    }

    setIsLoadingSummary(true);
    try {
      const result = await getRuntimeSummary(workdirPath);
      setRuntimeSummary(result.summary);
      setRuntimeErrors(result.errors);
    } finally {
      setIsLoadingSummary(false);
    }
  }, [canQueryRuntime, workdirPath]);

  useEffect(() => {
    void loadRuntimeSummary();
  }, [loadRuntimeSummary]);

  async function handleScanWorkspace() {
    if (!workdirPath) {
      return;
    }

    setIsScanning(true);
    try {
      const result = await scanWorkspace(workdirPath);
      setLastScan(result.summary);
      setRuntimeErrors(result.errors);
      await loadRuntimeSummary();
    } finally {
      setIsScanning(false);
    }
  }

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Overview</span>
          <h1>Dashboard</h1>
        </div>
        <button
          type="button"
          className="button primary"
          disabled={!canQueryRuntime || isScanning}
          onClick={() => void handleScanWorkspace()}
        >
          {isScanning ? "Scanning..." : "Scan workspace"}
        </button>
      </div>
      <WorkdirSetupPanel />
      <BootstrapSummary state={state} />
      <section className="panel">
        <div className="panel-heading">
          <h2>Runtime Summary</h2>
          <span className="muted">
            {isLoadingSummary ? "Loading..." : runtimeSummary ? "Live Stage 3 data" : "No runtime data"}
          </span>
        </div>
        <InfoGrid
          items={[
            { label: "Active stages", value: runtimeSummary?.active_stage_count },
            { label: "Inactive stages", value: runtimeSummary?.inactive_stage_count },
            { label: "Logical entities", value: runtimeSummary?.total_entities },
            { label: "Present files", value: runtimeSummary?.present_file_count },
            { label: "Missing files", value: runtimeSummary?.missing_file_count },
            { label: "Managed copies", value: runtimeSummary?.managed_copy_count },
            { label: "Invalid files", value: runtimeSummary?.invalid_file_count },
            {
              label: "Last reconciliation",
              value: formatDateTime(runtimeSummary?.last_reconciliation_at),
            },
            { label: "Schema version", value: runtimeSummary?.schema_version },
          ]}
        />
        <div className="stage-chip-row">
          {runtimeSummary?.entities_by_status.length ? (
            runtimeSummary.entities_by_status.map((entry) => (
              <span className="stage-chip" key={entry.status}>
                {entry.status}: {entry.count}
              </span>
            ))
          ) : (
            <span className="muted">No registered entity statuses yet.</span>
          )}
        </div>
      </section>
      {lastScan ? (
        <section className="panel">
          <div className="panel-heading">
            <h2>Last Scan</h2>
            <span className="muted">{formatDateTime(lastScan.latest_discovery_at)}</span>
          </div>
          <InfoGrid
            items={[
              { label: "Scanned files", value: lastScan.scanned_file_count },
              { label: "Registered files", value: lastScan.registered_file_count },
              { label: "Registered entities", value: lastScan.registered_entity_count },
              { label: "Updated files", value: lastScan.updated_file_count },
              { label: "Unchanged files", value: lastScan.unchanged_file_count },
              { label: "Missing files", value: lastScan.missing_file_count },
              { label: "Restored files", value: lastScan.restored_file_count },
              { label: "Invalid", value: lastScan.invalid_count },
              { label: "Duplicates", value: lastScan.duplicate_count },
              { label: "Created directories", value: lastScan.created_directory_count },
              { label: "Managed copies", value: lastScan.managed_copy_count },
              { label: "Elapsed (ms)", value: lastScan.elapsed_ms },
              { label: "Scan ID", value: lastScan.scan_id },
            ]}
          />
        </section>
      ) : null}
      <CommandErrorsPanel title="Runtime Errors" errors={runtimeErrors} />
      <ValidationIssues
        title="Config Validation"
        issues={state.validation.issues}
        emptyText="No config validation issues."
      />
    </div>
  );
}
