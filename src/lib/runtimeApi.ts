import { invoke } from "@tauri-apps/api/core";

import type {
  AppEventsResult,
  EntityDetailResult,
  EntityFilters,
  EntityListResult,
  RuntimeSummaryResult,
  ScanWorkspaceResult,
  StageListResult,
  WorkspaceExplorerResult,
} from "../types/domain";

export async function scanWorkspace(path: string): Promise<ScanWorkspaceResult> {
  return invoke<ScanWorkspaceResult>("scan_workspace", { path });
}

export async function getRuntimeSummary(path: string): Promise<RuntimeSummaryResult> {
  return invoke<RuntimeSummaryResult>("get_runtime_summary", { path });
}

export async function listStages(path: string): Promise<StageListResult> {
  return invoke<StageListResult>("list_stages", { path });
}

export async function listEntities(
  path: string,
  filters?: EntityFilters,
): Promise<EntityListResult> {
  return invoke<EntityListResult>("list_entities", { path, filters });
}

export async function getEntity(path: string, entityId: string): Promise<EntityDetailResult> {
  return invoke<EntityDetailResult>("get_entity", { path, entityId });
}

export async function listAppEvents(path: string, limit = 50): Promise<AppEventsResult> {
  return invoke<AppEventsResult>("list_app_events", { path, limit });
}

export async function getWorkspaceExplorer(path: string): Promise<WorkspaceExplorerResult> {
  return invoke<WorkspaceExplorerResult>("get_workspace_explorer", { path });
}
