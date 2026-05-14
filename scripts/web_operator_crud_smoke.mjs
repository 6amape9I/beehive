#!/usr/bin/env node

const apiBase = (process.env.BEEHIVE_API_BASE_URL ?? "http://127.0.0.1:8787").replace(/\/+$/, "");
const token = process.env.BEEHIVE_OPERATOR_TOKEN ?? "";
const suffix = `${Date.now()}-${Math.random().toString(16).slice(2, 8)}`;
const workspaceId = process.env.BEEHIVE_CRUD_SMOKE_WORKSPACE_ID ?? `crud-smoke-${suffix}`;

function headers() {
  return {
    Accept: "application/json",
    "Content-Type": "application/json",
    ...(token ? { Authorization: `Bearer ${token}` } : {}),
  };
}

async function request(method, path, body) {
  const response = await fetch(`${apiBase}${path}`, {
    method,
    headers: headers(),
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const payload = await response.json();
  if (!response.ok) {
    throw new Error(`${method} ${path} failed with HTTP ${response.status}`);
  }
  return payload;
}

function assertNoErrors(label, payload) {
  const errors = payload?.errors ?? [];
  if (errors.length > 0) {
    throw new Error(`${label} returned errors: ${JSON.stringify(errors)}`);
  }
}

function assertErrorCode(label, payload, code) {
  const errors = payload?.errors ?? [];
  if (!errors.some((error) => error.code === code)) {
    throw new Error(`${label} expected ${code}, got ${JSON.stringify(errors)}`);
  }
}

async function main() {
  const health = await request("GET", "/api/health");
  if (health.status !== "ok") {
    throw new Error(`Unexpected health response: ${JSON.stringify(health)}`);
  }

  const createWorkspace = await request("POST", "/api/workspaces", {
    id: workspaceId,
    name: "CRUD Smoke Workspace",
    bucket: "crud-smoke-bucket",
    workspace_prefix: `beehive-crud-smoke/${workspaceId}`,
    region: "test-region",
    endpoint: "https://s3.example.test",
  });
  assertNoErrors("POST workspace", createWorkspace);

  const patchWorkspace = await request("PATCH", `/api/workspaces/${encodeURIComponent(workspaceId)}`, {
    name: "CRUD Smoke Workspace Updated",
    endpoint: "https://s3-updated.example.test",
    region: "test-region-2",
  });
  assertNoErrors("PATCH workspace", patchWorkspace);

  const createStageA = await request("POST", `/api/workspaces/${encodeURIComponent(workspaceId)}/stages`, {
    stage_id: "stage_a",
    workflow_url: "https://n8n.example.test/webhook/stage-a",
    next_stage: null,
    max_attempts: 3,
    retry_delay_sec: 30,
    allow_empty_outputs: false,
  });
  assertNoErrors("POST stage_a", createStageA);

  const createStageB = await request("POST", `/api/workspaces/${encodeURIComponent(workspaceId)}/stages`, {
    stage_id: "stage_b",
    workflow_url: "https://n8n.example.test/webhook/stage-b",
    next_stage: null,
    max_attempts: 2,
    retry_delay_sec: 15,
    allow_empty_outputs: true,
  });
  assertNoErrors("POST stage_b", createStageB);

  const patchStageB = await request(
    "PATCH",
    `/api/workspaces/${encodeURIComponent(workspaceId)}/stages/stage_b`,
    {
      workflow_url: "https://n8n.example.test/webhook/stage-b-updated",
      max_attempts: 4,
      retry_delay_sec: 45,
      allow_empty_outputs: false,
      next_stage: null,
    },
  );
  assertNoErrors("PATCH stage_b", patchStageB);

  const linkStages = await request(
    "POST",
    `/api/workspaces/${encodeURIComponent(workspaceId)}/stages/stage_a/next-stage`,
    { next_stage: "stage_b" },
  );
  assertNoErrors("POST next-stage", linkStages);

  const blockedDelete = await request(
    "DELETE",
    `/api/workspaces/${encodeURIComponent(workspaceId)}/stages/stage_b`,
  );
  assertErrorCode("DELETE linked stage_b", blockedDelete, "delete_s3_stage_failed");

  const clearLink = await request(
    "POST",
    `/api/workspaces/${encodeURIComponent(workspaceId)}/stages/stage_a/next-stage`,
    { next_stage: null },
  );
  assertNoErrors("POST clear next-stage", clearLink);

  const deleteStageB = await request(
    "DELETE",
    `/api/workspaces/${encodeURIComponent(workspaceId)}/stages/stage_b`,
  );
  assertNoErrors("DELETE stage_b", deleteStageB);

  const deleteWorkspace = await request("DELETE", `/api/workspaces/${encodeURIComponent(workspaceId)}`);
  assertNoErrors("DELETE workspace", deleteWorkspace);

  const includeArchived = await request("GET", "/api/workspaces?include_archived=true");
  if (!Array.isArray(includeArchived.workspaces)) {
    throw new Error("include_archived workspace list did not return an array");
  }

  console.log(
    JSON.stringify({
      ok: true,
      api_base: apiBase,
      workspace_id: workspaceId,
      delete_workspace: deleteWorkspace.payload?.hard_deleted
        ? "hard_deleted"
        : deleteWorkspace.payload?.archived
          ? "archived"
          : "unknown",
      blocked_delete_code: blockedDelete.errors?.[0]?.code ?? null,
    }),
  );
}

main().catch((error) => {
  console.error(JSON.stringify({ ok: false, api_base: apiBase, workspace_id: workspaceId, error: error.message }));
  process.exit(1);
});
