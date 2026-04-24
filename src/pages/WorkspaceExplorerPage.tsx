import { useBootstrap } from "../app/BootstrapContext";
import { InfoGrid } from "../components/InfoGrid";

export function WorkspaceExplorerPage() {
  const { state } = useBootstrap();

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Filesystem</span>
          <h1>Workspace Explorer</h1>
        </div>
      </div>
      <section className="panel">
        <div className="panel-heading">
          <h2>Workspace Paths</h2>
          <span className="muted">Stage 1 filesystem visibility</span>
        </div>
        <InfoGrid
          items={[
            { label: "Workdir", value: state.workdir_state?.workdir_path },
            { label: "Config file", value: state.workdir_state?.pipeline_config_path },
            { label: "Database file", value: state.workdir_state?.database_path },
            { label: "Stages directory", value: state.workdir_state?.stages_dir_path },
            { label: "Logs directory", value: state.workdir_state?.logs_dir_path },
          ]}
        />
        <p className="empty-text">
          Directory browsing and file placement inspection are reserved for later stages.
        </p>
      </section>
    </div>
  );
}
