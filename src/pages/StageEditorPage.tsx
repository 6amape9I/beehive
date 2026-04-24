import { useBootstrap } from "../app/BootstrapContext";

export function StageEditorPage() {
  const { state } = useBootstrap();
  const stages = state.config?.stages ?? [];

  return (
    <div className="page-stack">
      <div className="page-heading">
        <div>
          <span className="eyebrow">Configuration</span>
          <h1>Stage Editor</h1>
        </div>
      </div>
      <section className="panel">
        <div className="panel-heading">
          <h2>Loaded Stage Definitions</h2>
          <span className="muted">{stages.length} stage(s)</span>
        </div>
        {stages.length === 0 ? (
          <p className="empty-text">Open or initialize a workdir to load stage definitions.</p>
        ) : (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Stage ID</th>
                  <th>Input</th>
                  <th>Output</th>
                  <th>Workflow URL</th>
                  <th>Retry</th>
                  <th>Next</th>
                </tr>
              </thead>
              <tbody>
                {stages.map((stage) => (
                  <tr key={stage.id}>
                    <td>
                      <strong>{stage.id}</strong>
                    </td>
                    <td>{stage.input_folder}</td>
                    <td>{stage.output_folder}</td>
                    <td>
                      <code>{stage.workflow_url}</code>
                    </td>
                    <td>
                      {stage.max_attempts} attempts / {stage.retry_delay_sec}s
                    </td>
                    <td>{stage.next_stage ?? "End"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  );
}
