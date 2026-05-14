import { invoke } from "@tauri-apps/api/core";

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
import type { BeehiveApiClient } from "./types";

export const tauriClient: BeehiveApiClient = {
  initializeWorkdir(path: string): Promise<BootstrapResult> {
    return invoke<BootstrapResult>("initialize_workdir", { path });
  },
  openWorkdir(path: string): Promise<BootstrapResult> {
    return invoke<BootstrapResult>("open_workdir", { path });
  },
  reloadWorkdir(path: string): Promise<BootstrapResult> {
    return invoke<BootstrapResult>("reload_workdir", { path });
  },
  listRegisteredWorkspaces(includeArchived = false): Promise<WorkspaceRegistryListResult> {
    return invoke<WorkspaceRegistryListResult>("list_registered_workspaces", {
      includeArchived,
    });
  },
  getRegisteredWorkspace(workspaceId: string): Promise<WorkspaceRegistryEntryResult> {
    return invoke<WorkspaceRegistryEntryResult>("get_registered_workspace", { workspaceId });
  },
  createRegisteredWorkspace(input: CreateWorkspaceRequest): Promise<WorkspaceMutationResult> {
    return invoke<WorkspaceMutationResult>("create_registered_workspace", { input });
  },
  updateRegisteredWorkspace(
    workspaceId: string,
    input: UpdateWorkspaceRequest,
  ): Promise<WorkspaceMutationResult> {
    return invoke<WorkspaceMutationResult>("update_registered_workspace", {
      workspaceId,
      input,
    });
  },
  deleteRegisteredWorkspace(workspaceId: string): Promise<WorkspaceMutationResult> {
    return invoke<WorkspaceMutationResult>("delete_registered_workspace", { workspaceId });
  },
  restoreRegisteredWorkspace(workspaceId: string): Promise<WorkspaceMutationResult> {
    return invoke<WorkspaceMutationResult>("restore_registered_workspace", { workspaceId });
  },
  openRegisteredWorkspace(workspaceId: string): Promise<BootstrapResult> {
    return invoke<BootstrapResult>("open_registered_workspace", { workspaceId });
  },
  getDashboardOverview(path: string): Promise<DashboardOverviewResult> {
    return invoke<DashboardOverviewResult>("get_dashboard_overview", { path });
  },
  scanWorkspace(path: string): Promise<ScanWorkspaceResult> {
    return invoke<ScanWorkspaceResult>("scan_workspace", { path });
  },
  reconcileS3Workspace(path: string): Promise<S3ReconciliationResult> {
    return invoke<S3ReconciliationResult>("reconcile_s3_workspace", { path });
  },
  reconcileS3WorkspaceById(workspaceId: string): Promise<S3ReconciliationResult> {
    return invoke<S3ReconciliationResult>("reconcile_s3_workspace_by_id", { workspaceId });
  },
  registerS3SourceArtifact(
    path: string,
    input: RegisterS3SourceArtifactRequest,
  ): Promise<RegisterS3SourceArtifactResult> {
    return invoke<RegisterS3SourceArtifactResult>("register_s3_source_artifact", { path, input });
  },
  registerS3SourceArtifactById(
    workspaceId: string,
    input: RegisterS3SourceArtifactRequest,
  ): Promise<RegisterS3SourceArtifactResult> {
    return invoke<RegisterS3SourceArtifactResult>("register_s3_source_artifact_by_id", {
      workspaceId,
      input,
    });
  },
  ensureStageDirectories(path: string): Promise<StageDirectoryProvisionResult> {
    return invoke<StageDirectoryProvisionResult>("ensure_stage_directories", { path });
  },
  getRuntimeSummary(path: string): Promise<RuntimeSummaryResult> {
    return invoke<RuntimeSummaryResult>("get_runtime_summary", { path });
  },
  listStages(path: string): Promise<StageListResult> {
    return invoke<StageListResult>("list_stages", { path });
  },
  getPipelineEditorState(path: string): Promise<PipelineEditorStateResult> {
    return invoke<PipelineEditorStateResult>("get_pipeline_editor_state", { path });
  },
  validatePipelineConfigDraft(
    path: string,
    draft: PipelineConfigDraft,
  ): Promise<ValidatePipelineConfigDraftResult> {
    return invoke<ValidatePipelineConfigDraftResult>("validate_pipeline_config_draft", {
      path,
      draft,
    });
  },
  savePipelineConfig(
    path: string,
    draft: PipelineConfigDraft,
    operatorComment?: string | null,
  ): Promise<SavePipelineConfigResult> {
    return invoke<SavePipelineConfigResult>("save_pipeline_config", {
      path,
      draft,
      operatorComment,
    });
  },
  createS3Stage(
    workspaceId: string,
    input: CreateS3StageRequest,
  ): Promise<CreateS3StageResult> {
    return invoke<CreateS3StageResult>("create_s3_stage", { workspaceId, input });
  },
  updateS3Stage(
    workspaceId: string,
    stageId: string,
    input: UpdateS3StageRequest,
  ): Promise<S3StageMutationResult> {
    return invoke<S3StageMutationResult>("update_s3_stage", { workspaceId, stageId, input });
  },
  deleteS3Stage(workspaceId: string, stageId: string): Promise<S3StageMutationResult> {
    return invoke<S3StageMutationResult>("delete_s3_stage", { workspaceId, stageId });
  },
  restoreS3Stage(workspaceId: string, stageId: string): Promise<S3StageMutationResult> {
    return invoke<S3StageMutationResult>("restore_s3_stage", { workspaceId, stageId });
  },
  updateStageNextStage(
    workspaceId: string,
    stageId: string,
    input: UpdateStageNextStageRequest,
  ): Promise<UpdateStageNextStageResult> {
    return invoke<UpdateStageNextStageResult>("update_stage_next_stage", {
      workspaceId,
      stageId,
      input,
    });
  },
  listEntities(path: string, query?: EntityListQuery): Promise<EntityListResult> {
    return invoke<EntityListResult>("list_entities", { path, query });
  },
  listEntityFiles(path: string, entityId?: string | null): Promise<EntityFilesResult> {
    return invoke<EntityFilesResult>("list_entity_files", { path, entityId });
  },
  getEntity(
    path: string,
    entityId: string,
    selectedFileId?: number | null,
  ): Promise<EntityDetailResult> {
    return invoke<EntityDetailResult>("get_entity", { path, entityId, selectedFileId });
  },
  createNextStageCopy(
    path: string,
    entityId: string,
    sourceStageId: string,
  ): Promise<FileCopyResult> {
    return invoke<FileCopyResult>("create_next_stage_copy", { path, entityId, sourceStageId });
  },
  runDueTasks(path: string): Promise<RunDueTasksResult> {
    return invoke<RunDueTasksResult>("run_due_tasks", { path });
  },
  runDueTasksLimited(path: string, maxTasks: number): Promise<RunDueTasksResult> {
    return invoke<RunDueTasksResult>("run_due_tasks_limited", { path, maxTasks });
  },
  runDueTasksLimitedById(workspaceId: string, maxTasks: number): Promise<RunDueTasksResult> {
    return invoke<RunDueTasksResult>("run_due_tasks_limited_by_id", { workspaceId, maxTasks });
  },
  runPipelineWaves(
    path: string,
    maxWaves: number,
    maxTasksPerWave: number,
    stopOnFirstFailure: boolean,
  ): Promise<RunPipelineWavesResult> {
    return invoke<RunPipelineWavesResult>("run_pipeline_waves", {
      path,
      maxWaves,
      maxTasksPerWave,
      stopOnFirstFailure,
    });
  },
  runPipelineWavesById(
    workspaceId: string,
    maxWaves: number,
    maxTasksPerWave: number,
    stopOnFirstFailure: boolean,
  ): Promise<RunPipelineWavesResult> {
    return invoke<RunPipelineWavesResult>("run_pipeline_waves_by_id", {
      workspaceId,
      maxWaves,
      maxTasksPerWave,
      stopOnFirstFailure,
    });
  },
  runSelectedPipelineWavesById(
    workspaceId: string,
    rootEntityFileIds: number[],
    maxWaves: number,
    maxTasksPerWave: number,
    stopOnFirstFailure: boolean,
  ): Promise<RunSelectedPipelineWavesResult> {
    return invoke<RunSelectedPipelineWavesResult>("run_selected_pipeline_waves_by_id", {
      workspaceId,
      rootEntityFileIds,
      maxWaves,
      maxTasksPerWave,
      stopOnFirstFailure,
    });
  },
  runEntityStage(path: string, entityId: string, stageId: string): Promise<RunEntityStageResult> {
    return invoke<RunEntityStageResult>("run_entity_stage", { path, entityId, stageId });
  },
  retryEntityStageNow(
    path: string,
    entityId: string,
    stageId: string,
    operatorComment?: string | null,
  ): Promise<ManualEntityStageActionResult> {
    return invoke<ManualEntityStageActionResult>("retry_entity_stage_now", {
      path,
      entityId,
      stageId,
      operatorComment,
    });
  },
  resetEntityStageToPending(
    path: string,
    entityId: string,
    stageId: string,
    operatorComment?: string | null,
  ): Promise<ManualEntityStageActionResult> {
    return invoke<ManualEntityStageActionResult>("reset_entity_stage_to_pending", {
      path,
      entityId,
      stageId,
      operatorComment,
    });
  },
  skipEntityStage(
    path: string,
    entityId: string,
    stageId: string,
    operatorComment?: string | null,
  ): Promise<ManualEntityStageActionResult> {
    return invoke<ManualEntityStageActionResult>("skip_entity_stage", {
      path,
      entityId,
      stageId,
      operatorComment,
    });
  },
  openEntityFile(path: string, entityFileId: number): Promise<OpenEntityPathResult> {
    return invoke<OpenEntityPathResult>("open_entity_file", { path, entityFileId });
  },
  openEntityFolder(path: string, entityFileId: number): Promise<OpenEntityPathResult> {
    return invoke<OpenEntityPathResult>("open_entity_folder", { path, entityFileId });
  },
  saveEntityFileBusinessJson(
    path: string,
    entityFileId: number,
    payloadJson: string,
    metaJson: string,
    operatorComment?: string | null,
  ): Promise<SaveEntityFileJsonResult> {
    return invoke<SaveEntityFileJsonResult>("save_entity_file_business_json", {
      path,
      entityFileId,
      payloadJson,
      metaJson,
      operatorComment,
    });
  },
  listStageRuns(path: string, entityId?: string | null): Promise<StageRunsResult> {
    return invoke<StageRunsResult>("list_stage_runs", { path, entityId });
  },
  listStageRunOutputs(workspaceId: string, runId: string): Promise<StageRunOutputsResult> {
    return invoke<StageRunOutputsResult>("list_stage_run_outputs", { workspaceId, runId });
  },
  reconcileStuckTasks(path: string): Promise<ReconcileStuckTasksResult> {
    return invoke<ReconcileStuckTasksResult>("reconcile_stuck_tasks", { path });
  },
  listAppEvents(path: string, limit = 50): Promise<AppEventsResult> {
    return invoke<AppEventsResult>("list_app_events", { path, limit });
  },
  getWorkspaceExplorer(path: string): Promise<WorkspaceExplorerResult> {
    return invoke<WorkspaceExplorerResult>("get_workspace_explorer", { path });
  },
  getWorkspaceExplorerById(workspaceId: string): Promise<WorkspaceExplorerResult> {
    return invoke<WorkspaceExplorerResult>("get_workspace_explorer_by_id", { workspaceId });
  },
};
