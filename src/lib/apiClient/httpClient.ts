import type {
  AppEventsResult,
  BootstrapResult,
  CreateS3StageRequest,
  CreateS3StageResult,
  CreateWorkspaceRequest,
  DashboardOverviewResult,
  EntityDetailResult,
  EntityFilesResult,
  EntityListQuery,
  EntityListResult,
  EntityMutationResult,
  FileCopyResult,
  ImportJsonBatchRequest,
  ImportJsonBatchResult,
  ManualEntityStageActionResult,
  OpenEntityPathResult,
  PipelineConfigDraft,
  PipelineEditorStateResult,
  ReconcileStuckTasksResult,
  RegisterS3SourceArtifactRequest,
  RegisterS3SourceArtifactResult,
  RunDueTasksResult,
  RunEntityStageResult,
  RunPipelineWavesResult,
  RunSelectedPipelineWavesResult,
  RuntimeSummaryResult,
  S3ReconciliationResult,
  S3StageMutationResult,
  SaveEntityFileJsonResult,
  SavePipelineConfigResult,
  ScanWorkspaceResult,
  StageDirectoryProvisionResult,
  StageListResult,
  StageRunOutputsResult,
  StageRunsResult,
  UpdateS3StageRequest,
  UpdateEntityRequest,
  UpdateStageNextStageRequest,
  UpdateStageNextStageResult,
  UpdateWorkspaceRequest,
  ValidatePipelineConfigDraftResult,
  WorkspaceExplorerResult,
  WorkspaceMutationResult,
  WorkspaceRegistryEntryResult,
  WorkspaceRegistryListResult,
} from "../../types/domain";
import type { BeehiveApiClient } from "./types";

