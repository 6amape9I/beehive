import { useParams } from "react-router-dom";

export function EntityDetailPage() {
  const { entityId } = useParams();

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Future runtime</span>
          <h1>Entity Detail</h1>
        </div>
      </div>
      <section className="panel">
        <h2>{entityId ?? "No entity selected"}</h2>
        <p className="empty-text">
          Stage 1 wires this route for the approved screen set. Entity runtime history and manual
          operations are deferred.
        </p>
      </section>
    </div>
  );
}
