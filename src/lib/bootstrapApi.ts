import { apiClient } from "./apiClient";
import type {
  BootstrapResult,
  CreateWorkspaceRequest,
  UpdateWorkspaceRequest,
  WorkspaceMutationResult,
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

export function listRegisteredWorkspaces(
  includeArchived = false,
): Promise<WorkspaceRegistryListResult> {
  return apiClient.listRegisteredWorkspaces(includeArchived);
}

export function getRegisteredWorkspace(
  workspaceId: string,
): Promise<WorkspaceRegistryEntryResult> {
  return apiClient.getRegisteredWorkspace(workspaceId);
}

export function createRegisteredWorkspace(
  input: CreateWorkspaceRequest,
): Promise<WorkspaceMutationResult> {
  return apiClient.createRegisteredWorkspace(input);
}

export function updateRegisteredWorkspace(
  workspaceId: string,
  input: UpdateWorkspaceRequest,
): Promise<WorkspaceMutationResult> {
  return apiClient.updateRegisteredWorkspace(workspaceId, input);
}

export function deleteRegisteredWorkspace(workspaceId: string): Promise<WorkspaceMutationResult> {
  return apiClient.deleteRegisteredWorkspace(workspaceId);
}

export function restoreRegisteredWorkspace(workspaceId: string): Promise<WorkspaceMutationResult> {
  return apiClient.restoreRegisteredWorkspace(workspaceId);
}

export function openRegisteredWorkspace(workspaceId: string): Promise<BootstrapResult> {
  return apiClient.openRegisteredWorkspace(workspaceId);
}
