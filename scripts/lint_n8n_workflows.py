#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


FORBIDDEN_STRINGS = (
    "X-Beehive-Source-Key",
    "/main_dir/pocessed",
)


@dataclass(frozen=True)
class WorkflowIssue:
    path: Path
    severity: str
    message: str


def iter_workflow_files(paths: Iterable[Path]) -> list[Path]:
    files: list[Path] = []
    for path in paths:
        if path.is_dir():
            files.extend(sorted(path.rglob("*.json")))
        elif path.suffix.lower() == ".json":
            files.append(path)
    return files


def walk_json(value: Any, path: str = "$") -> Iterable[tuple[str, Any]]:
    yield path, value
    if isinstance(value, dict):
        for key, item in value.items():
            yield f"{path}.{key}", key
            yield from walk_json(item, f"{path}.{key}")
    elif isinstance(value, list):
        for index, item in enumerate(value):
            yield from walk_json(item, f"{path}[{index}]")


def lint_workflow(path: Path) -> list[WorkflowIssue]:
    issues: list[WorkflowIssue] = []
    try:
        workflow = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        return [WorkflowIssue(path, "error", f"invalid JSON: {error}")]

    nodes = workflow.get("nodes")
    if not isinstance(nodes, list):
        issues.append(WorkflowIssue(path, "error", "workflow has no nodes array"))
        nodes = []

    for json_path, value in walk_json(workflow):
        if not isinstance(value, str):
            continue
        for forbidden in FORBIDDEN_STRINGS:
            if forbidden in value:
                issues.append(
                    WorkflowIssue(path, "error", f"{json_path} contains forbidden {forbidden}")
                )
        if json_path.lower().endswith("save_path") and looks_like_local_absolute_path(value):
            issues.append(
                WorkflowIssue(path, "error", f"{json_path} has local-looking absolute save_path")
            )

    code_node_count = 0
    for node in nodes:
        if not isinstance(node, dict):
            continue
        node_type = str(node.get("type", "")).lower()
        node_name = str(node.get("name", "unnamed node"))
        parameters = node.get("parameters", {})

        if "webhook" in node_type and "respondtowebhook" not in node_type:
            method = str(parameters.get("httpMethod", "")).upper()
            response_mode = str(parameters.get("responseMode", ""))
            if method != "POST":
                issues.append(WorkflowIssue(path, "error", f"{node_name}: webhook method is not POST"))
            if response_mode != "responseNode":
                issues.append(
                    WorkflowIssue(path, "error", f"{node_name}: webhook responseMode is not responseNode")
                )

        if "s3" in node_type and has_list_or_search_operation(parameters):
            issues.append(
                WorkflowIssue(path, "error", f"{node_name}: S3 list/search operation is not allowed")
            )

        if "code" in node_type:
            code_node_count += 1

    if code_node_count > 2 and not workflow_allows_code_nodes(workflow):
        issues.append(
            WorkflowIssue(
                path,
                "error",
                f"workflow has {code_node_count} Code nodes without beehive_code_node_justification",
            )
        )

    return issues


def looks_like_local_absolute_path(value: str) -> bool:
    if not value.startswith("/"):
        return False
    return not (value.startswith("/main_dir/") or value.startswith("/workspace/"))


def has_list_or_search_operation(parameters: Any) -> bool:
    for json_path, value in walk_json(parameters):
        if not isinstance(value, str):
            continue
        key = json_path.rsplit(".", 1)[-1].lower()
        normalized = value.strip().lower()
        if key in {"operation", "resource"} and normalized in {"list", "search", "listbucket", "searchbucket"}:
            return True
        if key == "operation" and re.search(r"\b(list|search)\b", normalized):
            return True
    return False


def workflow_allows_code_nodes(workflow: dict[str, Any]) -> bool:
    meta = workflow.get("meta")
    if isinstance(meta, dict) and meta.get("beehive_code_node_justification"):
        return True
    lint_config = workflow.get("beehive_linter")
    return isinstance(lint_config, dict) and bool(lint_config.get("code_node_justification"))


def lint_paths(paths: Iterable[Path]) -> list[WorkflowIssue]:
    issues: list[WorkflowIssue] = []
    files = iter_workflow_files(paths)
    if not files:
        return [WorkflowIssue(Path("."), "error", "no workflow JSON files found")]
    for path in files:
        issues.extend(lint_workflow(path))
    return issues


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Lint Beehive n8n workflow fixtures.")
    parser.add_argument("paths", nargs="+", type=Path)
    args = parser.parse_args(argv)

    issues = lint_paths(args.paths)
    for issue in issues:
        print(f"{issue.severity.upper()} {issue.path}: {issue.message}")
    return 1 if any(issue.severity == "error" for issue in issues) else 0


if __name__ == "__main__":
    raise SystemExit(main())
