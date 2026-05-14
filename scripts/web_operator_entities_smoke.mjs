#!/usr/bin/env node

const apiBase = (process.env.BEEHIVE_API_BASE_URL ?? "http://127.0.0.1:8787").replace(/\/+$/, "");
const token = process.env.BEEHIVE_OPERATOR_TOKEN ?? "";
const suffix = `${Date.now()}-${Math.random().toString(16).slice(2, 8)}`;
const workspaceName = process.env.BEEHIVE_ENTITIES_SMOKE_WORKSPACE_NAME ?? `B9 Smoke ${suffix}`;

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

  const createWorkspace = await request("POST", "/api/workspaces", { name: workspaceName });
  assertNoErrors("POST workspace name-only", createWorkspace);
  const workspaceId = createWorkspace.payload?.workspace?.id;
  if (!workspaceId) {
    throw new Error(`Workspace id was not returned: ${JSON.stringify(createWorkspace)}`);
  }

  const createStage = await request("POST", `/api/workspaces/${encodeURIComponent(workspaceId)}/stages`, {
    stage_id: "raw_entities",
    workflow_url: "https://n8n.example.test/webhook/raw-entities",
    max_attempts: 1,
    retry_delay_sec: 0,
    allow_empty_outputs: false,
  });
  assertNoErrors("POST stage", createStage);
  if (createStage.payload?.stage?.next_stage !== null) {
    throw new Error(`New stage was expected to have next_stage=null: ${JSON.stringify(createStage.payload?.stage)}`);
  }

  const deprecatedLink = await request(
    "POST",
    `/api/workspaces/${encodeURIComponent(workspaceId)}/stages/raw_entities/next-stage`,
    { next_stage: "processed" },
  );
  assertErrorCode("POST next-stage", deprecatedLink, "next_stage_deprecated");

  const importBatch = await request(
    "POST",
    `/api/workspaces/${encodeURIComponent(workspaceId)}/entities/import-json-batch`,
    {
      stage_id: "raw_entities",
      files: [
        {
          relative_path: "entities/a.json",
          file_name: "a.json",
          content: { entity_id: `b9-smoke-a-${suffix}`, name: "Smoke A" },
        },
        {
          relative_path: "entities/b.json",
          file_name: "b.json",
          content: { id: `b9-smoke-b-${suffix}`, name: "Smoke B" },
        },
        {
          relative_path: "entities/c.json",
          file_name: "c.json",
          content: { name: "Smoke C" },
        },
      ],
      options: { overwrite_existing: false },
    },
  );
  assertNoErrors("POST import-json-batch", importBatch);
  if ((importBatch.payload?.registered_count ?? 0) < 3) {
    throw new Error(`Expected 3 registered files, got ${JSON.stringify(importBatch.payload)}`);
  }

  const entities = await request("GET", `/api/workspaces/${encodeURIComponent(workspaceId)}/entities?limit=10`);
  assertNoErrors("GET entities", entities);
  if ((entities.entities?.length ?? 0) < 3) {
    throw new Error(`Expected at least 3 entities, got ${JSON.stringify(entities)}`);
  }

  const first = entities.entities[0];
  const firstFileId = first.latest_file_id;
  if (!first?.entity_id || !firstFileId) {
    throw new Error(`First entity row is not selectable: ${JSON.stringify(first)}`);
  }

  const patchEntity = await request(
    "PATCH",
    `/api/workspaces/${encodeURIComponent(workspaceId)}/entities/${encodeURIComponent(first.entity_id)}`,
    { operator_note: "smoke note", display_name: "Smoke Entity" },
  );
  assertNoErrors("PATCH entity", patchEntity);

  const archiveEntity = await request(
    "DELETE",
    `/api/workspaces/${encodeURIComponent(workspaceId)}/entities/${encodeURIComponent(first.entity_id)}`,
  );
  assertNoErrors("DELETE entity", archiveEntity);
  if (!archiveEntity.payload?.entity?.is_archived) {
    throw new Error(`Entity was not archived: ${JSON.stringify(archiveEntity)}`);
  }

  const afterArchive = await request("GET", `/api/workspaces/${encodeURIComponent(workspaceId)}/entities?limit=50`);
  assertNoErrors("GET entities after archive", afterArchive);
  if (afterArchive.entities?.some((entity) => entity.entity_id === first.entity_id)) {
    throw new Error(`Archived entity was still visible in default list: ${JSON.stringify(afterArchive)}`);
  }

  const restoreEntity = await request(
    "POST",
    `/api/workspaces/${encodeURIComponent(workspaceId)}/entities/${encodeURIComponent(first.entity_id)}/restore`,
  );
  assertNoErrors("POST restore entity", restoreEntity);

  const selectedRun = await request(
    "POST",
    `/api/workspaces/${encodeURIComponent(workspaceId)}/run-selected-pipeline-waves`,
    {
      root_entity_file_ids: [firstFileId],
      max_waves: 1,
      max_tasks_per_wave: 1,
      stop_on_first_failure: true,
    },
  );
  assertNoErrors("POST run-selected-pipeline-waves", selectedRun);

  const deleteStage = await request(
    "DELETE",
    `/api/workspaces/${encodeURIComponent(workspaceId)}/stages/raw_entities`,
  );
  assertNoErrors("DELETE stage", deleteStage);

  const deleteWorkspace = await request("DELETE", `/api/workspaces/${encodeURIComponent(workspaceId)}`);
  assertNoErrors("DELETE workspace", deleteWorkspace);

  console.log(
    JSON.stringify({
      ok: true,
      api_base: apiBase,
      workspace_id: workspaceId,
      workspace_name: workspaceName,
      imported: importBatch.payload?.registered_count ?? 0,
      first_entity_id: first.entity_id,
      first_file_id: firstFileId,
      archived_hidden_by_default: true,
      selected_run_claimed: selectedRun.summary?.total_claimed ?? null,
      stage_delete: deleteStage.payload?.hard_deleted ? "hard_deleted" : "archived",
      workspace_delete: deleteWorkspace.payload?.hard_deleted ? "hard_deleted" : "archived",
    }),
  );
}

main().catch((error) => {
  console.error(JSON.stringify({ ok: false, api_base: apiBase, workspace_name: workspaceName, error: error.message }));
  process.exit(1);
});
