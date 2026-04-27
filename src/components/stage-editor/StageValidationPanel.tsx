import type { ConfigValidationIssue } from "../../types/domain";
import { ValidationIssues } from "../ValidationIssues";

interface StageValidationPanelProps {
  issues: ConfigValidationIssue[];
}

export function StageValidationPanel({ issues }: StageValidationPanelProps) {
  return (
    <ValidationIssues
      title="Pipeline Validation"
      issues={issues}
      emptyText="No pipeline validation issues."
    />
  );
}
