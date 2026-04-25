interface DashboardActionsProps {
  canRun: boolean;
  activeAction: string | null;
  onRefresh: () => void;
  onScan: () => void;
  onRunDue: () => void;
  onReconcile: () => void;
}

export function DashboardActions({
  canRun,
  activeAction,
  onRefresh,
  onScan,
  onRunDue,
  onReconcile,
}: DashboardActionsProps) {
  const disabled = !canRun || activeAction !== null;

  return (
    <div className="button-row">
      <button type="button" className="button secondary" disabled={disabled} onClick={onRefresh}>
        {activeAction === "refresh" ? "Refreshing..." : "Refresh"}
      </button>
      <button type="button" className="button primary" disabled={disabled} onClick={onScan}>
        {activeAction === "scan" ? "Scanning..." : "Scan workspace"}
      </button>
      <button type="button" className="button secondary" disabled={disabled} onClick={onRunDue}>
        {activeAction === "run" ? "Running..." : "Run due tasks"}
      </button>
      <button type="button" className="button secondary" disabled={disabled} onClick={onReconcile}>
        {activeAction === "reconcile" ? "Reconciling..." : "Reconcile stuck"}
      </button>
    </div>
  );
}