export function createHttpClient(apiBaseUrl: string): BeehiveApiClient {
  const apiBase = apiBaseUrl.replace(/\/+$/, "");
  const viteEnv = (import.meta as ImportMeta & {
    env?: Record<string, string | undefined>;
  }).env;
  function configuredToken(): string | undefined {
    return (
      viteEnv?.VITE_BEEHIVE_OPERATOR_TOKEN ??
      (typeof window !== "undefined"
        ? window.localStorage.getItem("BEEHIVE_OPERATOR_TOKEN") ?? undefined
        : undefined)
    );
  }

  async function fetchJson<T>(path: string, init?: RequestInit): Promise<T> {
    const token = configuredToken();
    const response = await fetch(`${apiBase}${path}`, {
      ...init,
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
        ...(init?.headers ?? {}),
      },
    });
    const payload = (await response.json()) as T;
    if (!response.ok) {
      throw new Error(`HTTP ${response.status} for ${path}`);
    }
    return payload;
  }

  function postJson<T>(path: string, body?: unknown): Promise<T> {
    return fetchJson<T>(path, {
      method: "POST",
      body: JSON.stringify(body ?? {}),
    });
  }

  function patchJson<T>(path: string, body?: unknown): Promise<T> {
    return fetchJson<T>(path, {
      method: "PATCH",
      body: JSON.stringify(body ?? {}),
    });
  }

  function deleteJson<T>(path: string): Promise<T> {
    return fetchJson<T>(path, { method: "DELETE" });
  }

  function entityQueryString(query?: EntityListQuery): string {
    const params = new URLSearchParams();
    if (query?.search) params.set("search", query.search);
    if (query?.stage_id) params.set("stage_id", query.stage_id);
    if (query?.status) params.set("status", query.status);
    if (query?.include_archived) params.set("include_archived", "true");
    if (query?.limit) params.set("limit", String(query.limit));
    if (query?.offset) params.set("offset", String(query.offset));
    if (query?.page) params.set("page", String(query.page));
    if (query?.page_size) params.set("page_size", String(query.page_size));
    if (query?.sort_by) params.set("sort_by", query.sort_by);
    if (query?.sort_direction) params.set("sort_direction", query.sort_direction);
    const text = params.toString();
    return text ? `?${text}` : "";
  }

  function unsupported<T>(name: string): Promise<T> {
    return Promise.reject(new Error(`${name} is not available in HTTP client yet.`));
  }

  return {
    initializeWorkdir: (path: string): Promise<BootstrapResult> =>
      unsupported(`initializeWorkdir(${path})`),
    openWorkdir: (path: string): Promise<BootstrapResult> => unsupported(`openWorkdir(${path})`),
    reloadWorkdir: (path: string): Promise<BootstrapResult> =>
      unsupported(`reloadWorkdir(${path})`),
    listRegisteredWorkspaces: (includeArchived = false): Promise<WorkspaceRegistryListResult> =>
      fetchJson(`/api/workspaces?include_archived=${includeArchived ? "true" : "false"}`),
    getRegisteredWorkspace: (workspaceId: string): Promise<WorkspaceRegistryEntryResult> =>
      fetchJson(`/api/workspaces/${encodeURIComponent(workspaceId)}`),
    createRegisteredWorkspace: (input: CreateWorkspaceRequest): Promise<WorkspaceMutationResult> =>
      postJson("/api/workspaces", input),
    updateRegisteredWorkspace: (
      workspaceId: string,
      input: UpdateWorkspaceRequest,
    ): Promise<WorkspaceMutationResult> =>
      patchJson(`/api/workspaces/${encodeURIComponent(workspaceId)}`, input),
    deleteRegisteredWorkspace: (workspaceId: string): Promise<WorkspaceMutationResult> =>
      deleteJson(`/api/workspaces/${encodeURIComponent(workspaceId)}`),
    restoreRegisteredWorkspace: (workspaceId: string): Promise<WorkspaceMutationResult> =>
      postJson(`/api/workspaces/${encodeURIComponent(workspaceId)}/restore`),
    openRegisteredWorkspace: (workspaceId: string): Promise<BootstrapResult> =>
      unsupported(`openRegisteredWorkspace(${workspaceId})`),
    getDashboardOverview: (path: string): Promise<DashboardOverviewResult> =>
      unsupported(`getDashboardOverview(${path})`),
    scanWorkspace: (path: string): Promise<ScanWorkspaceResult> =>
      unsupported(`scanWorkspace(${path})`),
    reconcileS3Workspace: (workspaceId: string): Promise<S3ReconciliationResult> =>
      postJson(`/api/workspaces/${encodeURIComponent(workspaceId)}/reconcile-s3`),
    reconcileS3WorkspaceById: (workspaceId: string): Promise<S3ReconciliationResult> =>
      postJson(`/api/workspaces/${encodeURIComponent(workspaceId)}/reconcile-s3`),
    registerS3SourceArtifact: (
      workspaceId: string,
      input: RegisterS3SourceArtifactRequest,
    ): Promise<RegisterS3SourceArtifactResult> =>
      postJson(`/api/workspaces/${encodeURIComponent(workspaceId)}/register-s3-source`, input),
    registerS3SourceArtifactById: (
      workspaceId: string,
      input: RegisterS3SourceArtifactRequest,
    ): Promise<RegisterS3SourceArtifactResult> =>
      postJson(`/api/workspaces/${encodeURIComponent(workspaceId)}/register-s3-source`, input),
    ensureStageDirectories: (path: string): Promise<StageDirectoryProvisionResult> =>
      unsupported(`ensureStageDirectories(${path})`),
    getRuntimeSummary: (path: string): Promise<RuntimeSummaryResult> =>
      unsupported(`getRuntimeSummary(${path})`),
    listStages: (path: string): Promise<StageListResult> => unsupported(`listStages(${path})`),
    getPipelineEditorState: (path: string): Promise<PipelineEditorStateResult> =>
      unsupported(`getPipelineEditorState(${path})`),
    validatePipelineConfigDraft: (
      path: string,
      _draft: PipelineConfigDraft,
    ): Promise<ValidatePipelineConfigDraftResult> =>
      unsupported(`validatePipelineConfigDraft(${path})`),
    savePipelineConfig: (
      path: string,
      _draft: PipelineConfigDraft,
      _operatorComment?: string | null,
    ): Promise<SavePipelineConfigResult> => unsupported(`savePipelineConfig(${path})`),
    createS3Stage: (
      workspaceId: string,
      input: CreateS3StageRequest,
    ): Promise<CreateS3StageResult> =>
      postJson(`/api/workspaces/${encodeURIComponent(workspaceId)}/stages`, input),
    updateS3Stage: (
      workspaceId: string,
      stageId: string,
      input: UpdateS3StageRequest,
    ): Promise<S3StageMutationResult> =>
      patchJson(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/stages/${encodeURIComponent(stageId)}`,
        input,
      ),
    deleteS3Stage: (workspaceId: string, stageId: string): Promise<S3StageMutationResult> =>
      deleteJson(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/stages/${encodeURIComponent(stageId)}`,
      ),
    restoreS3Stage: (workspaceId: string, stageId: string): Promise<S3StageMutationResult> =>
      postJson(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/stages/${encodeURIComponent(stageId)}/restore`,
      ),
    updateStageNextStage: (
      workspaceId: string,
      stageId: string,
      input: UpdateStageNextStageRequest,
    ): Promise<UpdateStageNextStageResult> =>
      postJson(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/stages/${encodeURIComponent(stageId)}/next-stage`,
        input,
      ),
    listEntities: (workspaceId: string, query?: EntityListQuery): Promise<EntityListResult> =>
      fetchJson(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/entities${entityQueryString(query)}`,
      ),
    listWorkspaceEntities: (
      workspaceId: string,
      query?: EntityListQuery,
    ): Promise<EntityListResult> =>
      fetchJson(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/entities${entityQueryString(query)}`,
      ),
    listEntityFiles: (path: string, _entityId?: string | null): Promise<EntityFilesResult> =>
      unsupported(`listEntityFiles(${path})`),
    getEntity: (
      workspaceId: string,
      entityId: string,
      _selectedFileId?: number | null,
    ): Promise<EntityDetailResult> =>
      fetchJson(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/entities/${encodeURIComponent(entityId)}`,
      ),
    getWorkspaceEntity: (workspaceId: string, entityId: string): Promise<EntityDetailResult> =>
      fetchJson(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/entities/${encodeURIComponent(entityId)}`,
      ),
    updateWorkspaceEntity: (
      workspaceId: string,
      entityId: string,
      input: UpdateEntityRequest,
    ): Promise<EntityMutationResult> =>
      patchJson(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/entities/${encodeURIComponent(entityId)}`,
        input,
      ),
    archiveWorkspaceEntity: (
      workspaceId: string,
      entityId: string,
    ): Promise<EntityMutationResult> =>
      deleteJson(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/entities/${encodeURIComponent(entityId)}`,
      ),
    restoreWorkspaceEntity: (
      workspaceId: string,
      entityId: string,
    ): Promise<EntityMutationResult> =>
      postJson(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/entities/${encodeURIComponent(entityId)}/restore`,
      ),
    importWorkspaceEntitiesJsonBatch: (
      workspaceId: string,
      input: ImportJsonBatchRequest,
    ): Promise<ImportJsonBatchResult> =>
      postJson(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/entities/import-json-batch`,
        input,
      ),
    createNextStageCopy: (
      path: string,
      _entityId: string,
      _sourceStageId: string,
    ): Promise<FileCopyResult> => unsupported(`createNextStageCopy(${path})`),
    runDueTasks: (path: string): Promise<RunDueTasksResult> => unsupported(`runDueTasks(${path})`),
    runDueTasksLimited: (workspaceId: string, maxTasks: number): Promise<RunDueTasksResult> =>
      postJson(`/api/workspaces/${encodeURIComponent(workspaceId)}/run-small-batch`, {
        max_tasks: maxTasks,
      }),
    runDueTasksLimitedById: (workspaceId: string, maxTasks: number): Promise<RunDueTasksResult> =>
      postJson(`/api/workspaces/${encodeURIComponent(workspaceId)}/run-small-batch`, {
        max_tasks: maxTasks,
      }),
    runPipelineWaves: (
      workspaceId: string,
      maxWaves: number,
      maxTasksPerWave: number,
      stopOnFirstFailure: boolean,
    ): Promise<RunPipelineWavesResult> =>
      postJson(`/api/workspaces/${encodeURIComponent(workspaceId)}/run-pipeline-waves`, {
        max_waves: maxWaves,
        max_tasks_per_wave: maxTasksPerWave,
        stop_on_first_failure: stopOnFirstFailure,
      }),
    runPipelineWavesById: (
      workspaceId: string,
      maxWaves: number,
      maxTasksPerWave: number,
      stopOnFirstFailure: boolean,
    ): Promise<RunPipelineWavesResult> =>
      postJson(`/api/workspaces/${encodeURIComponent(workspaceId)}/run-pipeline-waves`, {
        max_waves: maxWaves,
        max_tasks_per_wave: maxTasksPerWave,
        stop_on_first_failure: stopOnFirstFailure,
      }),
    runSelectedPipelineWavesById: (
      workspaceId: string,
      rootEntityFileIds: number[],
      maxWaves: number,
      maxTasksPerWave: number,
      stopOnFirstFailure: boolean,
    ): Promise<RunSelectedPipelineWavesResult> =>
      postJson(`/api/workspaces/${encodeURIComponent(workspaceId)}/run-selected-pipeline-waves`, {
        root_entity_file_ids: rootEntityFileIds,
        max_waves: maxWaves,
        max_tasks_per_wave: maxTasksPerWave,
        stop_on_first_failure: stopOnFirstFailure,
      }),
    runEntityStage: (
      path: string,
      _entityId: string,
      _stageId: string,
    ): Promise<RunEntityStageResult> => unsupported(`runEntityStage(${path})`),
    retryEntityStageNow: (
      path: string,
      _entityId: string,
      _stageId: string,
      _operatorComment?: string | null,
    ): Promise<ManualEntityStageActionResult> => unsupported(`retryEntityStageNow(${path})`),
    resetEntityStageToPending: (
      path: string,
      _entityId: string,
      _stageId: string,
      _operatorComment?: string | null,
    ): Promise<ManualEntityStageActionResult> =>
      unsupported(`resetEntityStageToPending(${path})`),
    skipEntityStage: (
      path: string,
      _entityId: string,
      _stageId: string,
      _operatorComment?: string | null,
    ): Promise<ManualEntityStageActionResult> => unsupported(`skipEntityStage(${path})`),
    openEntityFile: (path: string, _entityFileId: number): Promise<OpenEntityPathResult> =>
      unsupported(`openEntityFile(${path})`),
    openEntityFolder: (path: string, _entityFileId: number): Promise<OpenEntityPathResult> =>
      unsupported(`openEntityFolder(${path})`),
    saveEntityFileBusinessJson: (
      path: string,
      _entityFileId: number,
      _payloadJson: string,
      _metaJson: string,
      _operatorComment?: string | null,
    ): Promise<SaveEntityFileJsonResult> => unsupported(`saveEntityFileBusinessJson(${path})`),
    listStageRuns: (path: string, _entityId?: string | null): Promise<StageRunsResult> =>
      unsupported(`listStageRuns(${path})`),
    listStageRunOutputs: (workspaceId: string, runId: string): Promise<StageRunOutputsResult> =>
      fetchJson(
        `/api/workspaces/${encodeURIComponent(workspaceId)}/stage-runs/${encodeURIComponent(runId)}/outputs`,
      ),
    reconcileStuckTasks: (path: string): Promise<ReconcileStuckTasksResult> =>
      unsupported(`reconcileStuckTasks(${path})`),
    listAppEvents: (path: string, _limit?: number): Promise<AppEventsResult> =>
      unsupported(`listAppEvents(${path})`),
    getWorkspaceExplorer: (workspaceId: string): Promise<WorkspaceExplorerResult> =>
      fetchJson(`/api/workspaces/${encodeURIComponent(workspaceId)}/workspace-explorer`),
    getWorkspaceExplorerById: (workspaceId: string): Promise<WorkspaceExplorerResult> =>
      fetchJson(`/api/workspaces/${encodeURIComponent(workspaceId)}/workspace-explorer`),
  };
}
