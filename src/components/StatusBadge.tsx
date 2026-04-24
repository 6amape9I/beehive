interface StatusBadgeProps {
  status: string;
}

export function StatusBadge({ status }: StatusBadgeProps) {
  const tone =
    status.includes("failed") || status.includes("invalid") || status.includes("error")
      ? "danger"
      : status.includes("fully") || status === "valid" || status === "ready" || status === "active"
        ? "success"
        : status.includes("warning") || status === "inactive"
          ? "warning"
          : "neutral";

  return <span className={`status-badge ${tone}`}>{status.replaceAll("_", " ")}</span>;
}
