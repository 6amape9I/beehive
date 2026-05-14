import { apiClient } from "./apiClient";
import type {
  AppEventsResult,
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
  UpdateStageNextStageRequest,
  UpdateStageNextStageResult,
  UpdateS3StageRequest,
  ValidatePipelineConfigDraftResult,
  WorkspaceExplorerResult,
} from "../types/domain";

export function getDashboardOverview(path: string): Promise<DashboardOverviewResult> {
  return apiClient.getDashboardOverview(path);
}

export function scanWorkspace(path: string): Promise<ScanWorkspaceResult> {
  return apiClient.scanWorkspace(path);
}

export function reconcileS3Workspace(path: string): Promise<S3ReconciliationResult> {
  return apiClient.reconcileS3Workspace(path);
}

export function reconcileS3WorkspaceById(workspaceId: string): Promise<S3ReconciliationResult> {
  return apiClient.reconcileS3WorkspaceById(workspaceId);
}

export function registerS3SourceArtifact(
  path: string,
  input: RegisterS3SourceArtifactRequest,
): Promise<RegisterS3SourceArtifactResult> {
  return apiClient.registerS3SourceArtifact(path, input);
}

export function registerS3SourceArtifactById(
  workspaceId: string,
  input: RegisterS3SourceArtifactRequest,
): Promise<RegisterS3SourceArtifactResult> {
  return apiClient.registerS3SourceArtifactById(workspaceId, input);
}

export function ensureStageDirectories(path: string): Promise<StageDirectoryProvisionResult> {
  return apiClient.ensureStageDirectories(path);
}

export function getRuntimeSummary(path: string): Promise<RuntimeSummaryResult> {
  return apiClient.getRuntimeSummary(path);
}

export function listStages(path: string): Promise<StageListResult> {
  return apiClient.listStages(path);
}

export function getPipelineEditorState(path: string): Promise<PipelineEditorStateResult> {
  return apiClient.getPipelineEditorState(path);
}

export function validatePipelineConfigDraft(
  path: string,
  draft: PipelineConfigDraft,
): Promise<ValidatePipelineConfigDraftResult> {
  return apiClient.validatePipelineConfigDraft(path, draft);
}

export function savePipelineConfig(
  path: string,
  draft: PipelineConfigDraft,
  operatorComment?: string | null,
): Promise<SavePipelineConfigResult> {
  return apiClient.savePipelineConfig(path, draft, operatorComment);
}

export function createS3Stage(
  workspaceId: string,
  input: CreateS3StageRequest,
): Promise<CreateS3StageResult> {
  return apiClient.createS3Stage(workspaceId, input);
}

export function updateS3Stage(
  workspaceId: string,
  stageId: string,
  input: UpdateS3StageRequest,
): Promise<S3StageMutationResult> {
  return apiClient.updateS3Stage(workspaceId, stageId, input);
}

export function deleteS3Stage(
  workspaceId: string,
  stageId: string,
): Promise<S3StageMutationResult> {
  return apiClient.deleteS3Stage(workspaceId, stageId);
}

export function restoreS3Stage(
  workspaceId: string,
  stageId: string,
): Promise<S3StageMutationResult> {
  return apiClient.restoreS3Stage(workspaceId, stageId);
}

export function updateStageNextStage(
  workspaceId: string,
  stageId: string,
  input: UpdateStageNextStageRequest,
): Promise<UpdateStageNextStageResult> {
  return apiClient.updateStageNextStage(workspaceId, stageId, input);
}

export function listEntities(path: string, query?: EntityListQuery): Promise<EntityListResult> {
  return apiClient.listEntities(path, query);
}

export function listEntityFiles(
  path: string,
  entityId?: string | null,
): Promise<EntityFilesResult> {
  return apiClient.listEntityFiles(path, entityId);
}

export function getEntity(
  path: string,
  entityId: string,
  selectedFileId?: number | null,
): Promise<EntityDetailResult> {
  return apiClient.getEntity(path, entityId, selectedFileId);
}

export function createNextStageCopy(
  path: string,
  entityId: string,
  sourceStageId: string,
): Promise<FileCopyResult> {
  return apiClient.createNextStageCopy(path, entityId, sourceStageId);
}

export function runDueTasks(path: string): Promise<RunDueTasksResult> {
  return apiClient.runDueTasks(path);
}

