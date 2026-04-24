export function EntitiesPage() {
  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Future runtime</span>
          <h1>Entities</h1>
        </div>
      </div>
      <section className="panel">
        <h2>Entity list placeholder</h2>
        <p className="empty-text">
          Stage 1 prepares entity domain types and SQLite tables, but does not scan or process JSON
          entities yet.
        </p>
      </section>
    </div>
  );
}
