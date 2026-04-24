import type { ConfigValidationIssue, WorkdirHealthIssue } from "../types/domain";
import { StatusBadge } from "./StatusBadge";

interface ValidationIssuesProps {
  title: string;
  issues: Array<ConfigValidationIssue | WorkdirHealthIssue>;
  emptyText: string;
}

export function ValidationIssues({ title, issues, emptyText }: ValidationIssuesProps) {
  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>{title}</h2>
        <span className="muted">{issues.length} issue(s)</span>
      </div>
      {issues.length === 0 ? (
        <p className="empty-text">{emptyText}</p>
      ) : (
        <div className="issue-list">
          {issues.map((issue) => (
            <article className="issue-row" key={`${issue.code}-${issue.path}-${issue.message}`}>
              <StatusBadge status={issue.severity} />
              <div>
                <strong>{issue.code}</strong>
                <p>{issue.message}</p>
                <code>{issue.path}</code>
              </div>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}