export function runDueTasksLimited(path: string, maxTasks: number): Promise<RunDueTasksResult> {
  return apiClient.runDueTasksLimited(path, maxTasks);
}

export function runDueTasksLimitedById(
  workspaceId: string,
  maxTasks: number,
): Promise<RunDueTasksResult> {
  return apiClient.runDueTasksLimitedById(workspaceId, maxTasks);
}

export function runPipelineWaves(
  path: string,
  maxWaves: number,
  maxTasksPerWave: number,
  stopOnFirstFailure: boolean,
): Promise<RunPipelineWavesResult> {
  return apiClient.runPipelineWaves(path, maxWaves, maxTasksPerWave, stopOnFirstFailure);
}

export function runPipelineWavesById(
  workspaceId: string,
  maxWaves: number,
  maxTasksPerWave: number,
  stopOnFirstFailure: boolean,
): Promise<RunPipelineWavesResult> {
  return apiClient.runPipelineWavesById(
    workspaceId,
    maxWaves,
    maxTasksPerWave,
    stopOnFirstFailure,
  );
}

export function runSelectedPipelineWavesById(
  workspaceId: string,
  rootEntityFileIds: number[],
  maxWaves: number,
  maxTasksPerWave: number,
  stopOnFirstFailure: boolean,
): Promise<RunSelectedPipelineWavesResult> {
  return apiClient.runSelectedPipelineWavesById(
    workspaceId,
    rootEntityFileIds,
    maxWaves,
    maxTasksPerWave,
    stopOnFirstFailure,
  );
}

export function runEntityStage(
  path: string,
  entityId: string,
  stageId: string,
): Promise<RunEntityStageResult> {
  return apiClient.runEntityStage(path, entityId, stageId);
}

export function retryEntityStageNow(
  path: string,
  entityId: string,
  stageId: string,
  operatorComment?: string | null,
): Promise<ManualEntityStageActionResult> {
  return apiClient.retryEntityStageNow(path, entityId, stageId, operatorComment);
}

export function resetEntityStageToPending(
  path: string,
  entityId: string,
  stageId: string,
  operatorComment?: string | null,
): Promise<ManualEntityStageActionResult> {
  return apiClient.resetEntityStageToPending(path, entityId, stageId, operatorComment);
}

export function skipEntityStage(
  path: string,
  entityId: string,
  stageId: string,
  operatorComment?: string | null,
): Promise<ManualEntityStageActionResult> {
  return apiClient.skipEntityStage(path, entityId, stageId, operatorComment);
}

export function openEntityFile(
  path: string,
  entityFileId: number,
): Promise<OpenEntityPathResult> {
  return apiClient.openEntityFile(path, entityFileId);
}

export function openEntityFolder(
  path: string,
  entityFileId: number,
): Promise<OpenEntityPathResult> {
  return apiClient.openEntityFolder(path, entityFileId);
}

export function saveEntityFileBusinessJson(
  path: string,
  entityFileId: number,
  payloadJson: string,
  metaJson: string,
  operatorComment?: string | null,
): Promise<SaveEntityFileJsonResult> {
  return apiClient.saveEntityFileBusinessJson(
    path,
    entityFileId,
    payloadJson,
    metaJson,
    operatorComment,
  );
}

export function listStageRuns(path: string, entityId?: string | null): Promise<StageRunsResult> {
  return apiClient.listStageRuns(path, entityId);
}

export function listStageRunOutputs(
  workspaceId: string,
  runId: string,
): Promise<StageRunOutputsResult> {
  return apiClient.listStageRunOutputs(workspaceId, runId);
}

export function reconcileStuckTasks(path: string): Promise<ReconcileStuckTasksResult> {
  return apiClient.reconcileStuckTasks(path);
}

export function listAppEvents(path: string, limit = 50): Promise<AppEventsResult> {
  return apiClient.listAppEvents(path, limit);
}

export function getWorkspaceExplorer(path: string): Promise<WorkspaceExplorerResult> {
  return apiClient.getWorkspaceExplorer(path);
}

export function getWorkspaceExplorerById(workspaceId: string): Promise<WorkspaceExplorerResult> {
  return apiClient.getWorkspaceExplorerById(workspaceId);
}
