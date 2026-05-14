#!/usr/bin/env node

const apiBase = (process.env.BEEHIVE_API_BASE_URL ?? "http://127.0.0.1:8787").replace(/\/+$/, "");
const token = process.env.BEEHIVE_OPERATOR_TOKEN;

async function fetchJson(path, init = {}) {
  const response = await fetch(`${apiBase}${path}`, {
    ...init,
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...(init.headers ?? {}),
    },
  });
  const text = await response.text();
  let payload = null;
  try {
    payload = text ? JSON.parse(text) : null;
  } catch (error) {
    throw new Error(`Invalid JSON from ${path}: ${error.message}`);
  }
  if (!response.ok) {
    throw new Error(`HTTP ${response.status} from ${path}: ${text}`);
  }
  return payload;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

async function main() {
  const health = await fetchJson("/api/health");
  assert(health?.status === "ok", "health response must be ok");

  const workspaces = await fetchJson("/api/workspaces");
  assert(Array.isArray(workspaces?.workspaces), "workspaces response must contain workspaces[]");
  assert(workspaces.workspaces.length > 0, "workspace registry must contain at least one workspace");

  const workspaceId = process.env.BEEHIVE_SMOKE_WORKSPACE_ID ?? workspaces.workspaces[0].id;
  const explorer = await fetchJson(`/api/workspaces/${encodeURIComponent(workspaceId)}/workspace-explorer`);
  assert(Array.isArray(explorer?.stages), "workspace explorer must contain stages[]");

  const selectedInvalid = await fetchJson(
    `/api/workspaces/${encodeURIComponent(workspaceId)}/run-selected-pipeline-waves`,
    {
      method: "POST",
      body: JSON.stringify({
        root_entity_file_ids: [],
        max_waves: 1,
        max_tasks_per_wave: 1,
        stop_on_first_failure: true,
      }),
    },
  );
  assert(
    Array.isArray(selectedInvalid?.errors) && selectedInvalid.errors.length > 0,
    "invalid selected run must return an operation error envelope",
  );

  console.log(
    JSON.stringify({
      ok: true,
      api_base: apiBase,
      workspace_id: workspaceId,
      stage_count: explorer.stages.length,
      selected_validation_code: selectedInvalid.errors[0]?.code ?? null,
    }),
  );
}

main().catch((error) => {
  console.error(JSON.stringify({ ok: false, error: error.message }));
  process.exitCode = 1;
});
