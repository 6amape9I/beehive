import type {
  ConfigValidationIssue,
  StageDefinitionDraft,
  StageUsageSummary,
} from "../../types/domain";
import { StatusBadge } from "../StatusBadge";

interface StageDraftListProps {
  stages: StageDefinitionDraft[];
  usages: StageUsageSummary[];
  issues: ConfigValidationIssue[];
  selectedIndex: number | null;
  onSelect: (index: number) => void;
}

function usageFor(usages: StageUsageSummary[], stage: StageDefinitionDraft) {
  return usages.find((usage) => usage.stage_id === (stage.original_stage_id ?? stage.id)) ?? null;
}

function issueCount(issues: ConfigValidationIssue[], index: number, stageId: string) {
  const prefix = `stages[${index}]`;
  return issues.filter(
    (issue) => issue.path.startsWith(prefix) || issue.message.includes(`'${stageId}'`),
  ).length;
}

export function StageDraftList({
  stages,
  usages,
  issues,
  selectedIndex,
  onSelect,
}: StageDraftListProps) {
  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>Draft Stages</h2>
        <span className="muted">{stages.length} stage(s)</span>
      </div>
      {stages.length === 0 ? (
        <p className="empty-text">No stages in draft.</p>
      ) : (
        <div className="table-wrap">
          <table className="stage-editor-table">
            <thead>
              <tr>
                <th>Select</th>
                <th>ID</th>
                <th>Input</th>
                <th>Output</th>
                <th>Next</th>
                <th>Retry</th>
                <th>Usage</th>
                <th>Validation</th>
              </tr>
            </thead>
            <tbody>
              {stages.map((stage, index) => {
                const usage = usageFor(usages, stage);
                const count = issueCount(issues, index, stage.id);
                return (
                  <tr key={`${stage.original_stage_id ?? stage.id}-${index}`} className={selectedIndex === index ? "selected-row" : ""}>
                    <td>
                      <button
                        type="button"
                        className="button secondary"
                        onClick={() => onSelect(index)}
                      >
                        Select
                      </button>
                    </td>
                    <td>
                      <strong>{stage.id || "(missing)"}</strong>
                      {stage.is_new ? <div className="muted">new</div> : null}
                    </td>
                    <td><code>{stage.input_folder}</code></td>
                    <td>{stage.next_stage ? <code>{stage.output_folder}</code> : "Terminal"}</td>
                    <td>{stage.next_stage ?? "End"}</td>
                    <td>{stage.max_attempts} / {stage.retry_delay_sec}s</td>
                    <td>
                      {usage ? (
                        <span>{usage.entity_count} entities, {usage.run_count} runs</span>
                      ) : (
                        <span className="muted">none</span>
                      )}
                    </td>
                    <td>
                      {count > 0 ? <StatusBadge status="warning" /> : <StatusBadge status="valid" />}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
