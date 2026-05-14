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
  UpdateWorkspaceRequest,
  ValidatePipelineConfigDraftResult,
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
  listEntityFiles(path: string, entityId?: string | null): Promise<EntityFilesResult>;
  getEntity(
    path: string,
    entityId: string,
    selectedFileId?: number | null,
  ): Promise<EntityDetailResult>;
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
