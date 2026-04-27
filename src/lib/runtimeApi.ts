import { invoke } from "@tauri-apps/api/core";

import type {
  AppEventsResult,
  DashboardOverviewResult,
  EntityDetailResult,
  EntityFilesResult,
  EntityListQuery,
  EntityListResult,
  FileCopyResult,
  ManualEntityStageActionResult,
  OpenEntityPathResult,
  ReconcileStuckTasksResult,
  RunDueTasksResult,
  RunEntityStageResult,
  RuntimeSummaryResult,
  SaveEntityFileJsonResult,
  ScanWorkspaceResult,
  StageDirectoryProvisionResult,
  StageListResult,
  StageRunsResult,
  WorkspaceExplorerResult,
} from "../types/domain";

export async function getDashboardOverview(path: string): Promise<DashboardOverviewResult> {
  return invoke<DashboardOverviewResult>("get_dashboard_overview", { path });
}

export async function scanWorkspace(path: string): Promise<ScanWorkspaceResult> {
  return invoke<ScanWorkspaceResult>("scan_workspace", { path });
}

export async function ensureStageDirectories(path: string): Promise<StageDirectoryProvisionResult> {
  return invoke<StageDirectoryProvisionResult>("ensure_stage_directories", { path });
}

export async function getRuntimeSummary(path: string): Promise<RuntimeSummaryResult> {
  return invoke<RuntimeSummaryResult>("get_runtime_summary", { path });
}

export async function listStages(path: string): Promise<StageListResult> {
  return invoke<StageListResult>("list_stages", { path });
}

export async function listEntities(
  path: string,
  query?: EntityListQuery,
): Promise<EntityListResult> {
  return invoke<EntityListResult>("list_entities", { path, query });
}

export async function listEntityFiles(
  path: string,
  entityId?: string | null,
): Promise<EntityFilesResult> {
  return invoke<EntityFilesResult>("list_entity_files", { path, entityId });
}

export async function getEntity(
  path: string,
  entityId: string,
  selectedFileId?: number | null,
): Promise<EntityDetailResult> {
  return invoke<EntityDetailResult>("get_entity", { path, entityId, selectedFileId });
}

export async function createNextStageCopy(
  path: string,
  entityId: string,
  sourceStageId: string,
): Promise<FileCopyResult> {
  return invoke<FileCopyResult>("create_next_stage_copy", {
    path,
    entityId,
    sourceStageId,
  });
}

export async function runDueTasks(path: string): Promise<RunDueTasksResult> {
  return invoke<RunDueTasksResult>("run_due_tasks", { path });
}

export async function runEntityStage(
  path: string,
  entityId: string,
  stageId: string,
): Promise<RunEntityStageResult> {
  return invoke<RunEntityStageResult>("run_entity_stage", { path, entityId, stageId });
}

export async function retryEntityStageNow(
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
}

export async function resetEntityStageToPending(
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
}

export async function skipEntityStage(
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
}

export async function openEntityFile(
  path: string,
  entityFileId: number,
): Promise<OpenEntityPathResult> {
  return invoke<OpenEntityPathResult>("open_entity_file", { path, entityFileId });
}

export async function openEntityFolder(
  path: string,
  entityFileId: number,
): Promise<OpenEntityPathResult> {
  return invoke<OpenEntityPathResult>("open_entity_folder", { path, entityFileId });
}

export async function saveEntityFileBusinessJson(
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
}

export async function listStageRuns(
  path: string,
  entityId?: string | null,
): Promise<StageRunsResult> {
  return invoke<StageRunsResult>("list_stage_runs", { path, entityId });
}

export async function reconcileStuckTasks(path: string): Promise<ReconcileStuckTasksResult> {
  return invoke<ReconcileStuckTasksResult>("reconcile_stuck_tasks", { path });
}

export async function listAppEvents(path: string, limit = 50): Promise<AppEventsResult> {
  return invoke<AppEventsResult>("list_app_events", { path, limit });
}

export async function getWorkspaceExplorer(path: string): Promise<WorkspaceExplorerResult> {
  return invoke<WorkspaceExplorerResult>("get_workspace_explorer", { path });
}
