interface StatusBadgeProps {
  status: string;
}

export function StatusBadge({ status }: StatusBadgeProps) {
  const tone =
    status.includes("failed") ||
    status.includes("blocked") ||
    status.includes("invalid") ||
    status.includes("error")
      ? "danger"
      : status.includes("fully") ||
          status === "valid" ||
          status === "ready" ||
          status === "active" ||
          status === "success" ||
          status === "done" ||
          status === "ok"
        ? "success"
        : status.includes("warning") || status === "inactive" || status === "retry_wait"
          ? "warning"
          : "neutral";

  return <span className={`status-badge ${tone}`}>{status.replaceAll("_", " ")}</span>;
}
