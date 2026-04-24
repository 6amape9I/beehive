import { InfoGrid } from "../../components/InfoGrid";
import { StatusBadge } from "../../components/StatusBadge";
import type { AppInitializationState } from "../../types/domain";

export function BootstrapSummary({ state }: { state: AppInitializationState }) {
  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>Bootstrap State</h2>
        <StatusBadge status={state.phase} />
      </div>
      <InfoGrid
        items={[
          { label: "Project", value: state.project_name },
          { label: "Workdir", value: state.selected_workdir_path },
          { label: "Config", value: state.config_path },
          { label: "Database", value: state.database_path },
          { label: "Config status", value: state.config_status },
          { label: "Database status", value: state.database_status },
          { label: "Stage count", value: state.stage_count },
          { label: "Last config load", value: state.last_config_load_at },
        ]}
      />
      <div className="stage-chip-row">
        {state.stage_ids.length === 0 ? (
          <span className="muted">No stages loaded.</span>
        ) : (
          state.stage_ids.map((stageId) => (
            <span className="stage-chip" key={stageId}>
              {stageId}
            </span>
          ))
        )}
      </div>
    </section>
  );
}
