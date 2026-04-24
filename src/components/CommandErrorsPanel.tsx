import type { CommandErrorInfo } from "../types/domain";
import { StatusBadge } from "./StatusBadge";

export function CommandErrorsPanel({
  title,
  errors,
}: {
  title: string;
  errors: CommandErrorInfo[];
}) {
  if (errors.length === 0) {
    return null;
  }

  return (
    <section className="panel">
      <div className="panel-heading">
        <h2>{title}</h2>
        <span className="muted">{errors.length} error(s)</span>
      </div>
      <div className="issue-list">
        {errors.map((error) => (
          <article className="issue-row" key={`${error.code}-${error.message}`}>
            <StatusBadge status="error" />
            <div>
              <strong>{error.code}</strong>
              <p>{error.message}</p>
              {error.path ? <code>{error.path}</code> : null}
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}
