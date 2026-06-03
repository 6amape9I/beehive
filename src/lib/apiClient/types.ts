import type {
  AppEventsResult,
  BootstrapResult,
  CreateS3StageRequest,
  CreateS3StageResult,
  CreateWorkspaceRequest,
  DashboardOverviewResult,
  EntityDetailResult,
  EntityFileS3JsonResult,
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
  RecoverExpiredWorkerLeasesResult,
  ReconcileStuckTasksResult,
  RegisterS3SourceArtifactRequest,
  RegisterS3SourceArtifactResult,
  ResetEntityStageRequest,
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
  UpdateStageNextStageRequest,
  UpdateStageNextStageResult,
  UpdateEntityRequest,
  UpdateWorkspaceRequest,
  ValidatePipelineConfigDraftResult,
  WorkerLeaseReleaseResult,
  WorkerPoolControlResult,
  WorkerReconcileStuckResult,
  WorkerSummaryResult,
  WorkspaceExplorerResult,
  WorkspaceMutationResult,
  WorkspaceRegistryEntryResult,
  WorkspaceRegistryListResult,
} from "../../types/domain";

export interface BeehiveApiClient {
  initializeWorkdir(path: string): Promise<BootstrapResult>;
  openWorkdir(path: string): Promise<BootstrapResult>;
  reloadWorkdir(path: string): Promise<BootstrapResult>;
  listRegisteredWorkspaces(includeArchived?: boolean): Promise<WorkspaceRegistryListResult>;
  getRegisteredWorkspace(workspaceId: string): Promise<WorkspaceRegistryEntryResult>;
  createRegisteredWorkspace(input: CreateWorkspaceRequest): Promise<WorkspaceMutationResult>;
  updateRegisteredWorkspace(
    workspaceId: string,
    input: UpdateWorkspaceRequest,
  ): Promise<WorkspaceMutationResult>;
  deleteRegisteredWorkspace(workspaceId: string): Promise<WorkspaceMutationResult>;
  restoreRegisteredWorkspace(workspaceId: string): Promise<WorkspaceMutationResult>;
  openRegisteredWorkspace(workspaceId: string): Promise<BootstrapResult>;
  getDashboardOverview(path: string): Promise<DashboardOverviewResult>;
  scanWorkspace(path: string): Promise<ScanWorkspaceResult>;
  reconcileS3Workspace(path: string): Promise<S3ReconciliationResult>;
  reconcileS3WorkspaceById(workspaceId: string): Promise<S3ReconciliationResult>;
  registerS3SourceArtifact(
    path: string,
    input: RegisterS3SourceArtifactRequest,
  ): Promise<RegisterS3SourceArtifactResult>;
  registerS3SourceArtifactById(
    workspaceId: string,
    input: RegisterS3SourceArtifactRequest,
  ): Promise<RegisterS3SourceArtifactResult>;
  ensureStageDirectories(path: string): Promise<StageDirectoryProvisionResult>;
  getRuntimeSummary(path: string): Promise<RuntimeSummaryResult>;
  listStages(path: string): Promise<StageListResult>;
  getPipelineEditorState(path: string): Promise<PipelineEditorStateResult>;
  validatePipelineConfigDraft(
    path: string,
    draft: PipelineConfigDraft,
  ): Promise<ValidatePipelineConfigDraftResult>;
  savePipelineConfig(
    path: string,
    draft: PipelineConfigDraft,
    operatorComment?: string | null,
  ): Promise<SavePipelineConfigResult>;
  createS3Stage(workspaceId: string, input: CreateS3StageRequest): Promise<CreateS3StageResult>;
  updateS3Stage(
    workspaceId: string,
    stageId: string,
    input: UpdateS3StageRequest,
  ): Promise<S3StageMutationResult>;
  deleteS3Stage(workspaceId: string, stageId: string): Promise<S3StageMutationResult>;
  restoreS3Stage(workspaceId: string, stageId: string): Promise<S3StageMutationResult>;
  updateStageNextStage(
    workspaceId: string,
    stageId: string,
    input: UpdateStageNextStageRequest,
  ): Promise<UpdateStageNextStageResult>;
  listEntities(path: string, query?: EntityListQuery): Promise<EntityListResult>;
  listWorkspaceEntities(workspaceId: string, query?: EntityListQuery): Promise<EntityListResult>;
  listEntityFiles(path: string, entityId?: string | null): Promise<EntityFilesResult>;
  getEntity(
    path: string,
    entityId: string,
    selectedFileId?: number | null,
  ): Promise<EntityDetailResult>;
  getWorkspaceEntity(workspaceId: string, entityId: string): Promise<EntityDetailResult>;
  viewWorkspaceEntityFileS3Json(
    workspaceId: string,
    entityFileId: number,
  ): Promise<EntityFileS3JsonResult>;
  resetWorkspaceEntityStageToPending(
    workspaceId: string,
    entityId: string,
    stageId: string,
    input: ResetEntityStageRequest,
  ): Promise<ManualEntityStageActionResult>;
  updateWorkspaceEntity(
    workspaceId: string,
    entityId: string,
    input: UpdateEntityRequest,
  ): Promise<EntityMutationResult>;
  archiveWorkspaceEntity(workspaceId: string, entityId: string): Promise<EntityMutationResult>;
  restoreWorkspaceEntity(workspaceId: string, entityId: string): Promise<EntityMutationResult>;
  importWorkspaceEntitiesJsonBatch(
    workspaceId: string,
    input: ImportJsonBatchRequest,
  ): Promise<ImportJsonBatchResult>;
  createNextStageCopy(
    path: string,
    entityId: string,
    sourceStageId: string,
  ): Promise<FileCopyResult>;
  runDueTasks(path: string): Promise<RunDueTasksResult>;
  runDueTasksLimited(path: string, maxTasks: number): Promise<RunDueTasksResult>;
  runDueTasksLimitedById(workspaceId: string, maxTasks: number): Promise<RunDueTasksResult>;
  runPipelineWaves(
    path: string,
    maxWaves: number,
    maxTasksPerWave: number,
    stopOnFirstFailure: boolean,
  ): Promise<RunPipelineWavesResult>;
  runPipelineWavesById(
    workspaceId: string,
    maxWaves: number,
    maxTasksPerWave: number,
    stopOnFirstFailure: boolean,
  ): Promise<RunPipelineWavesResult>;
  runSelectedPipelineWavesById(
    workspaceId: string,
    rootEntityFileIds: number[],
    maxWaves: number,
    maxTasksPerWave: number,
    stopOnFirstFailure: boolean,
  ): Promise<RunSelectedPipelineWavesResult>;
  getWorkerSummary(workspaceId: string): Promise<WorkerSummaryResult>;
  startWorkers(
    workspaceId: string,
    defaultWorkers: number,
    localLlmWorkers: number,
  ): Promise<WorkerPoolControlResult>;
  stopWorkers(workspaceId: string): Promise<WorkerPoolControlResult>;
  updateWorkerPool(
    workspaceId: string,
    resourceClass: string,
    desiredConcurrency: number,
  ): Promise<WorkerPoolControlResult>;
  recoverExpiredWorkerLeases(workspaceId: string): Promise<RecoverExpiredWorkerLeasesResult>;
  reconcileStuckWorkerStates(workspaceId: string): Promise<WorkerReconcileStuckResult>;
  pauseWorkers(workspaceId: string, reason?: string | null): Promise<WorkerPoolControlResult>;
  resumeWorkers(workspaceId: string): Promise<WorkerPoolControlResult>;
  pauseWorkerPool(
    workspaceId: string,
    resourceClass: string,
    reason?: string | null,
  ): Promise<WorkerPoolControlResult>;
  resumeWorkerPool(workspaceId: string, resourceClass: string): Promise<WorkerPoolControlResult>;
  releaseWorkerLease(
    workspaceId: string,
    leaseId: string,
    reason: string,
  ): Promise<WorkerLeaseReleaseResult>;
  runEntityStage(path: string, entityId: string, stageId: string): Promise<RunEntityStageResult>;
  retryEntityStageNow(
    path: string,
    entityId: string,
    stageId: string,
    operatorComment?: string | null,
  ): Promise<ManualEntityStageActionResult>;
  resetEntityStageToPending(
    path: string,
    entityId: string,
    stageId: string,
    operatorComment?: string | null,
  ): Promise<ManualEntityStageActionResult>;
  skipEntityStage(
    path: string,
    entityId: string,
    stageId: string,
    operatorComment?: string | null,
  ): Promise<ManualEntityStageActionResult>;
  openEntityFile(path: string, entityFileId: number): Promise<OpenEntityPathResult>;
  openEntityFolder(path: string, entityFileId: number): Promise<OpenEntityPathResult>;
  saveEntityFileBusinessJson(
    path: string,
    entityFileId: number,
    payloadJson: string,
    metaJson: string,
    operatorComment?: string | null,
  ): Promise<SaveEntityFileJsonResult>;
  listStageRuns(path: string, entityId?: string | null): Promise<StageRunsResult>;
  listStageRunOutputs(workspaceId: string, runId: string): Promise<StageRunOutputsResult>;
  reconcileStuckTasks(path: string): Promise<ReconcileStuckTasksResult>;
  listAppEvents(path: string, limit?: number): Promise<AppEventsResult>;
  getWorkspaceExplorer(path: string): Promise<WorkspaceExplorerResult>;
  getWorkspaceExplorerById(workspaceId: string): Promise<WorkspaceExplorerResult>;
}
