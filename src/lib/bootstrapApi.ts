import { apiClient } from "./apiClient";
import type {
  BootstrapResult,
  WorkspaceRegistryEntryResult,
  WorkspaceRegistryListResult,
} from "../types/domain";

export function initializeWorkdir(path: string): Promise<BootstrapResult> {
  return apiClient.initializeWorkdir(path);
}

export function openWorkdir(path: string): Promise<BootstrapResult> {
  return apiClient.openWorkdir(path);
}

export function reloadWorkdir(path: string): Promise<BootstrapResult> {
  return apiClient.reloadWorkdir(path);
}

export function listRegisteredWorkspaces(): Promise<WorkspaceRegistryListResult> {
  return apiClient.listRegisteredWorkspaces();
}

export function getRegisteredWorkspace(
  workspaceId: string,
): Promise<WorkspaceRegistryEntryResult> {
  return apiClient.getRegisteredWorkspace(workspaceId);
}

export function openRegisteredWorkspace(workspaceId: string): Promise<BootstrapResult> {
  return apiClient.openRegisteredWorkspace(workspaceId);
}
