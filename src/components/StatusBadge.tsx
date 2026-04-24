interface StatusBadgeProps {
  status: string;
}

export function StatusBadge({ status }: StatusBadgeProps) {
  const tone =
    status.includes("failed") || status.includes("invalid")
      ? "danger"
      : status.includes("fully") || status === "valid" || status === "ready"
        ? "success"
        : status.includes("warning")
          ? "warning"
          : "neutral";

  return <span className={`status-badge ${tone}`}>{status.replaceAll("_", " ")}</span>;
}
