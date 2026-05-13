import type { PipelineConfigDraft, RuntimeConfigDraft } from "../../types/domain";

interface ProjectRuntimeFormProps {
  draft: PipelineConfigDraft;
  disabled: boolean;
  onChange: (draft: PipelineConfigDraft) => void;
}

type RuntimeField = keyof RuntimeConfigDraft;
type StorageField = "bucket" | "workspace_prefix" | "region" | "endpoint";

const runtimeFields: Array<{ key: RuntimeField; label: string; min: number }> = [
  { key: "scan_interval_sec", label: "Scan interval seconds", min: 1 },
  { key: "max_parallel_tasks", label: "Max tasks per run", min: 1 },
  { key: "stuck_task_timeout_sec", label: "Stuck task timeout seconds", min: 1 },
  { key: "request_timeout_sec", label: "Request timeout seconds", min: 1 },
  { key: "file_stability_delay_ms", label: "File stability delay ms", min: 0 },
];

export function ProjectRuntimeForm({ draft, disabled, onChange }: ProjectRuntimeFormProps) {
  const storage = draft.storage ?? {
    provider: "local" as const,
    bucket: null,
    workspace_prefix: null,
    region: null,
    endpoint: null,
  };

  function updateRuntime(key: RuntimeField, value: number) {
    onChange({
      ...draft,
      runtime: {
        ...draft.runtime,
        [key]: value,
      },
    });
  }

  function updateStorageProvider(provider: "local" | "s3") {
    onChange({
      ...draft,
      storage: {
        ...storage,
        provider,
      },
    });
  }

  function updateStorageField(key: StorageField, value: string) {
    onChange({
      ...draft,
      storage: {
        ...storage,
        [key]: emptyToNull(value),
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
      <h3>Storage</h3>
      <div className="stage-editor-form-grid">
        <div className="form-row">
          <label htmlFor="storage-provider">Provider</label>
          <select
            id="storage-provider"
            value={storage.provider}
            disabled={disabled}
            onChange={(event) => updateStorageProvider(event.target.value as "local" | "s3")}
          >
            <option value="local">local</option>
            <option value="s3">s3</option>
          </select>
        </div>
        <StorageInput
          disabled={disabled}
          id="storage-bucket"
          label="Bucket"
          value={storage.bucket ?? ""}
          onChange={(value) => updateStorageField("bucket", value)}
        />
        <StorageInput
          disabled={disabled}
          id="storage-workspace-prefix"
          label="Workspace prefix"
          value={storage.workspace_prefix ?? ""}
          onChange={(value) => updateStorageField("workspace_prefix", value)}
        />
        <StorageInput
          disabled={disabled}
          id="storage-region"
          label="Region"
          value={storage.region ?? ""}
          onChange={(value) => updateStorageField("region", value)}
        />
        <StorageInput
          disabled={disabled}
          id="storage-endpoint"
          label="Endpoint"
          value={storage.endpoint ?? ""}
          onChange={(value) => updateStorageField("endpoint", value)}
        />
      </div>
    </section>
  );
}

function StorageInput({
  disabled,
  id,
  label,
  onChange,
  value,
}: {
  disabled: boolean;
  id: string;
  label: string;
  onChange: (value: string) => void;
  value: string;
}) {
  return (
    <div className="form-row">
      <label htmlFor={id}>{label}</label>
      <input
        id={id}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
      />
    </div>
  );
}

function emptyToNull(value: string): string | null {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}
