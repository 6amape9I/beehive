import type { StageDefinitionDraft, StageUsageSummary } from "../../types/domain";
import { StageUsageSummary as StageUsageSummaryView } from "./StageUsageSummary";

interface StageDraftFormProps {
  stage: StageDefinitionDraft | null;
  usage: StageUsageSummary | null;
  disabled: boolean;
  removeBlockedReason: string | null;
  onChange: (stage: StageDefinitionDraft) => void;
  onRemove: () => void;
}

const OUTPUT_CARDINALITY_HELP =
  'По умолчанию stage ожидает ровно 1 output. Если workflow может отфильтровать вход и ничего не вернуть - включите "Разрешено 0 выходов". Если workflow может породить несколько новых сущностей - включите "Разрешено несколько выходов".';
const LOCAL_LLM_HELP =
  "Если включено, этот stage будет выполняться отдельным пулом local_llm с ограниченным параллелизмом.";

function allowsZeroOutputs(stage: StageDefinitionDraft) {
  return !!(stage.allow_zero_outputs ?? stage.allow_empty_outputs ?? false);
}

export function StageDraftForm({
  stage,
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
  const zeroOutputsAllowed = allowsZeroOutputs(currentStage);
  const savePathAliases = currentStage.save_path_aliases ?? [];

  function update(patch: Partial<StageDefinitionDraft>) {
    onChange({ ...currentStage, ...patch });
  }

  function updateAliases(value: string) {
    update({
      save_path_aliases: value
        .split(/\r?\n/)
        .map((item) => item.trim())
        .filter(Boolean),
    });
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
          <label htmlFor="stage-input">Input folder</label>
          <input
            id="stage-input"
            value={currentStage.input_folder}
            disabled={disabled}
            onChange={(event) => update({ input_folder: event.target.value })}
          />
        </div>
        <div className="form-row">
          <label htmlFor="stage-input-uri">Input URI</label>
          <input
            id="stage-input-uri"
            value={currentStage.input_uri ?? ""}
            disabled={disabled}
            onChange={(event) => update({ input_uri: event.target.value || null })}
          />
        </div>
        <div className="form-row">
          <label htmlFor="stage-output">Output folder</label>
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
        <div className="form-row">
          <label htmlFor="stage-save-path-aliases">Save path aliases</label>
          <textarea
            id="stage-save-path-aliases"
            className="compact-textarea"
            value={savePathAliases.join("\n")}
            disabled={disabled}
            rows={3}
            onChange={(event) => updateAliases(event.target.value)}
          />
        </div>
        <div className="form-row">
          <label htmlFor="stage-uses-local-llm">Resource usage</label>
          <label className="checkbox-row">
            <input
              id="stage-uses-local-llm"
              type="checkbox"
              checked={currentStage.resource_class === "local_llm"}
              disabled={disabled}
              onChange={(event) =>
                update({ resource_class: event.target.checked ? "local_llm" : "default" })
              }
            />
            <span>Использует локальную LLM</span>
          </label>
          <p className="field-hint">{LOCAL_LLM_HELP}</p>
        </div>
        <div className="form-row">
          <label htmlFor="stage-allow-zero-outputs">Output cardinality</label>
          <label className="checkbox-row">
            <input
              id="stage-allow-zero-outputs"
              type="checkbox"
              checked={zeroOutputsAllowed}
              disabled={disabled}
              onChange={(event) => update({ allow_zero_outputs: event.target.checked })}
            />
            <span>Разрешено 0 выходов</span>
          </label>
          <label className="checkbox-row">
            <input
              id="stage-allow-multiple-outputs"
              type="checkbox"
              checked={!!currentStage.allow_multiple_outputs}
              disabled={disabled}
              onChange={(event) => update({ allow_multiple_outputs: event.target.checked })}
            />
            <span>Разрешено несколько выходов</span>
          </label>
          <p className="field-hint">{OUTPUT_CARDINALITY_HELP}</p>
        </div>
      </div>
      <StageUsageSummaryView usage={usage} />
    </section>
  );
}
