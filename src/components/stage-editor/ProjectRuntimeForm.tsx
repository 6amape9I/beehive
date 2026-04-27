import type { PipelineConfigDraft, RuntimeConfigDraft } from "../../types/domain";

interface ProjectRuntimeFormProps {
  draft: PipelineConfigDraft;
  disabled: boolean;
  onChange: (draft: PipelineConfigDraft) => void;
}

type RuntimeField = keyof RuntimeConfigDraft;

const runtimeFields: Array<{ key: RuntimeField; label: string; min: number }> = [
  { key: "scan_interval_sec", label: "Scan interval seconds", min: 1 },
  { key: "max_parallel_tasks", label: "Max tasks per run", min: 1 },
  { key: "stuck_task_timeout_sec", label: "Stuck task timeout seconds", min: 1 },
  { key: "request_timeout_sec", label: "Request timeout seconds", min: 1 },
  { key: "file_stability_delay_ms", label: "File stability delay ms", min: 0 },
];

export function ProjectRuntimeForm({ draft, disabled, onChange }: ProjectRuntimeFormProps) {
  function updateRuntime(key: RuntimeField, value: number) {
    onChange({
      ...draft,
      runtime: {
        ...draft.runtime,
        [key]: value,
      },
    });
  }

  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>Project / Runtime</h2>
        <span className="muted">Saved to pipeline.yaml</span>
      </div>
      <div className="stage-editor-form-grid">
        <div className="form-row">
          <label htmlFor="project-name">Project name</label>
          <input
            id="project-name"
            value={draft.project.name}
            disabled={disabled}
            onChange={(event) =>
              onChange({
                ...draft,
                project: { ...draft.project, name: event.target.value },
              })
            }
          />
        </div>
        <div className="form-row">
          <label htmlFor="project-workdir">Project workdir</label>
          <input id="project-workdir" value={draft.project.workdir} disabled readOnly />
          <p className="field-hint">Runtime uses the selected workdir path.</p>
        </div>
        {runtimeFields.map((field) => (
          <div className="form-row" key={field.key}>
            <label htmlFor={field.key}>{field.label}</label>
            <input
              id={field.key}
              type="number"
              min={field.min}
              value={draft.runtime[field.key]}
              disabled={disabled}
              onChange={(event) => updateRuntime(field.key, Number(event.target.value))}
            />
          </div>
        ))}
      </div>
    </section>
  );
}
