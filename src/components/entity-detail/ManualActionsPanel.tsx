import { formatDateTime } from "../../lib/formatters";
import type {
  EntityStageAllowedActions,
  EntityStageStateRecord,
} from "../../types/domain";
import { StatusBadge } from "../StatusBadge";

interface ManualActionsPanelProps {
  stageStates: EntityStageStateRecord[];
  allowedActions: EntityStageAllowedActions[];
  loadingAction: string | null;
  onRetry: (stageId: string) => void;
  onReset: (stageId: string) => void;
  onSkip: (stageId: string) => void;
  onRun: (stageId: string) => void;
}

function actionsForStage(
  allowedActions: EntityStageAllowedActions[],
  stageId: string,
): EntityStageAllowedActions | undefined {
  return allowedActions.find((action) => action.stage_id === stageId);
}

export function ManualActionsPanel({
  stageStates,
  allowedActions,
  loadingAction,
  onRetry,
  onReset,
  onSkip,
  onRun,
}: ManualActionsPanelProps) {
  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>Manual Actions</h2>
        <span className="muted">Backend allowed-actions model</span>
      </div>
      {stageStates.length === 0 ? (
        <p className="empty-text">No stage state rows are available for manual actions.</p>
      ) : (
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Stage</th>
                <th>Status</th>
                <th>Attempts</th>
                <th>Next retry</th>
                <th>Actions</th>
                <th>Reason</th>
              </tr>
            </thead>
            <tbody>
              {stageStates.map((state) => {
                const allowed = actionsForStage(allowedActions, state.stage_id);
                const isBusy = loadingAction?.endsWith(`:${state.stage_id}`) ?? false;
                return (
                  <tr key={state.id}>
                    <td>{state.stage_id}</td>
                    <td>
                      <StatusBadge status={state.status} />
                    </td>
                    <td>
                      {state.attempts}/{state.max_attempts}
                    </td>
                    <td>{formatDateTime(state.next_retry_at)}</td>
                    <td>
                      <div className="button-row">
                        <button
                          type="button"
                          className="button secondary"
                          disabled={isBusy || !allowed?.can_retry_now}
                          onClick={() => onRetry(state.stage_id)}
                        >
                          Retry now
                        </button>
                        <button
                          type="button"
                          className="button secondary"
                          disabled={isBusy || !allowed?.can_reset_to_pending}
                          onClick={() => onReset(state.stage_id)}
                        >
                          Reset
                        </button>
                        <button
                          type="button"
                          className="button secondary"
                          disabled={isBusy || !allowed?.can_skip}
                          onClick={() => onSkip(state.stage_id)}
                        >
                          Skip
                        </button>
                        <button
                          type="button"
                          className="button secondary"
                          disabled={isBusy || !allowed?.can_run_this_stage}
                          onClick={() => onRun(state.stage_id)}
                        >
                          Run this stage
                        </button>
                      </div>
                    </td>
                    <td>{allowed?.reasons.join(" ") || "No action is allowed for this state."}</td>
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

