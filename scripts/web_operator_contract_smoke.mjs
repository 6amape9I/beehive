#!/usr/bin/env node

import http from "node:http";

const apiBase = (process.env.BEEHIVE_API_BASE_URL ?? "http://127.0.0.1:8787").replace(/\/+$/, "");
const token = process.env.BEEHIVE_OPERATOR_TOKEN ?? "";
const suffix = `${Date.now()}-${Math.random().toString(16).slice(2, 8)}`;
const workspaceId = process.env.BEEHIVE_CONTRACT_SMOKE_WORKSPACE_ID ?? `b10-contract-${suffix}`;
const sourceEntityId = "symptom_Кольца_Кайзера-Флейшера_e74b3ffa92f0";

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
    throw new Error(`${method} ${path} failed with HTTP ${response.status}: ${JSON.stringify(payload)}`);
  }
  return payload;
}

function assertNoErrors(label, payload) {
  const errors = payload?.errors ?? [];
  if (errors.length > 0) {
    throw new Error(`${label} returned errors: ${JSON.stringify(errors)}`);
  }
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function readJsonBody(req) {
  return new Promise((resolve, reject) => {
    let raw = "";
    req.setEncoding("utf8");
    req.on("data", (chunk) => {
      raw += chunk;
    });
    req.on("end", () => {
      try {
        resolve(raw ? JSON.parse(raw) : {});
      } catch (error) {
        reject(error);
      }
    });
    req.on("error", reject);
  });
}

function startMockWebhook({ stageAPath, stageBPath }) {
  const requests = [];
  const server = http.createServer(async (req, res) => {
    try {
      const body = await readJsonBody(req);
      requests.push(body);
      const sourceBucket = body.source_bucket;
      const sourceKey = body.source_key;
      const runId = body.run_id;
      const workspace = body.workspace_id;
      const createdAt = new Date().toISOString();
      const manifest = {
        schema: "beehive.s3_artifact_manifest.v1",
        workspace_id: workspace,
        run_id: runId,
        source: {
          bucket: sourceBucket,
          key: sourceKey,
          version_id: body.source_version_id ?? null,
          etag: body.source_etag ?? null,
        },
        status: "success",
        outputs: [
          {
            artifact_id: "artifact-a",
            entity_id: "child-А",
            relation_to_source: "child_entity",
            bucket: sourceBucket,
            key: `${stageAPath}/artifact-a.json`,
            save_path: stageAPath,
            content_type: "application/json",
            checksum_sha256: null,
            size: 12,
          },
          {
            artifact_id: "artifact-b",
            entity_id: "child-Б",
            relation_to_source: "child_entity",
            bucket: sourceBucket,
            key: `${stageBPath}/artifact-b.json`,
            save_path: stageBPath,
            content_type: "application/json",
            checksum_sha256: null,
            size: 13,
          },
          {
            artifact_id: "artifact-conflict",
            entity_id: "child-А",
            relation_to_source: "child_entity",
            bucket: sourceBucket,
            key: `${stageAPath}/artifact-conflict.json`,
            save_path: stageAPath,
            content_type: "application/json",
            checksum_sha256: null,
            size: 14,
          },
        ],
        created_at: createdAt,
      };
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(JSON.stringify(manifest));
    } catch (error) {
      res.writeHead(500, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ error: String(error) }));
    }
  });

  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      resolve({
        url: `http://127.0.0.1:${address.port}/webhook/b10-contract`,
        requests,
        close: () => new Promise((closeResolve) => server.close(closeResolve)),
      });
    });
  });
}

