import { StatusBadge } from "../StatusBadge";
import type { DashboardOverview, DashboardStageCounters } from "../../types/domain";

function countersFor(stageId: string, counters: DashboardStageCounters[]) {
  return counters.find((counter) => counter.stage_id === stageId);
}

export function StageGraph({ overview }: { overview: DashboardOverview }) {
  const edgeSources = new Set(overview.stage_graph.edges.map((edge) => edge.from_stage_id));
  const terminalNodes = overview.stage_graph.nodes.filter((node) => !edgeSources.has(node.id));

  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>Stage Graph</h2>
        <span className="muted">{overview.stage_graph.nodes.length} stage(s)</span>
      </div>
      {overview.stage_graph.nodes.length === 0 ? (
        <p className="empty-text">No stages are configured.</p>
      ) : (
        <>
          <div className="stage-graph">
            {overview.stage_graph.nodes.map((node) => {
              const counter = countersFor(node.id, overview.stage_counters);
              return (
                <article className={`stage-node stage-node-${node.health}`} key={node.id}>
                  <div className="stage-node-header">
                    <strong>{node.label}</strong>
                    <StatusBadge status={node.health} />
                  </div>
                  <div className="stage-node-meta">
                    <span>{node.is_active ? "active" : "inactive"}</span>
                    <span>next: {node.next_stage ?? "terminal"}</span>
                  </div>
                  <div className="stage-node-counters">
                    <span>pending {counter?.pending ?? 0}</span>
                    <span>running {counter?.in_progress ?? 0}</span>
                    <span>done {counter?.done ?? 0}</span>
                    <span>failed {counter?.failed ?? 0}</span>
                    <span>blocked {counter?.blocked ?? 0}</span>
                  </div>
                </article>
              );
            })}
          </div>
          <div className="stage-links">
            <h3>Stage Links</h3>
            <div className="stage-link-list">
              {overview.stage_graph.edges.map((edge) => (
                <article
                  className={`stage-link-row ${edge.is_valid ? "stage-link-valid" : "stage-link-invalid"}`}
                  key={`${edge.from_stage_id}-${edge.to_stage_id}`}
                >
                  <StatusBadge status={edge.is_valid ? "valid" : "invalid"} />
                  <strong>
                    {edge.from_stage_id} -&gt; {edge.to_stage_id}
                  </strong>
                  <span>{edge.problem ?? "Link target is active."}</span>
                </article>
              ))}
              {terminalNodes.map((node) => (
                <article className="stage-link-row stage-link-terminal" key={`${node.id}-terminal`}>
                  <StatusBadge status="terminal" />
                  <strong>{node.id} -&gt; terminal</strong>
                  <span>No next stage configured.</span>
                </article>
              ))}
            </div>
          </div>
        </>
      )}
    </section>
  );
}
