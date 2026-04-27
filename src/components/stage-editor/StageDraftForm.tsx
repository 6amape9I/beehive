import type { StageDefinitionDraft, StageUsageSummary } from "../../types/domain";
import { StageUsageSummary as StageUsageSummaryView } from "./StageUsageSummary";

interface StageDraftFormProps {
  stage: StageDefinitionDraft | null;
  stages: StageDefinitionDraft[];
  usage: StageUsageSummary | null;
  disabled: boolean;
  removeBlockedReason: string | null;
  onChange: (stage: StageDefinitionDraft) => void;
  onRemove: () => void;
}

export function StageDraftForm({
  stage,
  stages,
  usage,
  disabled,
  removeBlockedReason,
  onChange,
  onRemove,
}: StageDraftFormProps) {
  if (!stage) {
    return (
      <section className="panel">
        <h2>Stage Form</h2>
        <p className="empty-text">Select a stage or add a new one.</p>
      </section>
    );
  }

  const currentStage = stage;
  const stageIdLocked = !currentStage.is_new;
  const terminal = !currentStage.next_stage;

  function update(patch: Partial<StageDefinitionDraft>) {
    onChange({ ...currentStage, ...patch });
  }

  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h2>Stage Form</h2>
          <span className="muted">{currentStage.is_new ? "New draft stage" : "Saved stage id is immutable"}</span>
        </div>
        <button
          type="button"
          className="button secondary"
          disabled={disabled || !!removeBlockedReason}
          onClick={onRemove}
          title={removeBlockedReason ?? undefined}
        >
          Remove from config
        </button>
      </div>
      {removeBlockedReason ? <p className="error-text">{removeBlockedReason}</p> : null}
      <div className="stage-editor-form-grid">
        <div className="form-row">
          <label htmlFor="stage-id">Stage ID</label>
          <input
            id="stage-id"
            value={currentStage.id}
            disabled={disabled || stageIdLocked}
            onChange={(event) => update({ id: event.target.value })}
          />
          {stageIdLocked ? <p className="field-hint">Saved stage IDs cannot be renamed in Stage 7.</p> : null}
        </div>
        <div className="form-row">
          <label htmlFor="stage-next">Next stage</label>
          <select
            id="stage-next"
            value={currentStage.next_stage ?? ""}
            disabled={disabled}
            onChange={(event) => update({ next_stage: event.target.value || null })}
          >
            <option value="">End / terminal</option>
            {stages
              .filter((candidate) => candidate.id !== currentStage.id)
              .map((candidate) => (
                <option key={candidate.id} value={candidate.id}>
                  {candidate.id}
                </option>
              ))}
          </select>
        </div>
        <div className="form-row">
          <label htmlFor="stage-input">Input folder</label>
          <input
            id="stage-input"
            value={currentStage.input_folder}
            disabled={disabled}
            onChange={(event) => update({ input_folder: event.target.value })}
          />
        </div>
        <div className="form-row">
          <label htmlFor="stage-output">
            Output folder {terminal ? "(optional for terminal stage)" : ""}
          </label>
          <input
            id="stage-output"
            value={currentStage.output_folder}
            disabled={disabled}
            onChange={(event) => update({ output_folder: event.target.value })}
          />
        </div>
        <div className="form-row">
          <label htmlFor="stage-workflow">Workflow URL</label>
          <input
            id="stage-workflow"
            value={currentStage.workflow_url}
            disabled={disabled}
            onChange={(event) => update({ workflow_url: event.target.value })}
          />
        </div>
        <div className="form-row">
          <label htmlFor="stage-max-attempts">Max attempts</label>
          <input
            id="stage-max-attempts"
            type="number"
            min={1}
            value={currentStage.max_attempts}
            disabled={disabled}
            onChange={(event) => update({ max_attempts: Number(event.target.value) })}
          />
        </div>
        <div className="form-row">
          <label htmlFor="stage-retry-delay">Retry delay seconds</label>
          <input
            id="stage-retry-delay"
            type="number"
            min={0}
            value={currentStage.retry_delay_sec}
            disabled={disabled}
            onChange={(event) => update({ retry_delay_sec: Number(event.target.value) })}
          />
        </div>
      </div>
      <StageUsageSummaryView usage={usage} />
    </section>
  );
}