async function main() {
  let mock = null;
  try {
    const health = await request("GET", "/api/health");
    assert(health.status === "ok", `Unexpected health response: ${JSON.stringify(health)}`);

    const createWorkspace = await request("POST", "/api/workspaces", {
      id: workspaceId,
      name: "B10 Contract Smoke",
      bucket: "contract-smoke-bucket",
      workspace_prefix: `beehive-contract-smoke/${workspaceId}`,
      region: "test-region",
      endpoint: "https://s3.example.test",
    });
    assertNoErrors("POST workspace", createWorkspace);

    const createStageA = await request("POST", `/api/workspaces/${encodeURIComponent(workspaceId)}/stages`, {
      stage_id: "stage_a",
      workflow_url: "http://127.0.0.1:9/not-ready",
      max_attempts: 1,
      retry_delay_sec: 0,
      allow_zero_outputs: false,
      allow_multiple_outputs: true,
    });
    assertNoErrors("POST stage_a", createStageA);

    const createStageB = await request("POST", `/api/workspaces/${encodeURIComponent(workspaceId)}/stages`, {
      stage_id: "stage_b",
      workflow_url: "https://n8n.example.test/webhook/stage-b",
      max_attempts: 1,
      retry_delay_sec: 0,
      allow_zero_outputs: false,
      allow_multiple_outputs: false,
    });
    assertNoErrors("POST stage_b", createStageB);

    const createStageC = await request("POST", `/api/workspaces/${encodeURIComponent(workspaceId)}/stages`, {
      stage_id: "stage_c",
      workflow_url: "https://n8n.example.test/webhook/stage-c",
      max_attempts: 1,
      retry_delay_sec: 0,
      allow_zero_outputs: false,
      allow_multiple_outputs: false,
    });
    assertNoErrors("POST stage_c", createStageC);

    const stageAPath = createStageB.payload.route_hints.save_path_aliases[0];
    const stageBPath = createStageC.payload.route_hints.save_path_aliases[0];
    mock = await startMockWebhook({ stageAPath, stageBPath });

    const updateStageA = await request(
      "PATCH",
      `/api/workspaces/${encodeURIComponent(workspaceId)}/stages/stage_a`,
      {
        workflow_url: mock.url,
        max_attempts: 1,
        retry_delay_sec: 0,
        allow_zero_outputs: false,
        allow_multiple_outputs: true,
      },
    );
    assertNoErrors("PATCH stage_a", updateStageA);

    const sourceStagePath = createStageA.payload.route_hints.save_path_aliases[0];
    const sourceKey = `${sourceStagePath}/source-${sourceEntityId}.json`;
    const source = await request(
      "POST",
      `/api/workspaces/${encodeURIComponent(workspaceId)}/register-s3-source`,
      {
        stage_id: "stage_a",
        entity_id: sourceEntityId,
        artifact_id: "source-artifact",
        bucket: "contract-smoke-bucket",
        key: sourceKey,
        size: 123,
      },
    );
    assertNoErrors("POST register-s3-source", source);
    const sourceFileId = source.payload?.file?.id;
    assert(sourceFileId, `Source file id missing: ${JSON.stringify(source)}`);

    const search = new URLSearchParams({ search: "Кольца", limit: "10" });
    const entities = await request(
      "GET",
      `/api/workspaces/${encodeURIComponent(workspaceId)}/entities?${search.toString()}`,
    );
    assertNoErrors("GET entities search", entities);
    assert(
      entities.entities.some((entity) => entity.entity_id === sourceEntityId),
      `Cyrillic query search did not return source entity: ${JSON.stringify(entities)}`,
    );

    const entityPath = `/api/workspaces/${encodeURIComponent(workspaceId)}/entities/${encodeURIComponent(sourceEntityId)}`;
    const detail = await request("GET", entityPath);
    assertNoErrors("GET Cyrillic entity", detail);
    assert(detail.detail?.entity?.entity_id === sourceEntityId, `Unexpected entity detail: ${JSON.stringify(detail)}`);

    const run = await request(
      "POST",
      `/api/workspaces/${encodeURIComponent(workspaceId)}/run-selected-pipeline-waves`,
      {
        root_entity_file_ids: [sourceFileId],
        max_waves: 1,
        max_tasks_per_wave: 1,
        stop_on_first_failure: true,
      },
    );
    assertNoErrors("POST run-selected-pipeline-waves", run);
    assert(run.summary?.total_succeeded === 1, `Expected one successful source run: ${JSON.stringify(run)}`);
    assert(run.summary?.total_blocked === 0, `Expected no blocked run: ${JSON.stringify(run)}`);
    assert(run.summary?.root_results?.[0]?.output_count === 2, `Expected two registered outputs: ${JSON.stringify(run)}`);
    assert(run.summary?.output_tree?.length === 2, `Expected two output nodes: ${JSON.stringify(run)}`);
    assert(mock.requests.length === 1, `Expected one mock webhook request, got ${mock.requests.length}`);
    assert(mock.requests[0].source_key === sourceKey, `Mock received wrong source key: ${JSON.stringify(mock.requests[0])}`);

    const runId = run.summary.root_results[0].run_ids[0];
    const outputs = await request(
      "GET",
      `/api/workspaces/${encodeURIComponent(workspaceId)}/stage-runs/${encodeURIComponent(runId)}/outputs`,
    );
    assertNoErrors("GET stage run outputs", outputs);
    assert(outputs.payload?.outputs?.length === 2, `Expected two persisted run outputs: ${JSON.stringify(outputs)}`);

    const patchEntity = await request("PATCH", entityPath, {
      display_name: "Кольца Кайзера-Флейшера",
      operator_note: "B10 URL decode smoke",
    });
    assertNoErrors("PATCH Cyrillic entity", patchEntity);
    const archiveEntity = await request("DELETE", entityPath);
    assertNoErrors("DELETE Cyrillic entity", archiveEntity);
    const restoreEntity = await request("POST", `${entityPath}/restore`);
    assertNoErrors("POST restore Cyrillic entity", restoreEntity);

    const deleteWorkspace = await request("DELETE", `/api/workspaces/${encodeURIComponent(workspaceId)}`);
    assertNoErrors("DELETE workspace", deleteWorkspace);

    console.log(
      JSON.stringify({
        ok: true,
        api_base: apiBase,
        workspace_id: workspaceId,
        source_entity_id: sourceEntityId,
        run_id: runId,
        registered_outputs: outputs.payload.outputs.length,
        mock_requests: mock.requests.length,
        workspace_cleanup: deleteWorkspace.payload?.hard_deleted
          ? "hard_deleted"
          : deleteWorkspace.payload?.archived
            ? "archived"
            : "unknown",
      }),
    );
  } finally {
    if (mock) await mock.close();
  }
}

main().catch(async (error) => {
  if (error?.stack) {
    console.error(error.stack);
  } else {
    console.error(error);
  }
  try {
    await request("DELETE", `/api/workspaces/${encodeURIComponent(workspaceId)}`);
  } catch {
    // Best-effort cleanup only.
  }
  process.exit(1);
});
