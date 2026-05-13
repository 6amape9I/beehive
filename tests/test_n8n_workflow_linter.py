from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).resolve().parents[1] / "scripts" / "lint_n8n_workflows.py"
SPEC = importlib.util.spec_from_file_location("lint_n8n_workflows", SCRIPT_PATH)
assert SPEC and SPEC.loader
lint_n8n_workflows = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = lint_n8n_workflows
SPEC.loader.exec_module(lint_n8n_workflows)


def write_workflow(path: Path, *, header: bool = False, method: str = "POST", operation: str = "download") -> None:
    parameters = {"operation": operation, "bucketName": "={{ $json.body.source_bucket }}"}
    if header:
        parameters["headers"] = {"X-Beehive-Source-Key": "={{ $json.body.source_key }}"}
    path.write_text(
        """
{
  "nodes": [
    {
      "name": "Webhook",
      "type": "n8n-nodes-base.webhook",
      "parameters": {
        "httpMethod": "%s",
        "responseMode": "responseNode"
      }
    },
    {
      "name": "S3",
      "type": "n8n-nodes-base.awsS3",
      "parameters": %s
    }
  ]
}
"""
        % (method, json.dumps(parameters)),
        encoding="utf-8",
    )


class N8nWorkflowLinterTest(unittest.TestCase):
    def test_linter_accepts_body_json_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            path = tmp_path / "workflow.json"
            write_workflow(path)

            issues = lint_n8n_workflows.lint_paths([tmp_path])

        self.assertEqual(issues, [])

    def test_linter_rejects_source_key_header(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            path = tmp_path / "workflow.json"
            write_workflow(path, header=True)

            issues = lint_n8n_workflows.lint_paths([tmp_path])

        self.assertTrue(any("X-Beehive-Source-Key" in issue.message for issue in issues))

    def test_linter_rejects_non_post_webhook(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            path = tmp_path / "workflow.json"
            write_workflow(path, method="GET")

            issues = lint_n8n_workflows.lint_paths([tmp_path])

        self.assertTrue(any("method is not POST" in issue.message for issue in issues))

    def test_linter_rejects_s3_search_operation(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            path = tmp_path / "workflow.json"
            write_workflow(path, operation="search")

            issues = lint_n8n_workflows.lint_paths([tmp_path])

        self.assertTrue(any("list/search" in issue.message for issue in issues))
