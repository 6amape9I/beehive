import type { DashboardOverview } from "../../types/domain";

export function SummaryCards({ overview }: { overview: DashboardOverview }) {
  const cards = [
    { label: "Entities", value: overview.totals.entities_total },
    { label: "Active stages", value: overview.totals.active_stages_total },
    { label: "Pending / due", value: overview.runtime.due_tasks_count },
    { label: "In progress", value: overview.runtime.in_progress_count },
    { label: "Retry wait", value: overview.runtime.retry_wait_count },
    { label: "Failed", value: overview.runtime.failed_count },
    { label: "Blocked", value: overview.runtime.blocked_count },
    { label: "Recent errors", value: overview.totals.errors_total },
  ];

  return (
    <section className="summary-card-grid" aria-label="Dashboard summary">
      {cards.map((card) => (
        <article className="summary-card" key={card.label}>
          <span>{card.label}</span>
          <strong>{card.value}</strong>
        </article>
      ))}
    </section>
  );
}
