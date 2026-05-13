import type {
  AppEventsResult,
  BootstrapResult,
  CreateS3StageRequest,
  CreateS3StageResult,
  DashboardOverviewResult,
  EntityDetailResult,
  EntityFilesResult,
  EntityListQuery,
  EntityListResult,
  FileCopyResult,
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
  RuntimeSummaryResult,
  S3ReconciliationResult,
  SaveEntityFileJsonResult,
  SavePipelineConfigResult,
  ScanWorkspaceResult,
  StageDirectoryProvisionResult,
  StageListResult,
  StageRunOutputsResult,
  StageRunsResult,
  ValidatePipelineConfigDraftResult,
  WorkspaceExplorerResult,
  WorkspaceRegistryEntryResult,
  WorkspaceRegistryListResult,
} from "../../types/domain";
import type { BeehiveApiClient } from "./types";

export function createHttpClient(apiBaseUrl: string): BeehiveApiClient {
  const apiBase = apiBaseUrl.replace(/\/+$/, "");

  async function fetchJson<T>(path: string, init?: RequestInit): Promise<T> {
    const response = await fetch(`${apiBase}${path}`, {
      ...init,
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
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

  function unsupported<T>(name: string): Promise<T> {
    return Promise.reject(new Error(`${name} is not available in HTTP client yet.`));
  }

  return {
    initializeWorkdir: (path: string): Promise<BootstrapResult> =>
      unsupported(`initializeWorkdir(${path})`),
    openWorkdir: (path: string): Promise<BootstrapResult> => unsupported(`openWorkdir(${path})`),
    reloadWorkdir: (path: string): Promise<BootstrapResult> =>
      unsupported(`reloadWorkdir(${path})`),
    listRegisteredWorkspaces: (): Promise<WorkspaceRegistryListResult> =>
      fetchJson("/api/workspaces"),
    getRegisteredWorkspace: (workspaceId: string): Promise<WorkspaceRegistryEntryResult> =>
      fetchJson(`/api/workspaces/${encodeURIComponent(workspaceId)}`),
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
    listEntities: (path: string, _query?: EntityListQuery): Promise<EntityListResult> =>
      unsupported(`listEntities(${path})`),
    listEntityFiles: (path: string, _entityId?: string | null): Promise<EntityFilesResult> =>
      unsupported(`listEntityFiles(${path})`),
    getEntity: (
      path: string,
      _entityId: string,
      _selectedFileId?: number | null,
    ): Promise<EntityDetailResult> => unsupported(`getEntity(${path})`),
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
