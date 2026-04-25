import { StatusBadge } from "../StatusBadge";
import type { DashboardOverview, DashboardStageCounters } from "../../types/domain";

function countersFor(stageId: string, counters: DashboardStageCounters[]) {
  return counters.find((counter) => counter.stage_id === stageId);
}

export function StageGraph({ overview }: { overview: DashboardOverview }) {
  const invalidEdges = overview.stage_graph.edges.filter((edge) => !edge.is_valid);

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
            {overview.stage_graph.nodes.map((node, index) => {
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
                  {index < overview.stage_graph.nodes.length - 1 ? (
                    <span className="stage-arrow" aria-hidden="true">
                      -&gt;
                    </span>
                  ) : null}
                </article>
              );
            })}
          </div>
          {invalidEdges.length > 0 ? (
            <div className="edge-problems">
              <h3>Link Problems</h3>
              <div className="issue-list">
                {invalidEdges.map((edge) => (
                  <article className="issue-row" key={`${edge.from_stage_id}-${edge.to_stage_id}`}>
                    <StatusBadge status="warning" />
                    <div>
                      <strong>
                        {edge.from_stage_id} -&gt; {edge.to_stage_id}
                      </strong>
                      <p>{edge.problem}</p>
                    </div>
                  </article>
                ))}
              </div>
            </div>
          ) : null}
        </>
      )}
    </section>
  );
}
